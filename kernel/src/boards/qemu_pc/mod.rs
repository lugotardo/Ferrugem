//! BSP: x86_64 PC-compatible platform, QEMU `q35` machine.
//!
//! Boots via Multiboot1/2 (`boot.s` + `linker.ld`), VGA text console mirrored
//! to COM1 (`console.rs`), 8259 PIC interrupt routing (`pic.rs`), and an e820
//! memory map parsed from the Multiboot info structure (`multiboot.rs`).

pub mod console;
pub mod multiboot;
pub mod pic;

/// Board bring-up: PIC remap/mask + VGA+COM1 console. Called from
/// `arch::x86_64::early_init` before interrupts are enabled.
pub fn init() {
    pic::init();
    console::init();
}

/// Unmask the IRQ lines this board expects a driver to be listening on
/// (timer + PS/2 keyboard). Called from `arch::x86_64::interrupts_init`.
pub fn unmask_default_irqs() {
    pic::unmask(0);
    pic::unmask(1);
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
    fn init() { pic::init(); }
    fn enable(id: u32) { pic::unmask(id as u8); }
    fn disable(id: u32) { pic::mask(id as u8); }
    fn eoi(id: u32) { pic::eoi(id as u8); }
}
