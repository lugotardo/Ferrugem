pub mod context;
pub mod gdt;
pub mod idt;
pub mod pci;
pub mod port;
pub mod paging;

// `console`/`multiboot`/`pic` are platform (BSP) concerns, not CPU-ISA ones —
// re-exported here (rather than moved call-by-call) so existing `super::pic`/
// `super::console` references in `idt.rs` keep working unchanged regardless
// of which x86_64 board is selected.
pub use crate::boards::current::{console, multiboot, pic};
pub use multiboot::parse_memory_map;

#[cfg(not(any(feature = "board-qemu-pc", feature = "board-virtualbox")))]
compile_error!("x86_64 build requires a board-* feature (board-qemu-pc or board-virtualbox)");
#[cfg(all(feature = "board-qemu-pc", feature = "board-virtualbox"))]
compile_error!("select exactly one x86_64 board feature");

pub fn early_init() {
    // Enable SSE/SSE2: clear CR0.EM (bit 2), set CR0.MP (bit 1),
    // set CR4.OSFXSR (bit 9) and CR4.OSXMMEXCPT (bit 10).
    // This must happen before any SSE instruction reaches the CPU, including
    // those emitted by musl's memcpy/memset in user space.
    unsafe {
        // Enable SSE/SSE2: clear CR0.EM (bit 2), set CR0.MP (bit 1),
        // set CR4.OSFXSR (bit 9) and CR4.OSXMMEXCPT (bit 10).
        let cr0: u64;
        core::arch::asm!("mov {}, cr0", out(reg) cr0, options(nostack));
        let cr0 = (cr0 & !(1u64 << 2)) | (1u64 << 1);
        core::arch::asm!("mov cr0, {}", in(reg) cr0, options(nostack));
        let cr4: u64;
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nostack));
        let cr4 = cr4 | (1u64 << 9) | (1u64 << 10);
        core::arch::asm!("mov cr4, {}", in(reg) cr4, options(nostack));
    }
    gdt::init();
    idt::init();
    unsafe { setup_syscall(); }
    crate::boards::current::init();
}

/// Configure the x86_64 SYSCALL/SYSRETQ mechanism.
///
/// MSR layout:
///   IA32_EFER[SCE]  = 1                           (enable SYSCALL)
///   IA32_STAR[47:32]= 0x08 (kernel CS)            (used by SYSCALL for CS/SS)
///   IA32_STAR[63:48]= 0x10 (user data GDT base)   (used by SYSRETQ: SS=+8, CS=+16)
///     → SYSRETQ SS = 0x10+8  | 3 = 0x1B = GDT user data (0x18)
///     → SYSRETQ CS = 0x10+16 | 3 = 0x23 = GDT user code (0x20)
///   IA32_LSTAR      = &syscall_entry_asm           (handler address)
///   IA32_FMASK      = 0x200                        (clear IF on SYSCALL entry)
unsafe fn setup_syscall() {
    unsafe fn wrmsr(ecx: u32, val: u64) {
        core::arch::asm!(
            "wrmsr",
            in("ecx")  ecx,
            in("eax")  (val & 0xFFFF_FFFF) as u32,
            in("edx")  (val >> 32) as u32,
            options(nostack)
        );
    }
    unsafe fn rdmsr(ecx: u32) -> u64 {
        let lo: u32; let hi: u32;
        core::arch::asm!("rdmsr", in("ecx") ecx, out("eax") lo, out("edx") hi, options(nostack));
        (lo as u64) | ((hi as u64) << 32)
    }

    // Set SCE (System Call Enable) in IA32_EFER
    let efer = rdmsr(0xC0000080) | 1;
    wrmsr(0xC0000080, efer);

    // IA32_STAR: kernel CS=0x08, user data base=0x10
    let star = (0x0010u64 << 48) | (0x0008u64 << 32);
    wrmsr(0xC0000081, star);

    // IA32_LSTAR: SYSCALL handler
    wrmsr(0xC0000082, idt::syscall_entry_asm as u64);

    // IA32_FMASK: clear IF (interrupt flag) so timer cannot fire before stack switch
    wrmsr(0xC0000084, 0x200);
}

pub fn interrupts_init() {
    crate::boards::current::unmask_default_irqs();
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
