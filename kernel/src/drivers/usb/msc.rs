//! USB Mass Storage Class driver over UHCI: SCSI block commands (TEST UNIT
//! READY, READ CAPACITY(10), READ(10), WRITE(10)) wrapped in the Bulk-Only
//! Transport (BOT) envelope (USB Mass Storage Class - Bulk-Only Transport,
//! rev. 1.0). Structurally similar to `hid.rs`: a small fixed table of
//! attached devices - but driven synchronously by whoever calls
//! `read_block`/`write_block` (a shell command today), not polled every
//! tick like keyboard input, since a disk has no "new data arrived"
//! push notification to wait for.
//!
//! Root-hub-only (see `uhci.rs`'s module doc comment), single LUN assumed
//! (`bCBWLUN = 0` always) - multi-LUN devices such as multi-slot card
//! readers aren't handled. STALL recovery (BOT spec section 6.7: clear the
//! offending endpoint's halt via `CLEAR_FEATURE`, then still read the CSW)
//! also isn't implemented - a stalled data stage is treated as a hard
//! failure and the command is simply reported as failed to the caller.

use super::protocol::*;
use super::uhci;

// ── Bulk-Only Transport wire format (USB MSC BOT spec section 5) ──────────

const CBW_SIGNATURE: u32 = 0x4342_5355; // 'U' 'S' 'B' 'C' on the wire (little-endian)
const CSW_SIGNATURE: u32 = 0x5342_5355; // 'U' 'S' 'B' 'S' on the wire (little-endian)
const CBW_LEN: usize = 31;
const CSW_LEN: usize = 13;
const CBW_FLAG_DATA_IN: u8 = 0x80;
const CSW_STATUS_PASSED: u8 = 0;

/// Generous timeout for a BOT command/data/status round trip: a real disk
/// can take a while to spin up or seek, and QEMU's `usb-storage` device is
/// itself backed by a regular file/image read. Much larger than
/// `uhci::CONTROL_TIMEOUT_US` (50 ms) on purpose.
const BOT_TIMEOUT_US: u32 = 500_000;

// ── SCSI command opcodes (SPC-3 / SBC-2) - only what this driver issues ───

const SCSI_TEST_UNIT_READY: u8 = 0x00;
const SCSI_READ_CAPACITY10: u8 = 0x25;
const SCSI_READ10:          u8 = 0x28;
const SCSI_WRITE10:         u8 = 0x2A;

/// Fixed logical block size this driver understands. The vast majority of
/// USB flash drives - and QEMU's `usb-storage` device - report exactly this
/// via READ CAPACITY(10); devices with a different block size (some 4Kn
/// drives) are rejected at `attach()` rather than silently mishandled.
pub const SECTOR_SIZE: usize = 512;

struct Device {
    #[allow(dead_code)] // kept for symmetry with hid.rs's Device and potential future STALL recovery
    ep0: Endpoint,
    bulk_in: Endpoint,
    bulk_out: Endpoint,
    /// Per-endpoint data toggle, owned by this driver for the endpoint's
    /// whole lifetime - see `uhci::bulk_transfer`'s doc comment.
    in_toggle: bool,
    out_toggle: bool,
    /// dCBWTag of the next command; just needs to keep changing so a stray
    /// CSW from a previous, already-abandoned command can't be mistaken for
    /// the current one. Wrapping is fine, this is not a security boundary.
    tag: u32,
    block_count: u32,
}

const MAX_DEVICES: usize = 2;
static mut DEVICES: [Option<Device>; MAX_DEVICES] = [const { None }; MAX_DEVICES];

