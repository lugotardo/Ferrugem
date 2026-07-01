/// Ring buffer + interrupt logic shared between all serial hardware backends.

use crate::drivers::ring_buf::RingBuf;

pub static mut INPUT: RingBuf<64> = RingBuf::new();

pub fn print_str(s: &str, write_byte: fn(u8)) {
    for b in s.bytes() {
        if b == b'\n' { write_byte(b'\r'); }
        write_byte(b);
    }
}

pub fn print_bytes(s: &[u8], write_byte: fn(u8)) {
    for &b in s {
        if b == b'\n' { write_byte(b'\r'); }
        write_byte(b);
    }
}

/// Drains the hardware FIFO into INPUT and wakes the TTY waiter if new bytes arrived.
pub fn handle_irq(rx_ready: fn() -> bool, read_data: fn() -> u8) {
    unsafe {
        let mut woke = false;
        while rx_ready() {
            INPUT.push(read_data());
            woke = true;
        }
        if woke { crate::scheduler::wake_tty_waiter(); }
    }
}

pub fn has_input() -> bool {
    unsafe { !INPUT.is_empty() }
}

pub fn read_byte() -> Option<u8> {
    unsafe { INPUT.pop() }
}
