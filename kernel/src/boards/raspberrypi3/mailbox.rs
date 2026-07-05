//! VideoCore mailbox property-channel interface (BCM2837).
//!
//! The ARM core never talks to the display hardware directly, the VC4 GPU
//! owns HDMI/composite timing and output entirely. This 32-bit mailbox
//! (property channel 8) is the only way to ask firmware for a framebuffer;
//! `framebuffer.rs` is the one caller today.
//!
//! Real hardware runs with the MMU and D-cache already on by the time this
//! can run (`arch::board_late_init` fires after `memory::init`'s paging
//! setup), unlike most bare-metal tutorials, which do this with caches off
//! and can hand the VC a raw pointer. With caching enabled, a CPU write to
//! the request buffer only reaches RAM (where the VC reads it directly, not
//! through the ARM cache) after an explicit clean; the reverse (flushing
//! stale cached copies of the VC's response) needs an explicit invalidate.
//! Both are done by hand around every call below.
//!
//! QEMU's `raspi3b` machine does not model this mailbox at all, so every
//! poll loop here is bounded, a `call()` on QEMU degrades to a bounded
//! spin-then-fail rather than hanging the day-to-day test target forever.

use super::PERIPHERAL_BASE;

const MAILBOX_BASE: usize = PERIPHERAL_BASE + 0xB880;
const MAILBOX_READ:   usize = MAILBOX_BASE;
const MAILBOX_STATUS: usize = MAILBOX_BASE + 0x18;
const MAILBOX_WRITE:  usize = MAILBOX_BASE + 0x20;

const MAILBOX_FULL:  u32 = 1 << 31;
const MAILBOX_EMPTY: u32 = 1 << 30;

const CHANNEL_PROPERTY: u32 = 8;

/// Response code written to word [1] of a property message on success.
pub const RESP_SUCCESS: u32 = 0x8000_0000;

/// Bounds every register poll below so a mailbox that never responds (e.g.
/// QEMU's `raspi3b`, which doesn't implement this peripheral) fails fast
/// instead of hanging boot.
const MAX_SPIN: u32 = 2_000_000;

fn reg(addr: usize) -> *mut u32 {
    addr as *mut u32
}

/// aarch64 D-cache line size in bytes, from `CTR_EL0.DminLine` (log2 of the
/// line size in 4-byte words), Cortex-A53 is always 64 B, but this reads
/// the real value instead of assuming it.
fn dcache_line_size() -> usize {
    let ctr: u64;
    unsafe { core::arch::asm!("mrs {v}, ctr_el0", v = out(reg) ctr, options(nostack)) };
    4usize << ((ctr >> 16) & 0xF)
}

fn clean_range(start: usize, len: usize) {
    let line = dcache_line_size();
    let end = start + len;
    let mut addr = start & !(line - 1);
    unsafe {
        while addr < end {
            core::arch::asm!("dc cvac, {a}", a = in(reg) addr, options(nostack));
            addr += line;
        }
        core::arch::asm!("dsb sy", options(nostack));
    }
}

fn invalidate_range(start: usize, len: usize) {
    let line = dcache_line_size();
    let end = start + len;
    let mut addr = start & !(line - 1);
    unsafe {
        core::arch::asm!("dsb sy", options(nostack));
        while addr < end {
            core::arch::asm!("dc ivac, {a}", a = in(reg) addr, options(nostack));
            addr += line;
        }
        core::arch::asm!("dsb sy", options(nostack));
    }
}

/// ARM physical address -> VC "bus address", direct/uncached alias (top two
/// bits set) so the VC reads/writes RAM directly instead of through its own
/// L2 cache, orthogonal to (and in addition to) the ARM-side `dc cvac`
/// above, which is a completely separate cache on the other side of the bus.
pub fn phys_to_bus(phys: usize) -> u32 {
    (phys as u32) | 0xC000_0000
}

/// VC bus address -> ARM physical address (strip the alias bits).
pub fn bus_to_phys(bus: u32) -> usize {
    (bus & 0x3FFF_FFFF) as usize
}

/// Send a property-tag message to the VC over channel 8 and wait for its
/// response in place. `buffer[0]` must already hold the message size in
/// bytes and `buffer[1] = 0` (request code); the tags follow per the
/// Broadcom mailbox property interface. The buffer's address must be
/// 16-byte aligned (`framebuffer.rs` enforces this via `repr(align(16))`).
///
/// Returns `false` on a firmware-reported failure or (real hardware should
/// never hit this, see module docs) a timed-out poll.
pub fn call(buffer: &mut [u32]) -> bool {
    let addr = buffer.as_mut_ptr() as usize;
    let len = buffer.len() * 4;
    clean_range(addr, len);

    let msg = phys_to_bus(addr) | CHANNEL_PROPERTY;
    unsafe {
        let mut spins = 0u32;
        while reg(MAILBOX_STATUS).read_volatile() & MAILBOX_FULL != 0 {
            spins += 1;
            if spins > MAX_SPIN {
                return false;
            }
        }
        reg(MAILBOX_WRITE).write_volatile(msg);

        // One spin budget for the whole response wait, not reset per
        // sub-loop iteration, otherwise a peripheral that's always
        // "not empty" but never returns a matching response (as an
        // unimplemented/stub mailbox could) would spin forever, since
        // neither inner condition alone would ever exceed MAX_SPIN.
        spins = 0;
        loop {
            while reg(MAILBOX_STATUS).read_volatile() & MAILBOX_EMPTY != 0 {
                spins += 1;
                if spins > MAX_SPIN {
                    return false;
                }
            }
            let resp = reg(MAILBOX_READ).read_volatile();
            if resp == msg {
                break;
            }
            spins += 1;
            if spins > MAX_SPIN {
                return false;
            }
        }
    }

    invalidate_range(addr, len);
    buffer[1] == RESP_SUCCESS
}
