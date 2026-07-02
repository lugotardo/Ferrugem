/// COM1 serial hardware primitives — port 0x3F8, NS16550-compatible (x86_64).
/// Ring buffer and interrupt routing live in arch/x86_64.rs.

use crate::arch::x86_64::{pic, port};

const COM1: u16 = 0x3F8;
const IER:  u16 = COM1 + 1;
const LSR:  u16 = COM1 + 5;

const LSR_RX_READY: u8 = 0x01;
const LSR_TX_EMPTY: u8 = 0x20;

pub fn init() {
    unsafe {
        port::outb(COM1 + 2, 0x01); // Enable FIFO, 1-byte trigger, no RX reset (preserves early input)
        port::outb(IER, 0x01);       // Enable RX interrupt
        pic::unmask(4);              // Unmask IRQ4 (COM1)
    }
}

pub fn write_byte(b: u8) {
    unsafe {
        while port::inb(LSR) & LSR_TX_EMPTY == 0 {}
        port::outb(COM1, b);
    }
}

pub fn rx_ready() -> bool {
    unsafe { port::inb(LSR) & LSR_RX_READY != 0 }
}

pub fn read_data() -> u8 {
    unsafe { port::inb(COM1) }
}
