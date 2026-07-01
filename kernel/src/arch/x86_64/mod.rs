pub mod console;
pub mod context;
pub mod gdt;
pub mod idt;
pub mod multiboot;
pub mod pic;
pub mod port;
pub mod paging;

pub use multiboot::parse_memory_map;

pub fn early_init() {
    gdt::init();
    idt::init();
    pic::init();
    console::init();
}

pub fn interrupts_init() {
    // Unmask timer (IRQ0) and keyboard (IRQ1)
    pic::unmask(0);
    pic::unmask(1);
    unsafe { core::arch::asm!("sti") };
}

pub fn halt() -> ! {
    loop {
        unsafe { core::arch::asm!("hlt") };
    }
}

/// Update TSS.rsp0 to the top of the current task's kernel stack.
pub fn set_kernel_stack(sp: u64) {
    gdt::set_rsp0(sp);
}
