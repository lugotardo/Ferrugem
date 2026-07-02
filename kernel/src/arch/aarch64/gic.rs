/// GICv2 driver (QEMU `virt` machine, `gic-version=2`).
/// Distributor at 0x0800_0000, CPU interface at 0x0801_0000.
/// Plays the same role here as `riscv64::plic` — routes both the ARM
/// generic timer PPI and the PL011 SPI, since aarch64 has no separate
/// CSR-based timer-interrupt path like RISC-V's `sie.STIE`.

const GICD_BASE: usize = 0x0800_0000;
const GICC_BASE: usize = 0x0801_0000;

const GICD_CTLR:      usize = 0x000;
const GICD_ISENABLER: usize = 0x100;
const GICD_ICENABLER: usize = 0x180;
const GICD_ITARGETSR: usize = 0x800;

const GICC_CTLR: usize = 0x000;
const GICC_PMR:  usize = 0x004;
const GICC_IAR:  usize = 0x00C;
const GICC_EOIR: usize = 0x010;

/// Non-secure physical timer PPI (INTID 16 + 14).
pub const TIMER_IRQ: u32 = 30;
/// PL011 UART0 SPI (INTID 32 + 1) on QEMU virt.
pub const UART_IRQ: u32 = 33;

fn gicd32(off: usize) -> *mut u32 { (GICD_BASE + off) as *mut u32 }
fn gicc32(off: usize) -> *mut u32 { (GICC_BASE + off) as *mut u32 }

pub fn init() {
    unsafe {
        gicd32(GICD_CTLR).write_volatile(1); // enable distributor
        gicc32(GICC_PMR).write_volatile(0xFF); // don't mask on priority
        gicc32(GICC_CTLR).write_volatile(1); // enable cpu interface
        enable(TIMER_IRQ);
        enable(UART_IRQ);
    }
}

pub fn enable(id: u32) {
    unsafe {
        // SPIs (id >= 32) need an explicit CPU target; PPIs/SGIs are banked
        // per-CPU and ignore GICD_ITARGETSR.
        if id >= 32 {
            let target = (GICD_BASE + GICD_ITARGETSR + id as usize) as *mut u8;
            target.write_volatile(1); // route to CPU0
        }
        let reg = gicd32(GICD_ISENABLER + (id as usize / 32) * 4);
        reg.write_volatile(1 << (id % 32));
    }
}

pub fn disable(id: u32) {
    unsafe {
        let reg = gicd32(GICD_ICENABLER + (id as usize / 32) * 4);
        reg.write_volatile(1 << (id % 32));
    }
}

pub fn eoi(id: u32) {
    unsafe { gicc32(GICC_EOIR).write_volatile(id); }
}

/// Acknowledge and dispatch the pending IRQ, then signal end-of-interrupt.
pub fn handle() {
    unsafe {
        let iar = gicc32(GICC_IAR).read_volatile();
        let id = iar & 0x3FF; // INTID field; 1023 = spurious
        match id {
            TIMER_IRQ => {
                crate::scheduler::tick();
                crate::drivers::timer::rearm();
            }
            UART_IRQ => crate::drivers::serial::handle_irq(),
            _ => {}
        }
        if id != 1023 {
            eoi(id);
        }
    }
}
