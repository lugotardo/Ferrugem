//! HID boot-protocol keyboard(s): activates the boot report format on each
//! device `hub.rs` finds, polls its interrupt-IN endpoint for 8-byte
//! reports (modifier byte, reserved byte, 6 simultaneous keycode slots),
//! and feeds the decoded keys into the same shared
//! `drivers::keyboard::src::state` machinery x86_64's PS/2 driver uses
//! (modifier tracking, layout, Caps/Num/Scroll Lock LED sync).
//!
//! Several keyboards can be attached at once (see `hub.rs`'s `MAX_KEYBOARDS`
//! and its hot-plug rescan); they all drive one shared `KeyboardState`, the
//! same way a real PC with two keyboards plugged in has one logical console
//! keyboard, not two independent ones.
//!
//! Assumes each keyboard's HID interface is interface 0 for the
//! class-specific `SET_PROTOCOL`/`SET_IDLE`/`SET_REPORT` requests below,
//! true for essentially every plain USB keyboard (a single-interface
//! device), but not guaranteed by the spec for composite devices (e.g. a
//! keyboard with a built-in USB hub or trackpad sharing one device).

use super::dwc2;
use super::protocol::*;
use crate::drivers::keyboard::src::state::{HidReportDecoder, KeyboardState};

const HID_REQ_SET_REPORT: u8 = 0x09;
const HID_REPORT_TYPE_OUTPUT: u16 = 0x02;

/// Bounded so one runaway hub can't loop forever; matches `hub.rs`'s own
/// `MAX_KEYBOARDS`, they're two views of the same limit.
const MAX_DEVICES: usize = 4;
/// Consecutive real transfer errors (not the routine "nothing new yet" NAK
/// case - see `dwc2::interrupt_in_poll`'s doc comment) before giving up on a
/// device even if the hub never reports its port's connection-change bit
/// (e.g. a keyboard that hangs without actually disconnecting).
const ERROR_STREAK_LIMIT: u32 = 20;

struct Device {
    /// The device's control endpoint (ep_num=0) - SET_PROTOCOL/SET_IDLE/
    /// SET_REPORT (LED) requests are control transfers and MUST target
    /// this, never `ep` below: a SETUP token sent to a non-control
    /// endpoint is a protocol violation, and on the x86_64/UHCI sibling of
    /// this driver it wedged the whole host controller (see
    /// `drivers::usb::hid::Device::ep0`'s doc comment) rather than merely
    /// NAKing or stalling.
    ep0: Endpoint,
    ep: Endpoint,
    decoder: HidReportDecoder,
    error_streak: u32,
    /// Expected data toggle for the next interrupt-IN poll; starts at DATA0
    /// (USB 2.0 §5.8.5) and flips on every successfully read report - see
    /// `dwc2::interrupt_in_poll`'s doc comment for why this can't be
    /// hardcoded.
    in_toggle: bool,
}

static mut DEVICES: [Option<Device>; MAX_DEVICES] = [const { None }; MAX_DEVICES];
static mut STATE: KeyboardState = KeyboardState::new();

/// Switch the device into boot protocol / infinite idle rate, and start
/// polling it. Called from `hub.rs` after enumeration (initial or hot-plug)
/// finds a HID boot-protocol keyboard interface. `ep0` is the device's
/// control endpoint, `ep` its interrupt-IN endpoint - see `Device::ep0`'s
/// doc comment for why these can't be the same value.
pub fn attach(ep0: Endpoint, ep: Endpoint) {
    let set_protocol = SetupPacket {
        request_type: DIR_OUT | TYPE_CLASS | RECIP_INTERFACE,
        request: HID_REQ_SET_PROTOCOL,
        value: HID_BOOT_PROTOCOL,
        index: 0,
        length: 0,
    };
    let _ = dwc2::control_transfer(&ep0, &set_protocol, &mut [], false);

    let set_idle = SetupPacket {
        request_type: DIR_OUT | TYPE_CLASS | RECIP_INTERFACE,
        request: HID_REQ_SET_IDLE,
        value: 0, // report only on change, we poll on our own schedule anyway
        index: 0,
        length: 0,
    };
    let _ = dwc2::control_transfer(&ep0, &set_idle, &mut [], false);

    unsafe {
        for slot in DEVICES.iter_mut() {
            if slot.is_none() {
                *slot = Some(Device { ep0, ep, decoder: HidReportDecoder::new(), error_streak: 0, in_toggle: false });
                crate::arch::aarch64::console::print_str("usb: keyboard ready\n");
                return;
            }
        }
    }
    crate::arch::aarch64::console::print_str("usb:   MAX_DEVICES keyboards already attached, ignoring\n");
}

