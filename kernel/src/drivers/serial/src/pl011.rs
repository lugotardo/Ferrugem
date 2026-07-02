/// PL011 UART hardware primitives — QEMU virt board at 0x0900_0000 (aarch64).
/// Ring buffer and interrupt routing live in arch/aarch64.rs.

const UART_BASE: usize = 0x0900_0000;

const UARTDR:   usize = 0x00; // data register (read = RX, write = TX)
const UARTFR:   usize = 0x18; // flag register
const UARTIMSC: usize = 0x38; // interrupt mask set/clear
const UARTICR:  usize = 0x44; // interrupt clear register

const UARTFR_RXFE: u32 = 1 << 4; // receive FIFO empty
const UARTFR_TXFF: u32 = 1 << 5; // transmit FIFO full

const RXIM: u32 = 1 << 4; // receive interrupt mask bit

fn reg(offset: usize) -> *mut u32 {
    (UART_BASE + offset) as *mut u32
}

pub fn init() {
    unsafe {
        reg(UARTICR).write_volatile(0x7FF);  // clear all pending interrupt status
        reg(UARTIMSC).write_volatile(RXIM);  // enable RX interrupt only
    }
}

pub fn write_byte(b: u8) {
    unsafe {
        while reg(UARTFR).read_volatile() & UARTFR_TXFF != 0 {}
        reg(UARTDR).write_volatile(b as u32);
    }
}

pub fn rx_ready() -> bool {
    unsafe { reg(UARTFR).read_volatile() & UARTFR_RXFE == 0 }
}

pub fn read_data() -> u8 {
    unsafe { reg(UARTDR).read_volatile() as u8 }
}
