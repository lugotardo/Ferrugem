use crate::drivers::timer::src::clint;

pub fn init() { clint::init(); }

/// Re-arming happens directly in `arch::riscv64::trap` via SBI on every
/// timer interrupt; nothing to do here.
pub fn rearm() {}
