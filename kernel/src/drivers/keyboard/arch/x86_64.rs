/// Keyboard driver for x86_64: ring buffer + scheduler wakeup on top of PS/2 hardware.

use crate::drivers::ring_buf::RingBuf;
use crate::drivers::keyboard::src::ps2;

pub use ps2::scancode_to_ascii;

static mut INPUT: RingBuf<64> = RingBuf::new();

pub fn init() { ps2::init(); }

/// Called from IRQ1 handler.
pub fn handle_irq() {
    unsafe {
        if let Some(sc) = ps2::try_read() {
            INPUT.push(sc);
            crate::scheduler::wake_tty_waiter();
        }
    }
}

pub fn has_input() -> bool {
    unsafe { !INPUT.is_empty() }
}

pub fn read_scancode() -> Option<u8> {
    unsafe { INPUT.pop() }
}

pub fn read_scancode_blocking() -> u8 {
    loop {
        if let Some(sc) = read_scancode() { return sc; }
        unsafe { core::arch::asm!("pause", options(nostack)) };
    }
}
