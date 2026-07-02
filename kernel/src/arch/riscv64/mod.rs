pub mod context;
pub mod trap;
pub mod paging;

// `console`/`sbi`/`plic`/`fdt` are platform (BSP) concerns, not CPU-ISA
// ones — re-exported here (rather than moved call-by-call) so existing
// `super::sbi`/`super::plic`/`super::console` references in `trap.rs` keep
// working unchanged regardless of which riscv64 board is selected.
pub use crate::boards::current::{console, fdt, plic, sbi};
pub use fdt::parse_memory_map;

#[cfg(not(feature = "board-qemu-virt-riscv64"))]
compile_error!("riscv64 build requires the board-qemu-virt-riscv64 feature");

pub fn early_init() {
    trap::init();
    crate::boards::current::init();
}

pub fn interrupts_init() {
    crate::boards::current::interrupts_init();
    unsafe {
        // Enable supervisor-mode global interrupt enable (sstatus.SIE = bit 1)
        core::arch::asm!("csrsi sstatus, 0x2", options(nostack));
        // Enable supervisor timer interrupt (sie.STIE = bit 5) and supervisor
        // external interrupt (sie.SEIE = bit 9) — without SEIE, PLIC-routed
        // interrupts (including the UART's RX interrupt) never trap, so serial
        // input never reaches the software ring buffer and typing appears dead.
        let stie: u64 = (1 << 5) | (1 << 9);
        core::arch::asm!("csrs sie, {}", in(reg) stie, options(nostack));
        // SUM (bit 18): allow S-mode to access U-mode pages (needed for sys_write
        // to read user buffers without causing a page fault).
        let sum: u64 = 1 << 18;
        core::arch::asm!("csrs sstatus, {}", in(reg) sum, options(nostack));
        // Arm the first timer tick; trap.rs re-arms it on every subsequent tick.
        let time: u64;
        core::arch::asm!("csrr {}, time", out(reg) time, options(nostack));
        sbi::set_timer(time + 100_000);
    }
}

pub fn set_kernel_stack(sp: u64) {
    context::set_kernel_stack(sp);
}

pub fn create_process_page_table(code_phys: usize, stack_phys: usize) -> Option<u64> {
    paging::create_process_page_table(code_phys, stack_phys)
}

pub fn create_empty_process_page_table() -> Option<u64> {
    paging::create_empty_process_page_table()
}

pub fn clone_user_page_table(src_phys: u64) -> Option<u64> {
    paging::clone_user_page_table(src_phys)
}

pub fn map_user_page(root_phys: u64, va: usize, pa: usize, prot: u32) -> bool {
    paging::map_user_page(root_phys, va, pa, prot)
}

pub fn kernel_page_table_phys() -> u64 {
    paging::kernel_page_table_phys()
}

pub fn switch_address_space(root_phys: u64) {
    paging::switch_address_space(root_phys)
}

pub fn halt() -> ! {
    loop {
        unsafe { core::arch::asm!("wfi", options(nostack)) };
    }
}
