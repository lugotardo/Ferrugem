/// ARM Generic Timer (physical, non-secure EL1), fires on GIC PPI 30.
/// QEMU virt exposes CNTFRQ_EL0; derive a ~100 Hz tick from it so timing
/// stays comparable across architectures regardless of the emulated
/// counter frequency (unlike RISC-V's fixed-cycle-count SBI timer).

use core::sync::atomic::{AtomicU64, Ordering};

static INTERVAL: AtomicU64 = AtomicU64::new(0);

fn read_freq() -> u64 {
    let freq: u64;
    unsafe { core::arch::asm!("mrs {}, cntfrq_el0", out(reg) freq, options(nostack)) };
    freq
}

pub fn init() {
    let interval = read_freq() / 100; // ~100 Hz
    INTERVAL.store(interval, Ordering::Relaxed);
    unsafe {
        core::arch::asm!("msr cntp_tval_el0, {v}", v = in(reg) interval, options(nostack));
        core::arch::asm!("msr cntp_ctl_el0, {v}", v = in(reg) 1u64, options(nostack)); // ENABLE=1, IMASK=0
    }
}

/// Re-arm the next tick. Called from the timer IRQ path (`gic::handle`).
pub fn rearm() {
    let interval = INTERVAL.load(Ordering::Relaxed);
    unsafe { core::arch::asm!("msr cntp_tval_el0, {v}", v = in(reg) interval, options(nostack)) };
}
