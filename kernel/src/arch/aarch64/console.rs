/// Minimal PL011 UART console for early boot/panic output — reachable even
/// before `drivers::serial::init()` runs, mirroring x86_64's direct COM1
/// access in `arch::x86_64::console`. The interrupt-driven runtime driver
/// lives in `drivers::serial::src::pl011` and duplicates these register
/// offsets; that split matches the existing x86_64 convention.

#[cfg(feature = "board-raspberrypi3")]
const UART_BASE: usize = crate::boards::raspberrypi3::PERIPHERAL_BASE + 0x0020_1000;
#[cfg(not(feature = "board-raspberrypi3"))]
const UART_BASE: usize = 0x0900_0000;

const UARTDR: usize = 0x00; // data register
const UARTFR: usize = 0x18; // flag register
const UARTFR_TXFF: u32 = 1 << 5; // transmit FIFO full

fn reg(offset: usize) -> *mut u32 {
    (UART_BASE + offset) as *mut u32
}

pub fn init() {
    // QEMU's PL011 model comes up already configured (115200 8N1) when
    // booted directly via -kernel; nothing to do here. Real Raspberry Pi 3
    // hardware (and QEMU's raspi3b model) additionally needs GPIO14/15
    // routed to UART0 before these registers do anything — see
    // `boards::raspberrypi3::gpio`.
    #[cfg(feature = "board-raspberrypi3")]
    crate::boards::raspberrypi3::gpio::init_uart_pins();
}

pub fn print_str(s: &str) {
    for b in s.bytes() {
        print_byte(b);
    }
}

pub fn print_byte(b: u8) {
    unsafe {
        while reg(UARTFR).read_volatile() & UARTFR_TXFF != 0 {}
        reg(UARTDR).write_volatile(b as u32);
    }
}
