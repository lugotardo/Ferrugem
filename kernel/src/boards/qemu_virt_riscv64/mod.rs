//! BSP: riscv64, QEMU `virt` machine.
//!
//! Boots via OpenSBI (`boot.s` + `linker.ld`), console I/O through SBI legacy
//! putchar (`console.rs` / `sbi.rs`), PLIC-routed UART interrupt (`plic.rs`),
//! and a memory map parsed from the FDT OpenSBI hands off (`fdt.rs`).

pub mod console;
pub mod fdt;
pub mod plic;
pub mod sbi;

/// Board bring-up: nothing to configure, OpenSBI already owns the UART.
/// Called from `arch::riscv64::early_init`.
pub fn init() {
    console::init();
}

/// Route the PLIC-managed UART interrupt. Called from
/// `arch::riscv64::interrupts_init`.
pub fn interrupts_init() {
    plic::init();
}

/// Zero-sized marker used only to anchor the `hal::*` trait impls below.
#[allow(dead_code)]
pub struct Board;

impl crate::hal::Console for Board {
    fn init() { console::init(); }
    fn write_byte(b: u8) { console::print_byte(b); }
}

impl crate::hal::Timer for Board {
    fn init() { crate::drivers::timer::init(); }
    fn rearm() { crate::drivers::timer::rearm(); }
}

impl crate::hal::InterruptController for Board {
    fn init() { plic::init(); }
    // The PLIC driver only ever routes one source (the UART) today; per-id
    // enable/disable isn't exposed yet, so these are no-ops rather than
    // fabricated functionality nothing calls.
    fn enable(_id: u32) {}
    fn disable(_id: u32) {}
    fn eoi(_id: u32) {}
}
