/// PLIC Platform-Level Interrupt Controller (QEMU virt machine).
/// Base address for qemu-virt: 0x0C00_0000

const PLIC_BASE: usize = 0x0C00_0000;

// UART0 is PLIC source 10 on QEMU virt
const UART_IRQ: u32 = 10;

// S-mode context for hart 0 on QEMU virt = context 1
const CONTEXT: usize = 1;

fn priority_reg(source: u32) -> *mut u32 {
    (PLIC_BASE + source as usize * 4) as *mut u32
}

fn enable_reg(context: usize, source: u32) -> *mut u32 {
    (PLIC_BASE + 0x2000 + context * 0x80 + (source as usize / 32) * 4) as *mut u32
}

fn threshold_reg(context: usize) -> *mut u32 {
    (PLIC_BASE + 0x200000 + context * 0x1000) as *mut u32
}

fn claim_reg(context: usize) -> *mut u32 {
    (PLIC_BASE + 0x200004 + context * 0x1000) as *mut u32
}

pub fn init() {
    unsafe {
        // Set UART IRQ priority to 1
        priority_reg(UART_IRQ).write_volatile(1);

        // Enable UART IRQ for S-mode context
        let reg = enable_reg(CONTEXT, UART_IRQ);
        let val = reg.read_volatile();
        reg.write_volatile(val | (1 << (UART_IRQ % 32)));

        // Set threshold to 0 (allow all priorities ≥ 1)
        threshold_reg(CONTEXT).write_volatile(0);
    }
}

pub fn handle() {
    unsafe {
        let source = claim_reg(CONTEXT).read_volatile();
        if source == UART_IRQ {
            crate::drivers::serial::handle_irq();
        }
        // Complete the claim
        claim_reg(CONTEXT).write_volatile(source);
    }
}
