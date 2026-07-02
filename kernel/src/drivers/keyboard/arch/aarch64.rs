/// Keyboard driver for aarch64: delegates I/O to the serial driver, same as
/// riscv64 (no PS/2 controller on QEMU virt).

use crate::drivers::keyboard::src::uart_kbd;

pub use uart_kbd::scancode_to_ascii;

pub fn init() {}

pub fn handle_irq() {}

pub fn has_input() -> bool {
    crate::drivers::serial::has_input()
}

pub fn read_scancode() -> Option<u8> {
    crate::drivers::serial::read_byte()
}

pub fn read_scancode_blocking() -> u8 {
    crate::drivers::serial::read_byte_blocking()
}
