//! USB host stack for x86_64 boards: brings up a UHCI controller found on
//! the PCI bus (`uhci.rs`), enumerates its 2 root ports, and wires up every
//! HID boot-protocol keyboard found into `crate::drivers::keyboard` (see
//! `drivers::keyboard::arch::x86_64`) - the x86 counterpart of
//! `boards::raspberrypi3::usb`. Ports are rechecked periodically after boot
//! too, so plugging or unplugging a keyboard later is picked up without a
//! reboot (see `hub::poll_hotplug`).
//!
//! Mass Storage (Bulk-Only Transport, `msc.rs`) devices are enumerated the
//! same way as keyboards, but exposed differently: there's no "new data
//! arrived" push to poll for from a disk, so `disk_read_block`/
//! `disk_write_block` below are synchronous calls driven by whoever needs
//! them (the shell's `diskinfo`/`diskread` commands today), not something
//! `maybe_poll_hotplug` feeds into a shared input buffer.
//!
//! Best-effort like the RPi3 stack: a machine with no UHCI controller at
//! all (e.g. QEMU `q35` started without `-usb`) just means `init` logs why
//! and returns, leaving PS/2 (and the serial console) fully usable either
//! way - see `uhci.rs`'s module doc comment for what's actually implemented.

mod protocol;
mod uhci;
mod hub;
mod hid;
mod msc;

/// See `boards::raspberrypi3::usb::HOTPLUG_POLL_PERIOD` - same reasoning,
/// a handful of PORTSC/control-transfer reads is cheap but not worth doing
/// on every single keystroke poll.
const HOTPLUG_POLL_PERIOD: u32 = 4096;
static mut POLL_COUNT: u32 = 0;

pub fn init() {
    if !uhci::init() {
        return;
    }
    hub::enumerate_root();
}

fn maybe_poll_hotplug() {
    unsafe {
        POLL_COUNT = POLL_COUNT.wrapping_add(1);
        if POLL_COUNT % HOTPLUG_POLL_PERIOD == 0 {
            hub::poll_hotplug();
        }
    }
}

pub fn has_key() -> bool {
    maybe_poll_hotplug();
    hid::has_key()
}

pub fn take_key() -> Option<u8> {
    maybe_poll_hotplug();
    hid::take_key()
}

pub fn take_scroll() -> Option<i8> {
    maybe_poll_hotplug();
    hid::take_scroll()
}

// ── mass storage ──────────────────────────────────────────────────────────

/// Logical block size assumed for every attached disk - see `msc::SECTOR_SIZE`'s doc comment.
pub fn disk_sector_size() -> usize {
    msc::SECTOR_SIZE
}

/// Number of currently attached USB mass-storage devices.
pub fn disk_count() -> usize {
    maybe_poll_hotplug();
    msc::device_count()
}

/// Block count of the `index`-th attached disk (0-based), or `None` if
/// there's no disk at that slot.
pub fn disk_block_count(index: usize) -> Option<u32> {
    msc::block_count(index)
}

/// Read one `disk_sector_size()`-byte block at `lba` from the `index`-th
/// attached disk into `buf`.
pub fn disk_read_block(index: usize, lba: u32, buf: &mut [u8]) -> Result<(), ()> {
    msc::read_block(index, lba, buf)
}

/// Write one `disk_sector_size()`-byte block at `lba` on the `index`-th
/// attached disk from `data`.
pub fn disk_write_block(index: usize, lba: u32, data: &[u8]) -> Result<(), ()> {
    msc::write_block(index, lba, data)
}
