/// Keyboard driver for RISC-V: delegates I/O to the serial driver.

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