/// Drop whatever device is attached at `hub_addr`/`hub_port`, if any -
/// called by `hub.rs` when its hot-plug rescan sees that port's connection
/// go away.
pub fn detach(hub_addr: u8, hub_port: u8) {
    unsafe {
        for slot in DEVICES.iter_mut() {
            if matches!(slot, Some(d) if d.ep.hub_addr == hub_addr && d.ep.hub_port == hub_port) {
                *slot = None;
            }
        }
    }
}

/// Peek: is a decoded byte available? Safe to call even if no keyboard was
/// ever attached (returns `false`).
pub fn has_key() -> bool {
    unsafe {
        if STATE.has_output() {
            return true;
        }
        poll_all();
        STATE.has_output()
    }
}

/// Consume the next decoded byte, polling hardware if `has_key` wasn't
/// already called this round.
pub fn take_key() -> Option<u8> {
    unsafe {
        if let Some(b) = STATE.pop_byte() {
            return Some(b);
        }
        poll_all();
        STATE.pop_byte()
    }
}

/// Same lazy-poll pattern as `take_key`, for a pending Shift+PageUp/
/// PageDown scrollback request (see `state::KeyboardState::take_scroll`)
/// instead of a byte. Only a USB HID keyboard can trigger this, the UART
/// fallback (`uart_kbd`) carries already-decoded bytes with no Shift/
/// scancode concept to intercept.
pub fn take_scroll() -> Option<i8> {
    unsafe {
        if let Some(d) = STATE.take_scroll() {
            return Some(d);
        }
        poll_all();
        STATE.take_scroll()
    }
}

fn poll_all() {
    unsafe {
        for slot in DEVICES.iter_mut() {
            let Some(dev) = slot else { continue };
            let mut report = [0u8; 8];
            match dwc2::interrupt_in_poll(&dev.ep, &mut dev.in_toggle, &mut report) {
                Ok(n) if n >= 8 => {
                    dev.decoder.feed(&report, &mut STATE);
                    dev.error_streak = 0;
                }
                Ok(_) => dev.error_streak = 0, // short/NAK'd read, not an error
                Err(()) => {
                    dev.error_streak += 1;
                    if dev.error_streak >= ERROR_STREAK_LIMIT {
                        crate::arch::aarch64::console::print_str(
                            "usb:   keyboard stopped responding, detaching\n",
                        );
                        *slot = None;
                    }
                }
            }
        }

        // Checked once per poll round (not per device): flipping one
        // keyboard's Caps Lock should light up every attached keyboard's
        // LED, and `take_led_dirty` only reports `true` once.
        if STATE.take_led_dirty() {
            let led = [STATE.hid_led_mask()];
            for slot in DEVICES.iter() {
                if let Some(dev) = slot {
                    send_led_report(&dev.ep0, led);
                }
            }
        }
    }
}

fn send_led_report(ep: &Endpoint, mut led: [u8; 1]) {
    let setup = SetupPacket {
        request_type: DIR_OUT | TYPE_CLASS | RECIP_INTERFACE,
        request: HID_REQ_SET_REPORT,
        value: (HID_REPORT_TYPE_OUTPUT << 8), // report ID 0
        index: 0,
        length: led.len() as u16,
    };
    let _ = dwc2::control_transfer(ep, &setup, &mut led, false);
}
