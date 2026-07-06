//! Enumeration for the UHCI driver: standard control requests to bring a
//! newly connected device up to a real address and classify it (HID boot
//! keyboard, Mass Storage, or an external hub to recurse into), plus a
//! best-effort hot-plug rescan of the controller's own 2 root ports.
//!
//! UHCI's root hub is just the controller's own 2 `PORTSC` registers
//! (`uhci::port_connection`, `uhci::reset_and_enable_port`), not a real hub
//! *chip* - `bring_up_port` talks to those directly. A real external hub
//! plugged into one of those 2 ports *is* enumerated, though: unlike
//! `boards::raspberrypi3::usb::hub`'s DWC2 root port (always High-speed,
//! needing split transactions for anything slower behind a hub), UHCI is
//! USB 1.1-only end to end - root port, any hub, and every device behind it
//! are all Full/Low speed, so a `uhci::Endpoint` never needs hub-routing
//! fields at all (see `protocol.rs`'s module doc comment). That keeps hub
//! support here notably simpler than the RPi3 board's version, structurally
//! it's the same recursive `bring_up_device` → `enumerate_hub_ports` →
//! `bring_up_hub_port` → `bring_up_device` walk, just without split
//! transactions to thread through.
//!
//! Known limitation, same one RPi3's hub driver accepts: hot-plug rescanning
//! only re-checks the controller's own 2 root ports (see `poll_hotplug`).
//! Unplugging a hub whose children were enumerated once doesn't cascade-
//! detach them from `hid`/`msc` - their driver-side slots simply become
//! unreachable dead entries rather than being freed, which can exhaust
//! `hid`/`msc`'s small fixed device tables after enough plug/unplug cycles,
//! but doesn't corrupt anything (a stale entry just stops responding).

use super::protocol::*;
use super::uhci;
use super::hid;
use super::msc;

/// Which driver a root port's device was handed off to, so a hot-plug
/// rescan knows which `detach()` to call. `Hub` itself is never detached
/// individually (see this module's doc comment on the hot-plug limitation);
/// it's tracked only so `bring_up_port` can tell "something is here and was
/// handled" apart from "nothing this driver understands was found."
#[derive(Clone, Copy)]
enum Attached {
    Keyboard,
    MassStorage,
    Hub,
}

static mut NEXT_ADDR: u8 = 1;
/// Which device address (if any) is currently attached to each of the 2
/// root ports, so a hot-plug rescan knows what to tear down.
static mut PORT_DEVICE: [Option<(u8, Attached)>; 2] = [None; 2];

/// Hub descriptors, port counts, and other topology bookkeeping don't need
/// anything nearly as large as `uhci::MAX_XFER` (256 B, sized for
/// descriptors HID/MSC devices return) - keeping a small dedicated cap here
/// just documents that intent rather than reusing an unrelated constant.
const MAX_HUB_PORTS: usize = 8;

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

fn get_hub_descriptor(ep: &Endpoint, buf: &mut [u8]) -> uhci::Result<usize> {
    let setup = SetupPacket {
        request_type: DIR_IN | TYPE_CLASS | RECIP_DEVICE,
        request: HUB_REQ_GET_DESCRIPTOR,
        value: (DESC_HUB as u16) << 8,
        index: 0,
        length: buf.len() as u16,
    };
    uhci::control_transfer(ep, &setup, buf, true)
}

fn set_port_feature(ep: &Endpoint, port: u8, feature: u16) -> uhci::Result<()> {
    let setup = SetupPacket {
        request_type: DIR_OUT | TYPE_CLASS | RECIP_OTHER,
        request: HUB_REQ_SET_PORT_FEATURE,
        value: feature,
        index: port as u16,
        length: 0,
    };
    uhci::control_transfer(ep, &setup, &mut [], false)?;
    Ok(())
}

fn clear_port_feature(ep: &Endpoint, port: u8, feature: u16) -> uhci::Result<()> {
    let setup = SetupPacket {
        request_type: DIR_OUT | TYPE_CLASS | RECIP_OTHER,
        request: HUB_REQ_CLEAR_PORT_FEATURE,
        value: feature,
        index: port as u16,
        length: 0,
    };
    uhci::control_transfer(ep, &setup, &mut [], false)?;
    Ok(())
}

