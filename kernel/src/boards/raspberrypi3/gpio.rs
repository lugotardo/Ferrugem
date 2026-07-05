/// Minimal BCM2837 GPIO driver, plus the PL011 UART0 hardware bring-up real
/// silicon needs, switches GPIO14/15 to ALT0 (UART0 TXD0/RXD0), disables
/// their pull resistors, and then programs the UART itself (baud rate,
/// format, enable).
///
/// Unlike QEMU virt's PL011 model, which comes up already wired to a
/// working 115200 8N1 console, real Raspberry Pi 3 hardware leaves GPIO14/15
/// in their reset state (pulled up, not routed to UART0) *and* leaves the
/// PL011 registers themselves in their power-on-reset state (disabled, no
/// baud rate programmed). Pin mux alone is not enough: nothing reaches the
/// pins without it, but the UART also transmits nothing until software
/// enables it and sets a baud-rate divisor, which QEMU's `raspi3b` model
/// happens to do for us and real hardware does not.

use super::PERIPHERAL_BASE;

const GPIO_BASE: usize = PERIPHERAL_BASE + 0x0020_0000;

const GPFSEL1:   usize = GPIO_BASE + 0x04;
const GPPUD:     usize = GPIO_BASE + 0x94;
const GPPUDCLK0: usize = GPIO_BASE + 0x98;

// PL011 UART0 registers (same block `drivers::serial::src::pl011` and
// `arch::aarch64::console` talk to at runtime, duplicated here per the
// project's existing convention of not sharing register offsets across
// modules, see those modules' doc comments).
const UART_BASE: usize = PERIPHERAL_BASE + 0x0020_1000;
const UARTIBRD:  usize = UART_BASE + 0x24; // integer baud rate divisor
const UARTFBRD:  usize = UART_BASE + 0x28; // fractional baud rate divisor
const UARTLCRH:  usize = UART_BASE + 0x2C; // line control
const UARTCR:    usize = UART_BASE + 0x30; // control
const UARTIMSC:  usize = UART_BASE + 0x38; // interrupt mask set/clear
const UARTICR:   usize = UART_BASE + 0x44; // interrupt clear

const UARTLCRH_FEN:    u32 = 1 << 4;    // enable TX/RX FIFOs
const UARTLCRH_WLEN_8: u32 = 0b11 << 5; // 8 data bits, no parity, 1 stop (8N1)
const UARTCR_UARTEN:   u32 = 1 << 0;
const UARTCR_TXE:      u32 = 1 << 8;
const UARTCR_RXE:      u32 = 1 << 9;

fn reg(addr: usize) -> *mut u32 {
    addr as *mut u32
}

fn delay(cycles: u32) {
    for _ in 0..cycles {
        unsafe { core::arch::asm!("nop", options(nostack, nomem)) };
    }
}

/// Route GPIO14/15 to UART0 (ALT0), disable their pull resistors, and bring
/// PL011 UART0 up at 115200 8N1. Must run before any other UART0 register
/// access, see module docs.
pub fn init_uart_pins() {
    unsafe {
        // Disable the UART before touching pin mux / baud / format, so a
        // half-configured line never gets driven onto the wire.
        reg(UARTCR).write_volatile(0);

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

        // Baud rate divisor for 115200 8N1 off PL011's 48 MHz reference
        // clock: BAUDDIV = 48_000_000 / (16 * 115200) = 26 + 3/64.
        // `config.txt`'s `enable_uart=1` + `core_freq=250` (see
        // boards/raspberrypi3/config.txt) keep that reference fixed
        // regardless of VideoCore frequency scaling.
        reg(UARTICR).write_volatile(0x7FF); // clear any pending interrupt status
        reg(UARTIBRD).write_volatile(26);
        reg(UARTFBRD).write_volatile(3);
        reg(UARTLCRH).write_volatile(UARTLCRH_WLEN_8 | UARTLCRH_FEN);
        reg(UARTIMSC).write_volatile(0); // driver init (pl011.rs) unmasks what it needs

        reg(UARTCR).write_volatile(UARTCR_UARTEN | UARTCR_TXE | UARTCR_RXE);
    }
}
