//! USB Mass Storage Class driver: SCSI block commands (TEST UNIT READY,
//! READ CAPACITY(10), READ(10), WRITE(10)) wrapped in the Bulk-Only
//! Transport (BOT) envelope (USB Mass Storage Class - Bulk-Only Transport,
//! rev. 1.0). Structurally the same driver as `drivers::usb::msc` (x86_64/
//! UHCI), against `dwc2::bulk_transfer` instead of `uhci::bulk_transfer` -
//! see that module's doc comment for the BOT protocol details, not repeated
//! here.
//!
//! **Compile-tested only, not yet verified on physical hardware or in
//! QEMU**: QEMU's `raspi3b` machine has no DWC2 model at all (see
//! `dwc2.rs`'s module doc comment), so this path has never actually talked
//! to a real USB drive the way the x86_64/UHCI sibling has been. It mirrors
//! that proven implementation as closely as `dwc2::bulk_transfer`'s
//! different (single-DMA-burst, not per-packet-TD) shape allows.
//!
//! Single LUN assumed (`bCBWLUN = 0` always), same as the x86_64 driver.
//! STALL recovery (BOT spec section 6.7) isn't implemented either - a
//! stalled data stage is reported as a plain command failure.

use super::dwc2;
use super::protocol::*;

const CBW_SIGNATURE: u32 = 0x4342_5355;
const CSW_SIGNATURE: u32 = 0x5342_5355;
const CBW_LEN: usize = 31;
const CSW_LEN: usize = 13;
const CBW_FLAG_DATA_IN: u8 = 0x80;
const CSW_STATUS_PASSED: u8 = 0;

const SCSI_TEST_UNIT_READY: u8 = 0x00;
const SCSI_READ_CAPACITY10: u8 = 0x25;
const SCSI_READ10:          u8 = 0x28;
const SCSI_WRITE10:         u8 = 0x2A;

/// See `drivers::usb::msc::SECTOR_SIZE`'s doc comment - same assumption.
pub const SECTOR_SIZE: usize = 512;

/// Matches `hub.rs`'s `MAX_KEYBOARDS` / `hid.rs`'s `MAX_DEVICES` - a fourth
/// view of the same "how many devices can reasonably hang off the LAN9514's
/// ports (or a further external hub) at once" limit.
const MAX_DEVICES: usize = 4;

struct Device {
    #[allow(dead_code)] // kept for symmetry with hid.rs's Device and potential future STALL recovery
    ep0: Endpoint,
    bulk_in: Endpoint,
    bulk_out: Endpoint,
    in_toggle: bool,
    out_toggle: bool,
    tag: u32,
    block_count: u32,
}

static mut DEVICES: [Option<Device>; MAX_DEVICES] = [const { None }; MAX_DEVICES];

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

fn bot_command(dev: &mut Device, cdb: &[u8], data: &mut [u8], data_in: bool) -> dwc2::Result<usize> {
    if cdb.len() > 16 || data.len() > 4096 {
        return Err(());
    }

    dev.tag = dev.tag.wrapping_add(1);
    let tag = dev.tag;

    let mut cbw = [0u8; CBW_LEN];
    cbw[0..4].copy_from_slice(&CBW_SIGNATURE.to_le_bytes());
    cbw[4..8].copy_from_slice(&tag.to_le_bytes());
    cbw[8..12].copy_from_slice(&(data.len() as u32).to_le_bytes());
    cbw[12] = if !data.is_empty() && data_in { CBW_FLAG_DATA_IN } else { 0 };
    cbw[13] = 0; // bCBWLUN: single LUN assumed
    cbw[14] = cdb.len() as u8;
    cbw[15..15 + cdb.len()].copy_from_slice(cdb);
    dwc2::bulk_transfer(&dev.bulk_out, &mut dev.out_toggle, &mut cbw, false)?;

    let mut transferred = 0usize;
    if !data.is_empty() {
        transferred = if data_in {
            dwc2::bulk_transfer(&dev.bulk_in, &mut dev.in_toggle, data, true)?
        } else {
            dwc2::bulk_transfer(&dev.bulk_out, &mut dev.out_toggle, data, false)?
        };
    }

    let mut csw = [0u8; CSW_LEN];
    dwc2::bulk_transfer(&dev.bulk_in, &mut dev.in_toggle, &mut csw, true)?;

    let sig = u32::from_le_bytes([csw[0], csw[1], csw[2], csw[3]]);
    let csw_tag = u32::from_le_bytes([csw[4], csw[5], csw[6], csw[7]]);
    let status = csw[12];
    if sig != CSW_SIGNATURE || csw_tag != tag || status != CSW_STATUS_PASSED {
        return Err(());
    }
    Ok(transferred)
}

