pub mod arch;
pub(crate) mod src;

pub fn init() {
    arch::init();
}

/// Re-arm the next tick. No-op on architectures whose timer free-runs or
/// re-arms itself directly in the trap handler (x86_64, riscv64).
pub fn rearm() {
    arch::rearm();
}
