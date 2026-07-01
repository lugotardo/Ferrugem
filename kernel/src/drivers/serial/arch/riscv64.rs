use crate::drivers::serial::src::{common, ns16550};

pub fn init()                  { ns16550::init(); }
pub fn write_byte(b: u8)       { ns16550::write_byte(b); }
pub fn print_str(s: &str)      { common::print_str(s, ns16550::write_byte); }
pub fn handle_irq()            { common::handle_irq(ns16550::rx_ready, ns16550::read_data); }
pub fn print_bytes(s: &[u8])   { common::print_bytes(s, ns16550::write_byte); }
pub use common::{has_input, read_byte};

pub fn read_byte_blocking() -> u8 {
    loop {
        if let Some(b) = read_byte() { return b; }
        unsafe { core::arch::asm!("wfi", options(nostack)) };
    }
}
