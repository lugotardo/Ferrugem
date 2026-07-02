/// Minimal BCM2837 GPIO driver — just enough to switch GPIO14/15 to ALT0
/// (UART0 TXD0/RXD0) and disable their pull resistors.
///
/// Unlike QEMU virt's PL011, which comes up already wired to a working
/// serial console, real Raspberry Pi 3 hardware leaves GPIO14/15 in their
/// reset state (pulled up, not routed to UART0) until software configures
/// them — the UART registers themselves are correct out of the box, but
/// nothing reaches the pins without this step first.

use super::PERIPHERAL_BASE;

const GPIO_BASE: usize = PERIPHERAL_BASE + 0x0020_0000;

const GPFSEL1:   usize = GPIO_BASE + 0x04;
const GPPUD:     usize = GPIO_BASE + 0x94;
const GPPUDCLK0: usize = GPIO_BASE + 0x98;

fn reg(addr: usize) -> *mut u32 {
    addr as *mut u32
}

fn delay(cycles: u32) {
    for _ in 0..cycles {
        unsafe { core::arch::asm!("nop", options(nostack, nomem)) };
    }
}

/// Route GPIO14/15 to UART0 (ALT0) and disable their pull resistors.
/// Must run before any PL011 register access — see module docs.
pub fn init_uart_pins() {
    unsafe {
        // GPFSEL1 covers GPIO10-19, 3 bits/pin: GPIO14 at bits [14:12],
        // GPIO15 at bits [17:15]. ALT0 function code = 0b100.
        let mut fsel = reg(GPFSEL1).read_volatile();
        fsel &= !((0b111 << 12) | (0b111 << 15));
        fsel |= (0b100 << 12) | (0b100 << 15);
        reg(GPFSEL1).write_volatile(fsel);

        // Classic BCM2835 pull up/down disable sequence (no direct
        // per-pin pull register on this SoC generation): stage the "off"
        // value in GPPUD, latch it into GPIO14/15 via GPPUDCLK0, then
        // clear both. The delays are the datasheet-recommended ~150 cycle
        // settling time; there's no status bit to poll instead.
        reg(GPPUD).write_volatile(0);
        delay(150);
        reg(GPPUDCLK0).write_volatile((1 << 14) | (1 << 15));
        delay(150);
        reg(GPPUD).write_volatile(0);
        reg(GPPUDCLK0).write_volatile(0);
    }
}
