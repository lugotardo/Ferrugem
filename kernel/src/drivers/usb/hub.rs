//! Root-port enumeration for the UHCI driver: standard control requests to
//! bring a newly connected device up to a real address and classify it,
//! plus a best-effort hot-plug rescan.
//!
//! Unlike `boards::raspberrypi3::usb::hub` there's no real hub *chip* to
//! send hub-class requests to here - UHCI's root hub is just the
//! controller's own 2 `PORTSC` registers (`uhci::port_connection`,
//! `uhci::reset_and_enable_port`), so this module is considerably smaller.
//! An external hub plugged into one of those 2 ports is not enumerated
//! (see `uhci.rs`'s module doc comment on why this driver is root-hub-only
//! for now).

use super::protocol::*;
use super::uhci;
use super::hid;

static mut NEXT_ADDR: u8 = 1;
/// Which device address (if any) is currently attached to each of the 2
/// root ports, so a hot-plug rescan knows what to tear down.
static mut PORT_DEVICE: [Option<u8>; 2] = [None; 2];

fn alloc_addr() -> u8 {
    unsafe {
        let a = NEXT_ADDR;
        NEXT_ADDR += 1;
        a
    }
}

fn log(s: &str) {
    crate::arch::x86_64::console::print_str(s);
}

fn get_descriptor(ep: &Endpoint, desc_type: u8, index: u8, buf: &mut [u8]) -> uhci::Result<usize> {
    let setup = SetupPacket {
        request_type: DIR_IN | TYPE_STANDARD | RECIP_DEVICE,
        request: REQ_GET_DESCRIPTOR,
        value: ((desc_type as u16) << 8) | index as u16,
        index: 0,
        length: buf.len() as u16,
    };
    uhci::control_transfer(ep, &setup, buf, true)
}

fn set_address(ep: &Endpoint, new_addr: u8) -> uhci::Result<()> {
    let setup = SetupPacket {
        request_type: DIR_OUT | TYPE_STANDARD | RECIP_DEVICE,
        request: REQ_SET_ADDRESS,
        value: new_addr as u16,
        index: 0,
        length: 0,
    };
    uhci::control_transfer(ep, &setup, &mut [], false)?;
    Ok(())
}

fn set_configuration(ep: &Endpoint, config: u8) -> uhci::Result<()> {
    let setup = SetupPacket {
        request_type: DIR_OUT | TYPE_STANDARD | RECIP_DEVICE,
        request: REQ_SET_CONFIGURATION,
        value: config as u16,
        index: 0,
        length: 0,
    };
    uhci::control_transfer(ep, &setup, &mut [], false)?;
    Ok(())
}

/// Entry point: bring up whatever's connected to either of the
/// controller's 2 root ports at boot time.
pub fn enumerate_root() {
    for port in 1..=uhci::port_count() {
        bring_up_port(port);
    }
}

/// Re-check both root ports for a connection-status change since the last
/// call. Meant to be called occasionally, not on every keystroke poll (see
/// `usb::mod`'s throttling), each check is one PORTSC read - cheap, but not
/// free.
pub fn poll_hotplug() {
    for port in 1..=uhci::port_count() {
        let (_, changed) = uhci::port_connection(port);
        if !changed {
            continue;
        }
        uhci::ack_port_connection_change(port);

        if let Some(addr) = unsafe { PORT_DEVICE[port as usize - 1].take() } {
            hid::detach(addr);
        }
        bring_up_port(port);
    }
}

fn bring_up_port(port: u8) {
    let Some(speed) = uhci::reset_and_enable_port(port) else {
        return; // nothing connected, or it didn't come up
    };
    log("usb:   port ");
    log_dec(port as u32);
    log(": device enabled\n");

    let addr = alloc_addr();
    if let Some((ep0, kbd_ep)) = bring_up_device(speed, addr) {
        unsafe { PORT_DEVICE[port as usize - 1] = Some(addr) };
        hid::attach(ep0, kbd_ep);
    }
}

fn log_dec(mut n: u32) {
    let mut buf = [0u8; 10];
    let mut i = 10;
    if n == 0 {
        log("0");
        return;
    }
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    if let Ok(s) = core::str::from_utf8(&buf[i..]) {
        log(s);
    }
}

