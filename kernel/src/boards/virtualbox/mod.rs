//! BSP: x86_64 PC-compatible platform, VirtualBox (booted from the GRUB ISO
//! built by `make virtualbox` / `make iso-virtualbox`).
//!
//! Identical to `boards::qemu_pc` today, VirtualBox and QEMU's `q35`
//! machine are both plain IBM-PC-compatible (Multiboot boot, VGA+COM1,
//! 8259 PIC, e820 memory map), but kept as an independent BSP so the two
//! can diverge later (e.g. ACPI quirks, VirtualBox-specific devices)
//! without having to split a shared module apart first.

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
