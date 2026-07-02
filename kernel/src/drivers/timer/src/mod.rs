#[cfg(target_arch = "x86_64")]
pub mod pit;
#[cfg(target_arch = "riscv64")]
pub mod clint;
#[cfg(target_arch = "aarch64")]
pub mod generic_timer;