/// Bring a freshly reset device (still at address 0) up to a real address
/// and, if it's a HID boot-protocol keyboard, return its interrupt-IN
/// endpoint; anything else is silently left unconfigured and ignored.
fn bring_up_device(speed: Speed, addr: u8) -> Option<(Endpoint, Endpoint)> {
    // Low-speed and full-speed ep0 both start with an 8-byte max packet
    // assumption here; unlike DWC2's board this driver never sees a
    // High-speed device (UHCI is USB 1.1-only), so there's no
    // bMaxPacketSize0-from-descriptor step needed before SET_ADDRESS - 8 is
    // always safe as the very first request's packet size regardless of
    // what the device later reports.
    let probe = Endpoint::control(0, 8, speed);
    let mut dev_desc8 = [0u8; 8];
    if get_descriptor(&probe, DESC_DEVICE, 0, &mut dev_desc8).is_err() {
        log("usb:   GET_DESCRIPTOR(8) failed on new device\n");
        return None;
    }
    let max_packet0 = dev_desc8[7].max(8) as u16;

    if set_address(&probe, addr).is_err() {
        log("usb:   SET_ADDRESS failed\n");
        return None;
    }
    crate::drivers::timer::src::pit::delay_ms(10); // recovery time after SET_ADDRESS (spec minimum 2 ms)

    let ep0 = Endpoint::control(addr, max_packet0, speed);
    let mut dev_desc = [0u8; 18];
    if get_descriptor(&ep0, DESC_DEVICE, 0, &mut dev_desc).is_err() {
        log("usb:   GET_DESCRIPTOR(device) failed after SET_ADDRESS\n");
        return None;
    }

    let kbd_ep = enumerate_as_hid_keyboard(ep0)?;
    Some((ep0, kbd_ep))
}

/// Walk a device's configuration descriptor looking for a HID
/// boot-protocol keyboard interface, and if found, its interrupt-IN
/// endpoint. `SET_CONFIGURATION` is only issued once that endpoint is
/// actually located; a device that isn't a keyboard is left unconfigured.
fn enumerate_as_hid_keyboard(ep0: Endpoint) -> Option<Endpoint> {
    let mut hdr = [0u8; 9];
    if get_descriptor(&ep0, DESC_CONFIGURATION, 0, &mut hdr).is_err() {
        return None;
    }
    let total_len = u16::from_le_bytes([hdr[2], hdr[3]]) as usize;
    if total_len < 9 || total_len > uhci::MAX_XFER {
        log("usb:   config descriptor size out of expected range, skipping device\n");
        return None;
    }

    let mut cfg = [0u8; uhci::MAX_XFER];
    let cfg = &mut cfg[..total_len];
    if get_descriptor(&ep0, DESC_CONFIGURATION, 0, cfg).is_err() {
        return None;
    }
    let config_value = cfg[5];

    let mut i = 0usize;
    let mut in_keyboard_iface = false;
    let mut found_ep: Option<(u8, u16)> = None;

    while i + 2 <= total_len {
        let len = cfg[i] as usize;
        if len == 0 {
            break;
        }
        let dtype = cfg[i + 1];
        if dtype == 4 && i + 9 <= total_len {
            let class = cfg[i + 5];
            let subclass = cfg[i + 6];
            let proto = cfg[i + 7];
            in_keyboard_iface =
                class == CLASS_HID && subclass == HID_SUBCLASS_BOOT && proto == HID_PROTOCOL_KEYBOARD;
        } else if dtype == 5 && in_keyboard_iface && found_ep.is_none() && i + 7 <= total_len {
            let ep_addr = cfg[i + 2];
            let attrs = cfg[i + 3];
            let max_pkt = u16::from_le_bytes([cfg[i + 4], cfg[i + 5]]);
            if ep_addr & 0x80 != 0 && attrs & 0x3 == 3 {
                found_ep = Some((ep_addr & 0x0F, max_pkt));
            }
        }
        i += len;
    }

    let (ep_num, max_pkt) = found_ep?;

    if set_configuration(&ep0, config_value).is_err() {
        log("usb:   SET_CONFIGURATION failed\n");
        return None;
    }

    log("usb:   HID boot keyboard found: addr=");
    log_dec(ep0.dev_addr as u32);
    log(" ep=");
    log_dec(ep_num as u32);
    log("\n");

    Some(Endpoint { dev_addr: ep0.dev_addr, ep_num, max_packet: max_pkt, speed: ep0.speed })
}
