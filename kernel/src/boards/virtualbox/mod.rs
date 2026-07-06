//! BSP: x86_64 PC-compatible platform, VirtualBox (booted from the GRUB ISO
//! built by `make virtualbox` / `make iso-virtualbox`).
//!
//! VirtualBox and QEMU's `q35` machine are both plain IBM-PC-compatible
//! (Multiboot boot, VGA+COM1, 8259 PIC, e820 memory map); `console`/
//! `multiboot`/`pic` below are `boards::pc_common`, shared verbatim with
//! `boards::qemu_pc` since neither has ever needed to diverge on them.
//! `boot.s` is the one piece that already did (VT-x needs 2 MiB paging
//! instead of qemu_pc's single 1 GiB huge page, see that file), which is
//! exactly why it stayed a separate per-board file instead of also moving
//! into `pc_common`.

pub use crate::boards::pc_common::{console, multiboot, pic};

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
