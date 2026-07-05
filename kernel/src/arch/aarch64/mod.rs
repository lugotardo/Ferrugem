pub mod boot;
pub mod console;
pub mod context;
pub mod exceptions;
pub mod paging;

// `intc` and `parse_memory_map` are platform (BSP) concerns, not CPU-ISA
// ones, re-exported here (rather than moved call-by-call) so the existing
// `super::intc::handle()` reference in `exceptions.rs` keeps working
// unchanged regardless of which aarch64 board is selected. Every board
// exposes its interrupt controller as `intc` (QEMU virt's is a real GICv2,
// re-exported from its own `gic` module under that name; the Raspberry Pi
// 3 has no GIC at all, so `intc` is the only accurate cross-board name).
pub use crate::boards::current::intc;
pub use crate::boards::current::parse_memory_map;

#[cfg(not(any(feature = "board-qemu-virt-aarch64", feature = "board-raspberrypi3")))]
compile_error!("aarch64 build requires a board-* feature (board-qemu-virt-aarch64 or board-raspberrypi3)");
#[cfg(all(feature = "board-qemu-virt-aarch64", feature = "board-raspberrypi3"))]
compile_error!("select exactly one aarch64 board feature");

/// Reserved for Fase 2 (EL0 userspace), not a real per-process VA layout
/// yet, defined only so `arch::mod.rs`'s cross-arch consts stay symmetric.
pub const USER_BASE_VA: usize = 0x40_0000_0000;
pub const USER_CODE_VA: usize = USER_BASE_VA;
pub const USER_STACK_TOP: usize = USER_BASE_VA + 0x1_0000;
pub const USER_ELF_STACK_TOP: usize = USER_BASE_VA + 0x1000_0000; // +256 MiB

pub fn early_init() {
    exceptions::init();
    console::init();
}

pub fn interrupts_init() {
    intc::init();
    unsafe {
        core::arch::asm!("msr daifclr, #2", options(nostack)); // unmask IRQs
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
