pub mod common;
#[cfg(target_arch = "riscv64")]
pub mod ns16550;
#[cfg(target_arch = "x86_64")]
pub mod com1;
