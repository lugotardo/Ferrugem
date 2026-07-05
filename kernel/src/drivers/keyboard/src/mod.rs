pub mod keycode;
pub mod layout;
pub mod state;

#[cfg(target_arch = "x86_64")]
pub mod ps2;
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
pub mod uart_kbd;
