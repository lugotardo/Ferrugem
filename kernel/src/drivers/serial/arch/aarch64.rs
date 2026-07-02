use crate::drivers::serial::src::{common, pl011};

pub fn init()                  { pl011::init(); }
pub fn write_byte(b: u8)       { pl011::write_byte(b); }
pub fn print_str(s: &str)      { common::print_str(s, pl011::write_byte); }
pub fn handle_irq()            { common::handle_irq(pl011::rx_ready, pl011::read_data); }
pub fn print_bytes(s: &[u8])   { common::print_bytes(s, pl011::write_byte); }
pub use common::{has_input, read_byte};

pub fn read_byte_blocking() -> u8 {
    loop {
        if let Some(b) = read_byte() { return b; }
        unsafe { core::arch::asm!("wfi", options(nostack)) };
    }
}
