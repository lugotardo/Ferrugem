//! USB enumeration: standard control requests, the hub class driver, and
//! the recursive walk that finds every HID boot-protocol keyboard anywhere
//! in the topology (not just the first - see `add_keyboard`), plus a
//! best-effort hot-plug rescan of the root hub's own ports.
//!
//! On a real Raspberry Pi 3, the device on the root port is always the
//! onboard SMSC LAN9514 (USB hub + Ethernet), its downstream ports are
//! where the 4 physical USB-A jacks actually land. This code doesn't
//! special-case that chip at all: it just speaks generic USB hub class
//! requests, so it also handles a user plugging in an external hub instead.
//! Hot-plug rescanning (`poll_hotplug`) only re-checks that root hub's own
//! ports though, on the (accurate, for this board) assumption that they're
//! the only place anything can physically be plugged/unplugged after boot;
//! a hub nested deeper than that is enumerated once when first found but
//! not itself re-polled.

use super::dwc2;
use super::hid;
use super::msc;
use super::protocol::*;

const MAX_PORTS: usize = 8;
/// Matches `hid.rs`'s `MAX_DEVICES` - two views of the same limit.
const MAX_KEYBOARDS: usize = 4;
/// Matches `msc.rs`'s `MAX_DEVICES` - same reasoning as `MAX_KEYBOARDS`.
const MAX_DISKS: usize = 4;

static mut NEXT_ADDR: u8 = 1;

/// The one hub this driver actively re-polls for hot-plug changes (see the
/// module doc comment on why one level is enough for this board).
struct RootHub {
    ep0: Endpoint,
    num_ports: u8,
}
static mut ROOT_HUB: Option<RootHub> = None;

/// Which of `MAX_KEYBOARDS` slots is attached where, so a hot-plug removal
/// on a given hub/port can tell `hid.rs` exactly which device went away.
static mut KEYBOARD_LOCATIONS: [Option<(u8, u8)>; MAX_KEYBOARDS] = [None; MAX_KEYBOARDS];
/// Same bookkeeping as `KEYBOARD_LOCATIONS`, for Mass Storage devices.
static mut DISK_LOCATIONS: [Option<(u8, u8)>; MAX_DISKS] = [None; MAX_DISKS];

fn alloc_addr() -> u8 {
    unsafe {
        let a = NEXT_ADDR;
        NEXT_ADDR += 1;
        a
    }
}