fn test_unit_ready(dev: &mut Device) {
    let cdb = [SCSI_TEST_UNIT_READY, 0, 0, 0, 0, 0];
    let _ = bot_command(dev, &cdb, &mut [], false);
}

fn read_capacity10(dev: &mut Device) -> dwc2::Result<(u32, u32)> {
    let cdb = [SCSI_READ_CAPACITY10, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let mut buf = [0u8; 8];
    bot_command(dev, &cdb, &mut buf, true)?;
    let last_lba = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let block_size = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
    Ok((last_lba, block_size))
}

/// Bring a freshly enumerated MSC bulk-only interface online - see
/// `drivers::usb::msc::attach`'s doc comment, identical shape.
pub fn attach(ep0: Endpoint, bulk_in: Endpoint, bulk_out: Endpoint) -> bool {
    let mut dev = Device {
        ep0, bulk_in, bulk_out,
        in_toggle: false, out_toggle: false,
        tag: 0, block_count: 0,
    };

    test_unit_ready(&mut dev);

    let (last_lba, block_size) = match read_capacity10(&mut dev) {
        Ok(v) => v,
        Err(()) => {
            log("usb:   MSC READ CAPACITY(10) failed, ignoring device\n");
            return false;
        }
    };
    if block_size as usize != SECTOR_SIZE {
        log("usb:   MSC device reports an unsupported block size, ignoring\n");
        return false;
    }
    let block_count = last_lba.saturating_add(1);
    dev.block_count = block_count;

    unsafe {
        for slot in DEVICES.iter_mut() {
            if slot.is_none() {
                *slot = Some(dev);
                log("usb: mass storage device ready, ");
                log_dec(block_count);
                log(" blocks\n");
                return true;
            }
        }
    }
    log("usb:   MAX_DEVICES disks already attached, ignoring\n");
    false
}

/// Drop whatever disk is attached at `hub_addr`/`hub_port`, if any - see
/// `hid::detach`'s doc comment, identical shape (location-based, not
/// address-based: only the root hub's ports are re-polled for hot-plug,
/// see `hub.rs`'s module doc comment).
pub fn detach(hub_addr: u8, hub_port: u8) {
    unsafe {
        for slot in DEVICES.iter_mut() {
            if matches!(slot, Some(d) if d.bulk_in.hub_addr == hub_addr && d.bulk_in.hub_port == hub_port) {
                *slot = None;
            }
        }
        // Keep occupied slots contiguous at the front: `device_count()` counts
        // them, but `block_count`/`read_block`/`write_block` index the array
        // positionally, so a hole left by detaching a lower-numbered device
        // would otherwise make a still-attached higher-numbered device
        // uncountable/unreachable by index.
        let mut write = 0;
        for read in 0..MAX_DEVICES {
            if DEVICES[read].is_some() {
                if write != read { DEVICES.swap(write, read); }
                write += 1;
            }
        }
    }
}

pub fn device_count() -> usize {
    unsafe { DEVICES.iter().filter(|d| d.is_some()).count() }
}

pub fn block_count(index: usize) -> Option<u32> {
    unsafe { DEVICES.get(index)?.as_ref().map(|d| d.block_count) }
}

pub fn read_block(index: usize, lba: u32, buf: &mut [u8]) -> dwc2::Result<()> {
    if buf.len() < SECTOR_SIZE {
        return Err(());
    }
    let dev = unsafe { DEVICES.get_mut(index).ok_or(())?.as_mut().ok_or(())? };
    let mut cdb = [0u8; 10];
    cdb[0] = SCSI_READ10;
    cdb[2..6].copy_from_slice(&lba.to_be_bytes());
    cdb[7..9].copy_from_slice(&1u16.to_be_bytes());
    bot_command(dev, &cdb, &mut buf[..SECTOR_SIZE], true)?;
    Ok(())
}

pub fn write_block(index: usize, lba: u32, data: &[u8]) -> dwc2::Result<()> {
    if data.len() < SECTOR_SIZE {
        return Err(());
    }
    let dev = unsafe { DEVICES.get_mut(index).ok_or(())?.as_mut().ok_or(())? };
    let mut cdb = [0u8; 10];
    cdb[0] = SCSI_WRITE10;
    cdb[2..6].copy_from_slice(&lba.to_be_bytes());
    cdb[7..9].copy_from_slice(&1u16.to_be_bytes());
    let mut buf = [0u8; SECTOR_SIZE];
    buf.copy_from_slice(&data[..SECTOR_SIZE]);
    bot_command(dev, &cdb, &mut buf, false)?;
    Ok(())
}
