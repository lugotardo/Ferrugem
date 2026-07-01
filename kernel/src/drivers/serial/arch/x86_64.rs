use crate::drivers::serial::src::{common, com1};

pub fn init()                  { com1::init(); }
pub fn write_byte(b: u8)       { com1::write_byte(b); }
pub fn print_str(s: &str)      { common::print_str(s, com1::write_byte); }
pub fn handle_irq()            { common::handle_irq(com1::rx_ready, com1::read_data); }
pub fn print_bytes(s: &[u8])   { common::print_bytes(s, com1::write_byte); }
pub use common::{has_input, read_byte};

pub fn read_byte_blocking() -> u8 {
    loop {
        if let Some(b) = read_byte() { return b; }
        unsafe { core::arch::asm!("pause", options(nostack)) };
    }
}