fn log(s: &str) {
    crate::arch::aarch64::console::print_str(s);
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

fn get_descriptor(ep: &Endpoint, desc_type: u8, index: u8, buf: &mut [u8]) -> dwc2::Result<usize> {
    let setup = SetupPacket {
        request_type: DIR_IN | TYPE_STANDARD | RECIP_DEVICE,
        request: REQ_GET_DESCRIPTOR,
        value: ((desc_type as u16) << 8) | index as u16,
        index: 0,
        length: buf.len() as u16,
    };
    dwc2::control_transfer(ep, &setup, buf, true)
}

fn set_address(ep: &Endpoint, new_addr: u8) -> dwc2::Result<()> {
    let setup = SetupPacket {
        request_type: DIR_OUT | TYPE_STANDARD | RECIP_DEVICE,
        request: REQ_SET_ADDRESS,
        value: new_addr as u16,
        index: 0,
        length: 0,
    };
    dwc2::control_transfer(ep, &setup, &mut [], false)?;
    Ok(())
}

fn set_configuration(ep: &Endpoint, config: u8) -> dwc2::Result<()> {
    let setup = SetupPacket {
        request_type: DIR_OUT | TYPE_STANDARD | RECIP_DEVICE,
        request: REQ_SET_CONFIGURATION,
        value: config as u16,
        index: 0,
        length: 0,
    };
    dwc2::control_transfer(ep, &setup, &mut [], false)?;
    Ok(())
}

fn get_hub_descriptor(ep: &Endpoint, buf: &mut [u8]) -> dwc2::Result<usize> {
    let setup = SetupPacket {
        request_type: DIR_IN | TYPE_CLASS | RECIP_DEVICE,
        request: HUB_REQ_GET_DESCRIPTOR,
        value: (DESC_HUB as u16) << 8,
        index: 0,
        length: buf.len() as u16,
    };
    dwc2::control_transfer(ep, &setup, buf, true)
}

fn set_port_feature(ep: &Endpoint, port: u8, feature: u16) -> dwc2::Result<()> {
    let setup = SetupPacket {
        request_type: DIR_OUT | TYPE_CLASS | RECIP_OTHER,
        request: HUB_REQ_SET_PORT_FEATURE,
        value: feature,
        index: port as u16,
        length: 0,
    };
    dwc2::control_transfer(ep, &setup, &mut [], false)?;
    Ok(())
}

fn clear_port_feature(ep: &Endpoint, port: u8, feature: u16) -> dwc2::Result<()> {
    let setup = SetupPacket {
        request_type: DIR_OUT | TYPE_CLASS | RECIP_OTHER,
        request: HUB_REQ_CLEAR_PORT_FEATURE,
        value: feature,
        index: port as u16,
        length: 0,
    };
    dwc2::control_transfer(ep, &setup, &mut [], false)?;
    Ok(())
}

/// Returns `(wPortStatus, wPortChange)`, the full 32-bit hub port status
/// word (USB 2.0 section 11.24.2.7) split into its two 16-bit halves.
fn get_port_status_and_change(ep: &Endpoint, port: u8) -> dwc2::Result<(u16, u16)> {
    let mut buf = [0u8; 4];
    let setup = SetupPacket {
        request_type: DIR_IN | TYPE_CLASS | RECIP_OTHER,
        request: REQ_GET_STATUS,
        value: 0,
        index: port as u16,
        length: 4,
    };
    dwc2::control_transfer(ep, &setup, &mut buf, true)?;
    Ok((u16::from_le_bytes([buf[0], buf[1]]), u16::from_le_bytes([buf[2], buf[3]])))
}

fn port_speed(status: u16) -> Speed {
    if status & (1 << 9) != 0 {
        Speed::Low
    } else if status & (1 << 10) != 0 {
        Speed::High
    } else {
        Speed::Full
    }
}

fn add_keyboard(ep0: Endpoint, ep: Endpoint) {
    unsafe {
        for loc in KEYBOARD_LOCATIONS.iter_mut() {
            if loc.is_none() {
                *loc = Some((ep.hub_addr, ep.hub_port));
                hid::attach(ep0, ep);
                return;
            }
        }
    }
    log("usb:   keyboard found but MAX_KEYBOARDS already attached, ignoring\n");
}

fn remove_keyboard_at(hub_addr: u8, hub_port: u8) {
    unsafe {
        for loc in KEYBOARD_LOCATIONS.iter_mut() {
            if *loc == Some((hub_addr, hub_port)) {
                *loc = None;
            }
        }
    }
    hid::detach(hub_addr, hub_port);
}

fn add_disk(ep0: Endpoint, bulk_in: Endpoint, bulk_out: Endpoint) {
    unsafe {
        for loc in DISK_LOCATIONS.iter_mut() {
            if loc.is_none() {
                if msc::attach(ep0, bulk_in, bulk_out) {
                    *loc = Some((bulk_in.hub_addr, bulk_in.hub_port));
                }
                return;
            }
        }
    }
    log("usb:   disk found but MAX_DISKS already attached, ignoring\n");
}

fn remove_disk_at(hub_addr: u8, hub_port: u8) {
    unsafe {
        for loc in DISK_LOCATIONS.iter_mut() {
            if *loc == Some((hub_addr, hub_port)) {
                *loc = None;
            }
        }
    }
    msc::detach(hub_addr, hub_port);
}

/// Entry point: enumerate whatever's on the root port (a Raspberry Pi 3
/// always has the LAN9514 hub there) and recurse into it collecting every
/// HID boot-protocol keyboard found. Called once at boot; see
/// `poll_hotplug` for what happens to connections made afterward.
pub fn enumerate_root() {
    let speed = dwc2::root_speed();
    bring_up_device(speed, 0, 0, alloc_addr());
}

/// Re-check the root hub's own ports for a connection-status change since
/// the last call, and (dis)enumerate accordingly. Best-effort and cheap
/// when nothing changed (one GET_STATUS control transfer per port); meant
/// to be called occasionally from the input poll path, not every keystroke
/// (see `boards::raspberrypi3::usb::maybe_poll_hotplug`'s throttling).
pub fn poll_hotplug() {
    let (ep0, num_ports) = unsafe {
        match &ROOT_HUB {
            Some(h) => (h.ep0, h.num_ports),
            None => return, // root device wasn't a hub (or enumeration failed); nothing to rescan
        }
    };

    for port in 1..=num_ports {
        let (status, change) = match get_port_status_and_change(&ep0, port) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if change & FEATURE_C_PORT_CONNECTION_BIT == 0 {
            continue;
        }
        let _ = clear_port_feature(&ep0, port, FEATURE_C_PORT_CONNECTION);

        // Whatever was here before (if anything) is gone now, whether this
        // change is a fresh connection or a disconnection: either way the
        // old device's address is no longer valid.
        remove_keyboard_at(ep0.dev_addr, port);
        remove_disk_at(ep0.dev_addr, port);

        if status & 1 != 0 {
            log("usb:   hot-plug: port ");
            log_dec(port as u32);
            log(" connected\n");
            bring_up_hub_port(&ep0, port, 20);
        } else {
            log("usb:   hot-plug: port ");
            log_dec(port as u32);
            log(" disconnected\n");
        }
    }
}

/// Bit 0 of wPortChange (USB 2.0 table 11-22): C_PORT_CONNECTION.
const FEATURE_C_PORT_CONNECTION_BIT: u16 = 1 << 0;

/// Bring a freshly connected/reset device (still at address 0) up to a real
/// address, then classify it: a hub recurses into its own ports, a HID
/// boot keyboard is registered via `add_keyboard`, anything else is
/// silently skipped.
fn bring_up_device(speed: Speed, hub_addr: u8, hub_port: u8, addr: u8) {
    let probe = Endpoint::control(0, 8, speed, hub_addr, hub_port);

    let mut first8 = [0u8; 8];
    if get_descriptor(&probe, DESC_DEVICE, 0, &mut first8).is_err() {
        log("usb:   GET_DESCRIPTOR(8) failed on new device\n");
        return;
    }
    // Low-speed ep0 max packet is architecturally fixed at 8 by the USB
    // spec; for Full/High speed, byte 7 of the descriptor we just read
    // (bMaxPacketSize0) is authoritative.
    let max_packet0 = if speed == Speed::Low { 8 } else { first8[7] as u16 };
    log("usb:   bMaxPacketSize0=");
    log_dec(max_packet0 as u32);
    log("\n");

    // Real hardware behavior QEMU's raspi3b (no DWC2 model at all) never
    // exercised: plenty of real devices, and the hubs they hang off, are
    // flaky right after SET_ADDRESS - needing more than one attempt and
    // more than the spec's 2 ms minimum recovery time before they reliably
    // respond at their new address. The same well-known case Linux's
    // usbcore retries for, not necessarily a hardware/driver bug.
    //
    // Retry the whole SET_ADDRESS+GET_DESCRIPTOR pair, not just the read,
    // and don't treat a failed SET_ADDRESS as immediately fatal: if an
    // earlier attempt's SETUP went through but we missed its STATUS
    // response, the device may already be listening at `addr` even though
    // *this* SET_ADDRESS call (re-sent to address 0, which it may no
    // longer answer to) reports an error - GET_DESCRIPTOR at the target
    // address is the real, unambiguous test either way.
    let ep0 = Endpoint::control(addr, max_packet0, speed, hub_addr, hub_port);
    let mut dev_desc = [0u8; 18];
    const RECOVERY_DELAYS_MS: [u64; 5] = [50, 100, 200, 500, 1000];
    let mut ready = false;
    for delay_ms in RECOVERY_DELAYS_MS {
        // Only meaningful for the device directly on the root port (a
        // downstream hub port's equivalent state lives in that hub's own
        // port status word, not HPRT): if repeated identical failures are
        // actually the root port itself having dropped `HPRT_ENA` (some
        // hardware auto-disables a port on a fault condition like the
        // BABBLE seen on `SET_ADDRESS`'s own status stage), no amount of
        // retrying the request alone would ever succeed - only a fresh
        // port reset recovers it.
        if hub_addr == 0 && !dwc2::root_port_ok() {
            log("usb:   root port dropped mid-enumeration, recovering...\n");
            if !dwc2::recover_root_port() {
                return;
            }
        }
        if set_address(&probe, addr).is_err() {
            log("usb:   SET_ADDRESS failed, checking if device answers anyway...\n");
        }
        dwc2::delay_ms(delay_ms);
        if get_descriptor(&ep0, DESC_DEVICE, 0, &mut dev_desc).is_ok() {
            ready = true;
            break;
        }
        log("usb:   GET_DESCRIPTOR(device) failed after SET_ADDRESS, retrying...\n");
    }
    if !ready {
        log("usb:   device never came up after SET_ADDRESS, giving up\n");
        return;
    }

    if dev_desc[4] == CLASS_HUB {
        log("usb:   addr ");
        log_dec(addr as u32);
        log(" is a hub, enumerating its ports\n");
        enumerate_hub_ports(ep0);
        return;
    }

    if let Some(kbd_ep) = enumerate_as_hid_keyboard(ep0) {
        add_keyboard(ep0, kbd_ep);
        return;
    }
    if let Some((bulk_in, bulk_out)) = enumerate_as_msc(ep0) {
        add_disk(ep0, bulk_in, bulk_out);
    }
}

fn enumerate_hub_ports(ep0: Endpoint) {
    let mut desc = [0u8; 9];
    if get_hub_descriptor(&ep0, &mut desc).is_err() {
        log("usb:   GET_HUB_DESCRIPTOR failed\n");
        return;
    }
    let num_ports = (desc[2] as usize).min(MAX_PORTS) as u8;
    let settle_ms = (desc[5] as u64 * 2).max(20);

    let _ = set_configuration(&ep0, 1); // near-universal: hubs have exactly one configuration, value 1

    log("usb:   hub has ");
    log_dec(num_ports as u32);
    log(" ports\n");

    // Only the root hub is tracked for `poll_hotplug` (see module doc
    // comment); a hub found while already inside another hub's port walk
    // still gets its devices enumerated below, just not re-polled later.
    unsafe {
        if ROOT_HUB.is_none() {
            ROOT_HUB = Some(RootHub { ep0, num_ports });
        }
    }

    for port in 1..=num_ports {
        bring_up_hub_port(&ep0, port, settle_ms);
    }
}

fn bring_up_hub_port(ep0: &Endpoint, port: u8, settle_ms: u64) {
    if set_port_feature(ep0, port, FEATURE_PORT_POWER).is_err() {
        return;
    }
    dwc2::delay_ms(settle_ms);

    let (status, _) = match get_port_status_and_change(ep0, port) {
        Ok(v) => v,
        Err(_) => return,
    };
    if status & 1 == 0 {
        return; // nothing plugged into this port
    }

    log("usb:   port ");
    log_dec(port as u32);
    log(": device connected, resetting...\n");

    if set_port_feature(ep0, port, FEATURE_PORT_RESET).is_err() {
        return;
    }
    dwc2::delay_ms(50); // reset pulse width
    let _ = clear_port_feature(ep0, port, FEATURE_C_PORT_RESET);
    dwc2::delay_ms(10); // reset recovery time

    let (status, _) = match get_port_status_and_change(ep0, port) {
        Ok(v) => v,
        Err(_) => return,
    };
    if status & (1 << 1) == 0 {
        log("usb:   port did not enable after reset\n");
        return;
    }

    bring_up_device(port_speed(status), ep0.dev_addr, port, alloc_addr());
}

/// Walk a device's configuration descriptor looking for a HID
/// boot-protocol keyboard interface, and if found, its interrupt-IN
/// endpoint. `SET_CONFIGURATION` is only issued once that endpoint is
/// actually located, a device that isn't a keyboard is left unconfigured
/// and simply ignored.
fn enumerate_as_hid_keyboard(ep0: Endpoint) -> Option<Endpoint> {
    let mut hdr = [0u8; 9];
    if get_descriptor(&ep0, DESC_CONFIGURATION, 0, &mut hdr).is_err() {
        return None;
    }
    let total_len = u16::from_le_bytes([hdr[2], hdr[3]]) as usize;
    if total_len < 9 || total_len > 256 {
        log("usb:   config descriptor size out of expected range, skipping device\n");
        return None;
    }

    let mut cfg = [0u8; 256];
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

    Some(Endpoint {
        dev_addr: ep0.dev_addr,
        ep_num,
        max_packet: max_pkt,
        speed: ep0.speed,
        hub_addr: ep0.hub_addr,
        hub_port: ep0.hub_port,
    })
}

/// Walk a device's configuration descriptor looking for a Mass Storage /
/// SCSI transparent / Bulk-Only Transport interface, and if found, its
/// bulk IN and bulk OUT endpoints - see `drivers::usb::hub`'s
/// `enumerate_as_msc` (x86_64/UHCI), the identical walk against a
/// differently-shaped `Endpoint` (hub-routing fields included here).
fn enumerate_as_msc(ep0: Endpoint) -> Option<(Endpoint, Endpoint)> {
    let mut hdr = [0u8; 9];
    if get_descriptor(&ep0, DESC_CONFIGURATION, 0, &mut hdr).is_err() {
        return None;
    }
    let total_len = u16::from_le_bytes([hdr[2], hdr[3]]) as usize;
    if total_len < 9 || total_len > 256 {
        return None;
    }

    let mut cfg = [0u8; 256];
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
        Endpoint {
            dev_addr: ep0.dev_addr, ep_num: in_num, max_packet: in_mps,
            speed: ep0.speed, hub_addr: ep0.hub_addr, hub_port: ep0.hub_port,
        },
        Endpoint {
            dev_addr: ep0.dev_addr, ep_num: out_num, max_packet: out_mps,
            speed: ep0.speed, hub_addr: ep0.hub_addr, hub_port: ep0.hub_port,
        },
    ))
}