fn log(s: &str) {
    crate::arch::x86_64::console::print_str(s);
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

/// One full BOT transaction: send a CBW built from `cdb`, transfer `data`
/// (if non-empty) on the bulk IN or OUT endpoint according to `data_in`,
/// then read and validate the CSW. Bumps `dev.tag` first, even on eventual
/// failure, so a retried command never reuses a tag a straggling response
/// from the previous attempt could still match.
///
/// A failure in the data stage skips straight to returning `Err(())`
/// without attempting to read the CSW - see this module's doc comment on
/// why STALL recovery isn't implemented. A well-behaved device won't have
/// anything queued to send afterward anyway since the command never
/// completed on its end either.
fn bot_command(dev: &mut Device, cdb: &[u8], data: &mut [u8], data_in: bool) -> uhci::Result<usize> {
    if cdb.len() > 16 || data.len() > uhci::MAX_BULK_XFER {
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
    uhci::bulk_transfer(&dev.bulk_out, &mut dev.out_toggle, &mut cbw, false, BOT_TIMEOUT_US)?;

    let mut transferred = 0usize;
    if !data.is_empty() {
        transferred = if data_in {
            uhci::bulk_transfer(&dev.bulk_in, &mut dev.in_toggle, data, true, BOT_TIMEOUT_US)?
        } else {
            uhci::bulk_transfer(&dev.bulk_out, &mut dev.out_toggle, data, false, BOT_TIMEOUT_US)?
        };
    }

    let mut csw = [0u8; CSW_LEN];
    uhci::bulk_transfer(&dev.bulk_in, &mut dev.in_toggle, &mut csw, true, BOT_TIMEOUT_US)?;

    let sig = u32::from_le_bytes([csw[0], csw[1], csw[2], csw[3]]);
    let csw_tag = u32::from_le_bytes([csw[4], csw[5], csw[6], csw[7]]);
    let status = csw[12];
    if sig != CSW_SIGNATURE || csw_tag != tag || status != CSW_STATUS_PASSED {
        return Err(());
    }
    Ok(transferred)
}

/// All-zero CDB except opcode: some USB flash drives NAK or stall the very
/// first real command right after enumeration until they've seen one
/// TEST UNIT READY settle them. Errors here are ignored - this is a
/// best-effort nudge, not something `attach()` acts on either way.
fn test_unit_ready(dev: &mut Device) {
    let cdb = [SCSI_TEST_UNIT_READY, 0, 0, 0, 0, 0];
    let _ = bot_command(dev, &cdb, &mut [], false);
}

/// READ CAPACITY(10): returns `(last_lba, block_size_bytes)`.
fn read_capacity10(dev: &mut Device) -> uhci::Result<(u32, u32)> {
    let cdb = [SCSI_READ_CAPACITY10, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let mut buf = [0u8; 8];
    bot_command(dev, &cdb, &mut buf, true)?;
    let last_lba = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let block_size = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
    Ok((last_lba, block_size))
}

/// Bring a freshly enumerated MSC bulk-only interface online: settle it
/// with TEST UNIT READY, read its capacity, and - if the block size is one
/// this driver understands - register it as an attached disk. Called from
/// `hub.rs` after enumeration (initial or hot-plug) finds a Mass Storage /
/// SCSI / Bulk-Only interface.
///
/// Returns `false` on any failure; like a HID device that doesn't
/// configure, this isn't fatal to the rest of USB enumeration, the caller
/// just leaves the device otherwise unconfigured.
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

/// Drop whatever disk is attached at `dev_addr`, if any - called by
/// `hub.rs` when its hot-plug rescan sees that port's connection go away.
pub fn detach(dev_addr: u8) {
    unsafe {
        for slot in DEVICES.iter_mut() {
            if matches!(slot, Some(d) if d.bulk_in.dev_addr == dev_addr) {
                *slot = None;
            }
        }
    }
}

/// Number of currently attached mass-storage devices.
pub fn device_count() -> usize {
    unsafe { DEVICES.iter().filter(|d| d.is_some()).count() }
}

/// Block count of the `index`-th attached device (0-based), or `None` if
/// there's no device at that slot.
pub fn block_count(index: usize) -> Option<u32> {
    unsafe { DEVICES.get(index)?.as_ref().map(|d| d.block_count) }
}

/// Read one `SECTOR_SIZE`-byte block at `lba` from the `index`-th attached
/// device into `buf[..SECTOR_SIZE]`.
pub fn read_block(index: usize, lba: u32, buf: &mut [u8]) -> uhci::Result<()> {
    if buf.len() < SECTOR_SIZE {
        return Err(());
    }
    let dev = unsafe { DEVICES.get_mut(index).ok_or(())?.as_mut().ok_or(())? };
    let mut cdb = [0u8; 10];
    cdb[0] = SCSI_READ10;
    cdb[2..6].copy_from_slice(&lba.to_be_bytes());
    cdb[7..9].copy_from_slice(&1u16.to_be_bytes()); // transfer length: 1 block
    bot_command(dev, &cdb, &mut buf[..SECTOR_SIZE], true)?;
    Ok(())
}

/// Write one `SECTOR_SIZE`-byte block at `lba` on the `index`-th attached
/// device from `data[..SECTOR_SIZE]`.
pub fn write_block(index: usize, lba: u32, data: &[u8]) -> uhci::Result<()> {
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
