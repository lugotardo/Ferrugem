//! HID boot-protocol keyboard(s) on UHCI: activates boot report format on
//! each device `hub.rs` finds, polls its interrupt-IN endpoint for 8-byte
//! reports, and feeds them into the same shared
//! `drivers::keyboard::src::state` machinery the PS/2 driver uses
//! (modifiers, layout, Caps/Num/Scroll Lock LED sync) - see
//! `boards::raspberrypi3::usb::hid` for the DWC2 equivalent, this is
//! structurally the same driver against a different transfer backend.
//!
//! UHCI only has 2 root ports, so at most 2 keyboards can be attached
//! without an external hub (not enumerated yet, see `uhci.rs`'s doc
//! comment); they still both drive one shared `KeyboardState`.

use super::protocol::*;
use super::uhci;
use crate::drivers::keyboard::src::state::{HidReportDecoder, KeyboardState};

const MAX_DEVICES: usize = 2;
/// See the identical constant in `boards::raspberrypi3::usb::hid` - same
/// reasoning: give up on a device that's stopped responding even if the
/// port's own connection-change bit never fires.
const ERROR_STREAK_LIMIT: u32 = 20;

struct Device {
    /// The device's control endpoint (ep_num=0) - SET_PROTOCOL/SET_IDLE/
    /// SET_REPORT (LED) requests are control transfers and MUST target
    /// this, never `ep` below: a SETUP token sent to a non-control
    /// endpoint is a protocol violation that QEMU's UHCI model doesn't
    /// merely NAK or stall, it wedges the whole controller (USBSTS shows
    /// Host Controller Process Error, HCHalted) so every future transfer -
    /// including unrelated ones to other devices - silently stops working.
    ep0: Endpoint,
    ep: Endpoint,
    decoder: HidReportDecoder,
    error_streak: u32,
    /// Expected data toggle for the next interrupt-IN poll; starts at DATA0
    /// (USB 2.0 §5.8.5) and flips on every successfully read report - see
    /// `uhci::interrupt_in_poll`'s doc comment for why this can't be
    /// hardcoded.
    in_toggle: bool,
}

static mut DEVICES: [Option<Device>; MAX_DEVICES] = [const { None }; MAX_DEVICES];
static mut STATE: KeyboardState = KeyboardState::new();

/// Switch the device into boot protocol / infinite idle rate, and start
/// polling it. Called from `hub.rs` after enumeration (initial or
/// hot-plug) finds a HID boot-protocol keyboard interface. `ep0` is the
/// device's control endpoint, `ep` its interrupt-IN endpoint - see
/// `Device::ep0`'s doc comment for why these can't be the same value.
pub fn attach(ep0: Endpoint, ep: Endpoint) {
    let set_protocol = SetupPacket {
        request_type: DIR_OUT | TYPE_CLASS | RECIP_INTERFACE,
        request: HID_REQ_SET_PROTOCOL,
        value: HID_BOOT_PROTOCOL,
        index: 0,
        length: 0,
    };
    let _ = uhci::control_transfer(&ep0, &set_protocol, &mut [], false);

    let set_idle = SetupPacket {
        request_type: DIR_OUT | TYPE_CLASS | RECIP_INTERFACE,
        request: HID_REQ_SET_IDLE,
        value: 0,
        index: 0,
        length: 0,
    };
    let _ = uhci::control_transfer(&ep0, &set_idle, &mut [], false);

    unsafe {
        for slot in DEVICES.iter_mut() {
            if slot.is_none() {
                *slot = Some(Device { ep0, ep, decoder: HidReportDecoder::new(), error_streak: 0, in_toggle: false });
                crate::arch::x86_64::console::print_str("usb: keyboard ready\n");
                return;
            }
        }
    }
    crate::arch::x86_64::console::print_str("usb:   MAX_DEVICES keyboards already attached, ignoring\n");
}

/// Drop whatever device is attached at `dev_addr`, if any - called by
/// `hub.rs` when its hot-plug rescan sees that port's connection go away.
pub fn detach(dev_addr: u8) {
    unsafe {
        for slot in DEVICES.iter_mut() {
            if matches!(slot, Some(d) if d.ep.dev_addr == dev_addr) {
                *slot = None;
            }
        }
    }
}

pub fn has_key() -> bool {
    unsafe {
        if STATE.has_output() {
            return true;
        }
        poll_all();
        STATE.has_output()
    }
}

pub fn take_key() -> Option<u8> {
    unsafe {
        if let Some(b) = STATE.pop_byte() {
            return Some(b);
        }
        poll_all();
        STATE.pop_byte()
    }
}

fn poll_all() {
    unsafe {
        for slot in DEVICES.iter_mut() {
            let Some(dev) = slot else { continue };
            let mut report = [0u8; 8];
            match uhci::interrupt_in_poll(&dev.ep, &mut dev.in_toggle, &mut report) {
                Ok(n) if n >= 8 => {
                    dev.decoder.feed(&report, &mut STATE);
                    dev.error_streak = 0;
                }
                Ok(_) => dev.error_streak = 0,
                Err(()) => {
                    dev.error_streak += 1;
                    if dev.error_streak >= ERROR_STREAK_LIMIT {
                        crate::arch::x86_64::console::print_str(
                            "usb:   keyboard stopped responding, detaching\n",
                        );
                        *slot = None;
                    }
                }
            }
        }

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
        value: HID_REPORT_TYPE_OUTPUT << 8, // report ID 0
        index: 0,
        length: led.len() as u16,
    };
    let _ = uhci::control_transfer(ep, &setup, &mut led, false);
}