/// Returns `(wPortStatus, wPortChange)`, the full 32-bit hub port status
/// word (USB 2.0 §11.24.2.7) split into its two 16-bit halves.
fn get_port_status_and_change(ep: &Endpoint, port: u8) -> uhci::Result<(u16, u16)> {
    let mut buf = [0u8; 4];
    let setup = SetupPacket {
        request_type: DIR_IN | TYPE_CLASS | RECIP_OTHER,
        request: REQ_GET_STATUS,
        value: 0,
        index: port as u16,
        length: 4,
    };
    uhci::control_transfer(ep, &setup, &mut buf, true)?;
    Ok((u16::from_le_bytes([buf[0], buf[1]]), u16::from_le_bytes([buf[2], buf[3]])))
}

/// Bit 9 of wPortStatus (USB 2.0 table 11-21): Low-Speed Device Attached.
/// No High-Speed bit to check here (unlike RPi3's DWC2 root, see this
/// module's doc comment) - anything that isn't reported Low-speed on a
/// UHCI-topology hub is Full-speed.
fn hub_port_speed(status: u16) -> Speed {
    if status & (1 << 9) != 0 { Speed::Low } else { Speed::Full }
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

        if let Some((addr, kind)) = unsafe { PORT_DEVICE[port as usize - 1].take() } {
            match kind {
                Attached::Keyboard    => hid::detach(addr),
                Attached::MassStorage => msc::detach(addr),
                // No cascading detach for a hub's children - see this
                // module's doc comment on that accepted limitation.
                Attached::Hub         => {}
            }
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
    if let Some(kind) = bring_up_device(speed, addr) {
        unsafe { PORT_DEVICE[port as usize - 1] = Some((addr, kind)) };
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

/// Bring a freshly reset device (still at address 0) up to a real address,
/// then classify it: a hub recurses into its own downstream ports (each of
/// which can itself hold another hub, another keyboard, another disk, and
/// so on - there's no depth limit here, only `alloc_addr`'s `u8` address
/// space, same as real USB), a HID boot keyboard or Mass Storage interface
/// hands off to that driver's `attach()`. Anything else - or a match whose
/// own `attach()` fails - is silently left unconfigured and ignored.
fn bring_up_device(speed: Speed, addr: u8) -> Option<Attached> {
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

    if dev_desc[4] == CLASS_HUB {
        log("usb:   addr ");
        log_dec(addr as u32);
        log(" is a hub, enumerating its ports\n");
        enumerate_hub_ports(ep0);
        return Some(Attached::Hub);
    }

    if let Some(kbd_ep) = enumerate_as_hid_keyboard(ep0) {
        hid::attach(ep0, kbd_ep);
        return Some(Attached::Keyboard);
    }
    if let Some((bulk_in, bulk_out)) = enumerate_as_msc(ep0) {
        if msc::attach(ep0, bulk_in, bulk_out) {
            return Some(Attached::MassStorage);
        }
    }
    None
}

/// Read a hub's own class descriptor to learn its downstream port count,
/// configure it (hubs universally have exactly one configuration, value 1 -
/// same assumption RPi3's `hub.rs` makes), then bring up whatever's plugged
/// into each of its ports. Devices found here (including nested hubs) are
/// enumerated through the exact same `bring_up_device` used for root ports.
fn enumerate_hub_ports(ep0: Endpoint) {
    let mut desc = [0u8; 9];
    if get_hub_descriptor(&ep0, &mut desc).is_err() {
        log("usb:   GET_HUB_DESCRIPTOR failed\n");
        return;
    }
    let num_ports = (desc[2] as usize).min(MAX_HUB_PORTS) as u8;
    // bPwrOn2PwrGood (desc[5]) is in 2 ms units; floor it at 20 ms so a hub
    // that reports an unrealistically small value still gets a sane settle
    // time (same floor RPi3's `hub.rs` uses).
    let settle_ms = (desc[5] as u32 * 2).max(20);

    let _ = set_configuration(&ep0, 1);

    log("usb:   hub has ");
    log_dec(num_ports as u32);
    log(" ports\n");

    for port in 1..=num_ports {
        bring_up_hub_port(&ep0, port, settle_ms);
    }
}

fn bring_up_hub_port(ep0: &Endpoint, port: u8, settle_ms: u32) {
    if set_port_feature(ep0, port, FEATURE_PORT_POWER).is_err() {
        return;
    }
    crate::drivers::timer::src::pit::delay_ms(settle_ms);

    let (status, _) = match get_port_status_and_change(ep0, port) {
        Ok(v) => v,
        Err(_) => return,
    };
    if status & 1 == 0 {
        return; // nothing plugged into this port
    }

    log("usb:   hub port ");
    log_dec(port as u32);
    log(": device connected, resetting...\n");

    if set_port_feature(ep0, port, FEATURE_PORT_RESET).is_err() {
        return;
    }
    crate::drivers::timer::src::pit::delay_ms(50); // reset pulse width
    let _ = clear_port_feature(ep0, port, FEATURE_C_PORT_RESET);
    crate::drivers::timer::src::pit::delay_ms(10); // reset recovery time

    let (status, _) = match get_port_status_and_change(ep0, port) {
        Ok(v) => v,
        Err(_) => return,
    };
    if status & (1 << 1) == 0 {
        log("usb:   hub port did not enable after reset\n");
        return;
    }

    bring_up_device(hub_port_speed(status), alloc_addr());
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

/// Walk a device's configuration descriptor looking for a Mass Storage /
/// SCSI transparent / Bulk-Only Transport interface, and if found, its
/// bulk IN and bulk OUT endpoints. `SET_CONFIGURATION` is only issued once
/// both endpoints are actually located; a device that isn't a recognized
/// MSC device is left unconfigured. Structurally the same walk as
/// `enumerate_as_hid_keyboard`, just matching a different interface class
/// and endpoint transfer type (bulk, not interrupt) - and needing two
/// endpoints instead of one.
fn enumerate_as_msc(ep0: Endpoint) -> Option<(Endpoint, Endpoint)> {
    let mut hdr = [0u8; 9];
    if get_descriptor(&ep0, DESC_CONFIGURATION, 0, &mut hdr).is_err() {
        return None;
    }
    let total_len = u16::from_le_bytes([hdr[2], hdr[3]]) as usize;
    if total_len < 9 || total_len > uhci::MAX_XFER {
        return None;
    }

    let mut cfg = [0u8; uhci::MAX_XFER];
    let cfg = &mut cfg[..total_len];
    if get_descriptor(&ep0, DESC_CONFIGURATION, 0, cfg).is_err() {
        return None;
    }
    let config_value = cfg[5];

    let mut i = 0usize;
    let mut in_msc_iface = false;
    let mut bulk_in: Option<(u8, u16)> = None;
    let mut bulk_out: Option<(u8, u16)> = None;

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
            in_msc_iface =
                class == CLASS_MSC && subclass == MSC_SUBCLASS_SCSI && proto == MSC_PROTOCOL_BULK_ONLY;
        } else if dtype == 5 && in_msc_iface && i + 7 <= total_len {
            let ep_addr = cfg[i + 2];
            let attrs = cfg[i + 3];
            let max_pkt = u16::from_le_bytes([cfg[i + 4], cfg[i + 5]]);
            if attrs & 0x3 == 0x2 {
                // bulk transfer type (USB 2.0 table 9-13)
                if ep_addr & 0x80 != 0 {
                    if bulk_in.is_none() { bulk_in = Some((ep_addr & 0x0F, max_pkt)); }
                } else if bulk_out.is_none() {
                    bulk_out = Some((ep_addr & 0x0F, max_pkt));
                }
            }
        }
        i += len;
    }

    let (in_num, in_mps) = bulk_in?;
    let (out_num, out_mps) = bulk_out?;

    if set_configuration(&ep0, config_value).is_err() {
        log("usb:   SET_CONFIGURATION failed (MSC)\n");
        return None;
    }

    log("usb:   MSC bulk-only interface found: addr=");
    log_dec(ep0.dev_addr as u32);
    log("\n");

    Some((
        Endpoint { dev_addr: ep0.dev_addr, ep_num: in_num, max_packet: in_mps, speed: ep0.speed },
        Endpoint { dev_addr: ep0.dev_addr, ep_num: out_num, max_packet: out_mps, speed: ep0.speed },
    ))
}
