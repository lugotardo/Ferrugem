/// Keyboard driver for RISC-V: delegates I/O to the serial driver.

use crate::drivers::keyboard::src::uart_kbd;

pub fn init() {}

pub fn handle_irq() {}

pub fn has_input() -> bool {
    uart_kbd::has_input()
}

pub fn read_byte() -> Option<u8> {
    uart_kbd::read_byte()
}

pub fn read_byte_blocking() -> u8 {
    uart_kbd::read_byte_blocking()
}
