pub mod console;
pub mod context;
pub mod fdt;
pub mod sbi;
pub mod trap;
pub mod plic;
pub mod paging;

pub use fdt::parse_memory_map;

pub fn early_init() {
    trap::init();
    console::init();
}

pub fn interrupts_init() {
    plic::init();
    unsafe {
        // Enable supervisor-mode global interrupt enable (sstatus.SIE = bit 1)
        core::arch::asm!("csrsi sstatus, 0x2", options(nostack));
        // Enable supervisor timer interrupt (sie.STIE = bit 5).
        let stie: u64 = 1 << 5;
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
