/// 8253/8254 PIT channel 0 at ~100 Hz (IRQ0).

use crate::arch::x86_64::port;

const PIT_CHANNEL0: u16 = 0x40;
const PIT_CMD:      u16 = 0x43;

pub fn init() {
    // 100 Hz → divisor = 1_193_182 / 100 = 11931
    let divisor: u16 = 11931;
    unsafe {
        port::outb(PIT_CMD, 0x36); // channel 0, lobyte/hibyte, square wave
        port::outb(PIT_CHANNEL0, (divisor & 0xFF) as u8);
        port::outb(PIT_CHANNEL0, (divisor >> 8) as u8);
    }
}
