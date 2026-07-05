/// 8253/8254 PIT channel 0 at ~100 Hz (IRQ0), plus a channel 2 busy-wait
/// delay (`delay_us`) for drivers needing a real, calibrated hold time
/// (e.g. `drivers::usb::uhci`'s mandatory reset pulse widths) - a separate
/// channel from channel 0's ongoing scheduler tick so using one doesn't
/// disturb the other.

use crate::arch::x86_64::port;

const PIT_CHANNEL0: u16 = 0x40;
const PIT_CHANNEL2: u16 = 0x42;
const PIT_CMD:      u16 = 0x43;
const PIT_GATE:     u16 = 0x61; // "NMI status and control" - bit 0 gates channel 2, bit 5 reads its output

const PIT_HZ: u64 = 1_193_182;

pub fn init() {
    // 100 Hz → divisor = 1_193_182 / 100 = 11931
    let divisor: u16 = 11931;
    unsafe {
        port::outb(PIT_CMD, 0x36); // channel 0, lobyte/hibyte, square wave
        port::outb(PIT_CHANNEL0, (divisor & 0xFF) as u8);
        port::outb(PIT_CHANNEL0, (divisor >> 8) as u8);
    }
}

/// Busy-wait for at least `us` microseconds using PIT channel 2 in one-shot
/// mode (classic technique, see the OSDev wiki's PIT article): load a
/// terminal count, gate the channel on via port 0x61 bit 0 (keeping the PC
/// speaker, bit 1, off), then poll bit 5 of that same port - it mirrors the
/// channel's raw OUT pin, which goes high once the count reaches zero.
/// Capped at one PIT reload (~54.9 ms at 1.193182 MHz / 0xFFFF) per call;
/// callers needing longer just call this in a loop.
pub fn delay_us(us: u32) {
    let mut remaining_us = us;
    while remaining_us > 0 {
        let chunk_us = remaining_us.min(54_000);
        remaining_us -= chunk_us;

        let count = ((PIT_HZ * chunk_us as u64) / 1_000_000).clamp(1, 0xFFFF) as u16;
        unsafe {
            // Gate on / speaker off *before* loading the count (the order
            // the 8254 recipe is universally documented in, e.g. the OSDev
            // wiki and every BIOS-era reference implementation) so counting
            // starts as soon as the high byte lands, not before.
            let gate = port::inb(PIT_GATE);
            port::outb(PIT_GATE, (gate & !0x02) | 0x01);

            port::outb(PIT_CMD, 0b1011_0000); // channel 2, lobyte/hibyte, mode 0 (terminal count), binary
            port::outb(PIT_CHANNEL2, (count & 0xFF) as u8);
            port::outb(PIT_CHANNEL2, (count >> 8) as u8);

            while port::inb(PIT_GATE) & 0x20 == 0 {}

            port::outb(PIT_GATE, gate); // restore original gate/speaker state
        }
    }
}

pub fn delay_ms(ms: u32) {
    delay_us(ms.saturating_mul(1000));
}
