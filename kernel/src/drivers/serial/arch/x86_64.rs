use crate::drivers::serial::src::{common, com1};

pub fn init()                  { com1::init(); }
// Mirrors onto the VGA text console (`boards::current::console::put_byte`)
// so the interactive userspace shell - which talks to COM1 straight through
// the `write` syscall (`syscall::sys_write`), never through the board's
// early console - is visible in the QEMU display window too, not just over
// serial. Boot-time diagnostics (USB, ...) already reach VGA on their own via
// `boards::current::console::print_byte`; this covers everything after that.
pub fn write_byte(b: u8)       { com1::write_byte(b); crate::boards::current::console::put_byte(b); }
pub fn print_str(s: &str)      { common::print_str(s, write_byte); }
pub fn handle_irq()            { common::handle_irq(com1::rx_ready, com1::read_data); }
pub fn print_bytes(s: &[u8])   { common::print_bytes(s, write_byte); }
pub use common::{has_input, read_byte};

pub fn read_byte_blocking() -> u8 {
    loop {
        if let Some(b) = read_byte() { return b; }
        unsafe { core::arch::asm!("pause", options(nostack)) };
    }
}
