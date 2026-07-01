/// NS16550A UART hardware primitives — QEMU virt board at 0x1000_0000 (RISC-V).
/// Ring buffer and interrupt routing live in arch/riscv64.rs.

const UART_BASE: usize = 0x1000_0000;

const THR: usize = 0; // Transmit holding (write)
const RBR: usize = 0; // Receive buffer (read)
const IER: usize = 1; // Interrupt enable
const FCR: usize = 2; // FIFO control
const LCR: usize = 3; // Line control
const MCR: usize = 4; // Modem control
const LSR: usize = 5; // Line status

const LSR_RX_READY: u8 = 0x01;
const LSR_TX_EMPTY: u8 = 0x20;

fn reg(offset: usize) -> *mut u8 {
    (UART_BASE + offset) as *mut u8
}

pub fn init() {
    unsafe {
        reg(IER).write_volatile(0x00); // Disable all interrupts
        reg(LCR).write_volatile(0x80); // DLAB on
        reg(0).write_volatile(0x01);   // Divisor low: 115200 baud
        reg(1).write_volatile(0x00);   // Divisor high
        reg(LCR).write_volatile(0x03); // 8N1, DLAB off
        reg(FCR).write_volatile(0xC7); // Enable + reset FIFOs
        reg(MCR).write_volatile(0x0B); // RTS + DTR
        reg(IER).write_volatile(0x01); // Enable RX interrupt
    }
}

pub fn write_byte(b: u8) {
    unsafe {
        while reg(LSR).read_volatile() & LSR_TX_EMPTY == 0 {}
        reg(THR).write_volatile(b);
    }
}

pub fn rx_ready() -> bool {
    unsafe { reg(LSR).read_volatile() & LSR_RX_READY != 0 }
}

pub fn read_data() -> u8 {
    unsafe { reg(RBR).read_volatile() }
}
