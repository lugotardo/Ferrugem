/// Interrupt routing for the Raspberry Pi 3 (BCM2837), which has no GIC.
///
/// Two separate blocks have to be combined to reach the same two IRQ
/// sources `qemu_virt_aarch64::gic` routes through one GICv2:
///
/// - The "ARM local" / QA7 block (fixed physical base `0x4000_0000` on
///   every BCM283x, independent of the peripheral base) owns per-core
///   routing of the ARM generic timer's interrupt lines, and also reports
///   whether the legacy controller below has a pending peripheral IRQ.
/// - The legacy BCM2835-style interrupt controller (peripheral offset
///   `0xB200`) owns every peripheral IRQ, including PL011 UART0 (physical
///   IRQ 57).
use super::PERIPHERAL_BASE;

const LOCAL_BASE: usize = 0x4000_0000;
const CORE0_TIMER_IRQCNTL: usize = LOCAL_BASE + 0x40;
const CORE0_IRQ_SOURCE:    usize = LOCAL_BASE + 0x60;
const CORE0_SRC_CNTPNSIRQ: u32 = 1 << 1;
const CORE0_SRC_GPU:       u32 = 1 << 8; // legacy controller has a pending IRQ

const LEGACY_BASE: usize = PERIPHERAL_BASE + 0xB200;
const IRQ_PENDING_2: usize = LEGACY_BASE + 0x08;
const ENABLE_IRQS_2: usize = LEGACY_BASE + 0x14;

/// UART0 is peripheral IRQ 57 (bit 25 of the IRQS_2 range, which covers
/// peripheral IRQs 32-63).
pub const UART_IRQ: u32 = 57;
/// Synthetic id for the ARM generic timer's non-secure physical comparator
/// (`CNTPNSIRQ`), the QA7 block has no peripheral-IRQ-style numbering.
pub const TIMER_IRQ: u32 = 1;

fn reg(addr: usize) -> *mut u32 {
    addr as *mut u32
}

pub fn init() {
    unsafe {
        // Route CNTPNSIRQ (the timer generic_timer.rs arms via CNTP_*) to
        // core 0's IRQ line.
        let ctl = reg(CORE0_TIMER_IRQCNTL).read_volatile();
        reg(CORE0_TIMER_IRQCNTL).write_volatile(ctl | CORE0_SRC_CNTPNSIRQ);
        // Enable UART0 in the legacy peripheral controller.
        reg(ENABLE_IRQS_2).write_volatile(1 << (UART_IRQ - 32));
    }
}

pub fn enable(id: u32) {
    unsafe {
        match id {
            TIMER_IRQ => {
                let ctl = reg(CORE0_TIMER_IRQCNTL).read_volatile();
                reg(CORE0_TIMER_IRQCNTL).write_volatile(ctl | CORE0_SRC_CNTPNSIRQ);
            }
            UART_IRQ => reg(ENABLE_IRQS_2).write_volatile(1 << (UART_IRQ - 32)),
            _ => {}
        }
    }
}

pub fn disable(id: u32) {
    unsafe {
        match id {
            TIMER_IRQ => {
                let ctl = reg(CORE0_TIMER_IRQCNTL).read_volatile();
                reg(CORE0_TIMER_IRQCNTL).write_volatile(ctl & !CORE0_SRC_CNTPNSIRQ);
            }
            // The legacy controller only exposes a DISABLE_IRQS_2 *set*
            // register (writing 1 disables, writing 0 is a no-op), no
            // masking needed beyond what `init` already enabled once.
            _ => {}
        }
    }
}

/// No explicit end-of-interrupt step exists for either block: the timer
/// condition clears when `generic_timer::rearm` reprograms `CNTP_TVAL`, and
/// the legacy controller's pending bits are live status (self-clearing once
/// the peripheral itself deasserts), not a latch you acknowledge.
pub fn eoi(_id: u32) {}

/// Read both controllers and dispatch to whichever source is pending.
/// Called from `arch::aarch64::exceptions::aarch64_irq_handler` via the
/// `intc` re-export.
pub fn handle() {
    unsafe {
        let source = reg(CORE0_IRQ_SOURCE).read_volatile();
        if source & CORE0_SRC_CNTPNSIRQ != 0 {
            crate::scheduler::tick();
            crate::drivers::timer::rearm();
            // USB HID keyboard input (`usb::hid`) is polled, not
            // IRQ-driven, a task blocked in `block_on_tty` only gets
            // re-scheduled to re-check it via an explicit wake, which
            // otherwise only ever comes from the UART RX interrupt. Waking
            // it every tick (~10 ms) here is a safe no-op when nothing is
            // actually waiting (`wake_tty_waiter` no-ops with no waiter).
            crate::scheduler::wake_tty_waiter();
        }
        if source & CORE0_SRC_GPU != 0 {
            let pending = reg(IRQ_PENDING_2).read_volatile();
            if pending & (1 << (UART_IRQ - 32)) != 0 {
                crate::drivers::serial::handle_irq();
            }
        }
    }
}
