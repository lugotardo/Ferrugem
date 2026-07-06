#[cfg(target_arch = "x86_64")]
pub mod x86_64;
#[cfg(target_arch = "x86_64")]
pub use x86_64::{early_init, interrupts_init, halt, parse_memory_map};

#[cfg(target_arch = "riscv64")]
pub mod riscv64;
#[cfg(target_arch = "riscv64")]
pub use riscv64::{early_init, interrupts_init, halt, parse_memory_map};

#[cfg(target_arch = "aarch64")]
pub mod aarch64;
#[cfg(target_arch = "aarch64")]
pub use aarch64::{early_init, interrupts_init, halt, parse_memory_map};

// ── Context-switch HAL ────────────────────────────────────────────────────────

/// Save current task's kernel RSP into `*cur_sp` and switch to `next_sp`.
/// Interrupts should be disabled by the caller.
///
/// # Safety
/// Both stack pointers must be valid kernel stacks set up by `task_init_stack`
/// (or saved by a prior `context_switch` call for the same task).
pub unsafe fn context_switch(cur_sp: *mut u64, next_sp: u64) {
    #[cfg(target_arch = "x86_64")]
    x86_64::context::context_switch(cur_sp, next_sp);

    #[cfg(target_arch = "riscv64")]
    riscv64::context::context_switch(cur_sp, next_sp);

    #[cfg(target_arch = "aarch64")]
    aarch64::context::context_switch(cur_sp, next_sp);
}

/// Initialise a new task's kernel stack so `context_switch` can resume it at `entry`.
/// Returns the initial `kernel_sp` to store in the task struct.
///
/// # Safety
/// `stack` must be a valid, exclusively-owned byte buffer with at least 64 bytes.
pub unsafe fn task_init_stack(stack: &mut [u8], entry: fn() -> !) -> u64 {
    #[cfg(target_arch = "x86_64")]
    return x86_64::context::task_init_stack(stack, entry);

    #[cfg(target_arch = "riscv64")]
    return riscv64::context::task_init_stack(stack, entry);

    #[cfg(target_arch = "aarch64")]
    return aarch64::context::task_init_stack(stack, entry);
}

/// Build a kernel stack so `context_switch` will enter unprivileged mode at `user_rip`.
/// On x86_64: stack frame ends with `iretq` via trampoline.
/// On RISC-V: ra=trampoline, s0=user_rsp, s1=user_rip, s2=sstatus; trampoline does `sret`.
///
/// # Safety
/// `stack` must be a valid, exclusively-owned byte buffer with at least 96 bytes.
pub unsafe fn task_init_userspace_stack(stack: &mut [u8], user_rip: u64, user_rsp: u64) -> u64 {
    #[cfg(target_arch = "x86_64")]
    return x86_64::context::task_init_userspace_stack(stack, user_rip, user_rsp);

    #[cfg(target_arch = "riscv64")]
    return riscv64::context::task_init_userspace_stack(stack, user_rip, user_rsp);

    #[cfg(target_arch = "aarch64")]
    return aarch64::context::task_init_userspace_stack(stack, user_rip, user_rsp);
}

/// Like `task_init_userspace_stack` but the child enters with rax=0 (fork returns 0 in child).
/// `a0..a5` are the 6 syscall argument registers, and `callee_saved` is
/// [rbx, rbp, r12, r13, r14, r15], both captured at the time of the clone()/fork()
/// call. A real clone() duplicates the entire register file except rax, and
/// compiled code (libc's `__clone` trampoline keeps its entry-point function
/// pointer in a register, not on the stack) depends on that, so the child must
/// resume with the exact same values or it jumps to garbage / reads NULL.
#[allow(clippy::too_many_arguments)]
pub unsafe fn task_init_fork_stack(
    stack: &mut [u8], user_rip: u64, user_rsp: u64,
    a0: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64,
    callee_saved: [u64; 6],
) -> u64 {
    #[cfg(target_arch = "x86_64")]
    return x86_64::context::task_init_fork_stack(stack, user_rip, user_rsp, a0, a1, a2, a3, a4, a5, callee_saved);

    #[cfg(target_arch = "riscv64")]
    {
        let _ = (a0, a1, a2, a3, a4, a5, callee_saved);
        return riscv64::context::task_init_userspace_stack(stack, user_rip, user_rsp);
    }

    #[cfg(target_arch = "aarch64")]
    return aarch64::context::task_init_fork_stack(
        stack, user_rip, user_rsp, a0, a1, a2, a3, a4, a5, callee_saved,
    );
}

// ── Page protection flags (arch-neutral) ─────────────────────────────────────

pub const PROT_READ:  u32 = 1 << 0;
pub const PROT_WRITE: u32 = 1 << 1;
pub const PROT_EXEC:  u32 = 1 << 2;
pub const PROT_USER:  u32 = 1 << 3;

// ── User virtual address layout (arch-neutral re-exports) ────────────────────

#[cfg(target_arch = "x86_64")]
pub const USER_CODE_VA:   usize = x86_64::paging::USER_CODE_VA;
#[cfg(target_arch = "x86_64")]
pub const USER_STACK_TOP: usize = x86_64::paging::USER_STACK_TOP;
/// Base VA where PIE ELFs are loaded (start of user address space).
#[cfg(target_arch = "x86_64")]
pub const USER_BASE_VA: usize = 0x0000_0080_0000_0000;
/// Initial stack top for ELF processes. Placed 256 MiB above USER_BASE_VA so
/// it cannot overlap even large statically-linked musl binaries (~420 KiB now).
#[cfg(target_arch = "x86_64")]
pub const USER_ELF_STACK_TOP: usize = USER_BASE_VA + 0x1000_0000; // +256 MiB

#[cfg(target_arch = "riscv64")]
pub const USER_CODE_VA:   usize = riscv64::paging::USER_CODE_VA;
#[cfg(target_arch = "riscv64")]
pub const USER_STACK_TOP: usize = riscv64::paging::USER_STACK_TOP;
/// Base VA where PIE ELFs are loaded (start of user address space = L2[4]).
#[cfg(target_arch = "riscv64")]
pub const USER_BASE_VA: usize = 0x1_0000_0000;
/// Initial stack top for ELF processes on RISC-V (256 MiB above base).
#[cfg(target_arch = "riscv64")]
pub const USER_ELF_STACK_TOP: usize = USER_BASE_VA + 0x1000_0000; // +256 MiB

#[cfg(target_arch = "aarch64")]
pub const USER_CODE_VA: usize = aarch64::USER_CODE_VA;
#[cfg(target_arch = "aarch64")]
pub const USER_STACK_TOP: usize = aarch64::USER_STACK_TOP;
/// Reserved for Fase 2 (EL0 userspace), see `aarch64::mod.rs`.
#[cfg(target_arch = "aarch64")]
pub const USER_BASE_VA: usize = aarch64::USER_BASE_VA;
#[cfg(target_arch = "aarch64")]
pub const USER_ELF_STACK_TOP: usize = aarch64::USER_ELF_STACK_TOP;

// ── Per-process page table management ────────────────────────────────────────

/// Allocate an isolated page table for a new user process mapping `code_phys`
/// and `stack_phys` at the canonical user VAs.  Returns the physical address
/// of the root table, or `None` on OOM.
pub fn create_process_page_table(code_phys: usize, stack_phys: usize) -> Option<u64> {
    #[cfg(target_arch = "x86_64")]
    return x86_64::paging::create_process_page_table(code_phys, stack_phys);

    #[cfg(target_arch = "riscv64")]
    return riscv64::create_process_page_table(code_phys, stack_phys);

    #[cfg(target_arch = "aarch64")]
    return aarch64::create_process_page_table(code_phys, stack_phys);

    #[allow(unreachable_code)]
    None
}

/// Allocate an empty process page table containing only kernel entries.
/// User mappings are added later via `map_user_page`.
/// Returns the physical address of the root table, or `None` on OOM.
pub fn create_empty_process_page_table() -> Option<u64> {
    #[cfg(target_arch = "x86_64")]
    return x86_64::paging::create_empty_process_page_table();

    #[cfg(target_arch = "riscv64")]
    return riscv64::create_empty_process_page_table();

    #[cfg(target_arch = "aarch64")]
    return aarch64::create_empty_process_page_table();

    #[allow(unreachable_code)]
    None
}

/// Deep-copy a process page table: new physical frames for all user pages,
/// fresh intermediate tables, shared kernel entries.  Returns new root phys.
pub fn clone_user_page_table(src_phys: u64) -> Option<u64> {
    #[cfg(target_arch = "x86_64")]
    return x86_64::paging::clone_user_page_table(src_phys);
    #[cfg(target_arch = "riscv64")]
    return riscv64::clone_user_page_table(src_phys);
    #[cfg(target_arch = "aarch64")]
    return aarch64::clone_user_page_table(src_phys);
    #[allow(unreachable_code)]
    None
}

/// Map a single 4 KiB page at virtual address `va` to physical address `pa`
/// inside the page table rooted at `pt_phys`.  `prot` is a bitmask of
/// `PROT_READ | PROT_WRITE | PROT_EXEC | PROT_USER`.
/// Returns false if `va` is in the kernel range, on OOM, or if `va` hits an
/// existing huge-page leaf that cannot be subdivided.
pub fn map_user_page(pt_phys: u64, va: usize, pa: usize, prot: u32) -> bool {
    #[cfg(target_arch = "x86_64")]
    return x86_64::paging::map_user_page(pt_phys, va, pa, prot);

    #[cfg(target_arch = "riscv64")]
    return riscv64::map_user_page(pt_phys, va, pa, prot);

    #[cfg(target_arch = "aarch64")]
    return aarch64::map_user_page(pt_phys, va, pa, prot);

    #[allow(unreachable_code)]
    false
}

/// Physical address of the kernel's root page table (PML4 / Sv39 root).
pub fn kernel_page_table_phys() -> u64 {
    #[cfg(target_arch = "x86_64")]
    return x86_64::paging::kernel_page_table_phys();

    #[cfg(target_arch = "riscv64")]
    return riscv64::kernel_page_table_phys();

    #[cfg(target_arch = "aarch64")]
    return aarch64::kernel_page_table_phys();

    #[allow(unreachable_code)]
    0
}

/// Load a page table root (flushes TLB).  Pass the physical address returned
/// by `create_process_page_table` or `kernel_page_table_phys`.
pub fn switch_address_space(pt_phys: u64) {
    #[cfg(target_arch = "x86_64")]
    x86_64::paging::switch_address_space(pt_phys);

    #[cfg(target_arch = "riscv64")]
    riscv64::switch_address_space(pt_phys);

    #[cfg(target_arch = "aarch64")]
    aarch64::switch_address_space(pt_phys);
}

/// Atomically read the current interrupt-enable state, then disable interrupts.
/// Returns `true` if interrupts were enabled before the call.
/// On x86_64 this reads RFLAGS.IF and issues `cli`.
/// On RISC-V this atomically clears sstatus.SIE via `csrrci`.
pub fn save_and_disable_interrupts() -> bool {
    unsafe {
        #[cfg(target_arch = "x86_64")] {
            let flags: u64;
            core::arch::asm!(
                "pushfq",
                "pop {f}",
                "cli",
                f = out(reg) flags,
            );
            (flags >> 9) & 1 != 0
        }
        #[cfg(target_arch = "riscv64")] {
            let prev: u64;
            core::arch::asm!(
                "csrrci {p}, sstatus, 0x2",
                p = out(reg) prev,
                options(nostack)
            );
            (prev >> 1) & 1 != 0
        }
        #[cfg(target_arch = "aarch64")] {
            let daif: u64;
            core::arch::asm!("mrs {d}, daif", d = out(reg) daif, options(nostack));
            core::arch::asm!("msr daifset, #2", options(nostack)); // mask IRQ (I bit)
            (daif >> 7) & 1 == 0
        }
    }
}

/// Re-enable interrupts if they were enabled before `save_and_disable_interrupts`.
pub fn restore_interrupt_state(was_enabled: bool) {
    if was_enabled {
        unsafe {
            #[cfg(target_arch = "x86_64")]
            core::arch::asm!("sti", options(nostack));
            #[cfg(target_arch = "riscv64")]
            core::arch::asm!("csrsi sstatus, 0x2", options(nostack));
            #[cfg(target_arch = "aarch64")]
            core::arch::asm!("msr daifclr, #2", options(nostack));
        }
    }
}

/// Update the arch-specific kernel-stack pointer used on privilege-level transitions.
/// On x86_64 this writes TSS.rsp0; on RISC-V it would update sscratch.
pub fn set_kernel_stack(sp: u64) {
    #[cfg(target_arch = "x86_64")]
    x86_64::set_kernel_stack(sp);

    #[cfg(target_arch = "riscv64")]
    riscv64::set_kernel_stack(sp);

    #[cfg(target_arch = "aarch64")]
    aarch64::set_kernel_stack(sp);
}

/// Print a string via the arch-specific early console (VGA+COM1 or SBI).
pub fn console_print_str(s: &str) {
    #[cfg(target_arch = "x86_64")]
    x86_64::console::print_str(s);
    #[cfg(target_arch = "riscv64")]
    riscv64::console::print_str(s);
    #[cfg(target_arch = "aarch64")]
    aarch64::console::print_str(s);
}

/// Return the architecture name string.
pub fn name() -> &'static str {
    #[cfg(target_arch = "x86_64")]  { "x86_64"  }
    #[cfg(target_arch = "riscv64")] { "riscv64" }
    #[cfg(target_arch = "aarch64")] { "aarch64" }
}

/// Clear the console (VGA clear on x86_64, ANSI escape on RISC-V/aarch64).
pub fn console_clear() {
    #[cfg(target_arch = "x86_64")]
    x86_64::console::clear();
    #[cfg(target_arch = "riscv64")]
    riscv64::console::print_str("\x1b[2J\x1b[H");
    #[cfg(target_arch = "aarch64")]
    aarch64::console::print_str("\x1b[2J\x1b[H");
}

/// Initialise arch-specific paging (called from memory::init).
pub fn paging_init() {
    #[cfg(target_arch = "x86_64")]
    x86_64::paging::init();
    #[cfg(target_arch = "riscv64")]
    riscv64::paging::init();
    #[cfg(target_arch = "aarch64")]
    aarch64::paging::init();
}

/// Board bring-up that can only run once paging is active, e.g. the
/// Raspberry Pi 3's HDMI/VideoCore-mailbox framebuffer, whose physical address is
/// decided by firmware at runtime and needs remapping via `paging::
/// map_uncached`, which needs the identity map `paging_init` just built.
/// The USB host stack doesn't strictly need paging up, but is grouped here
/// too since both are RPi3-only, best-effort hardware bring-up steps.
/// Called from `init::kernel_main` right after `memory::init`. A no-op on
/// every board that doesn't need it (which is all of them except RPi3).
pub fn board_late_init() {
    #[cfg(all(target_arch = "aarch64", feature = "board-raspberrypi3"))]
    {
        crate::boards::raspberrypi3::hdmi::init();
        crate::boards::raspberrypi3::usb::init();
    }
}

/// Mark a 4 KiB physical page as not-present (guard page).
/// Returns `true` if the MMU entry was successfully removed, `false` on any
/// failure (OOM, unsupported on this arch) callers fall back to canary only.
pub fn protect_guard_page(phys: usize) -> bool {
    #[cfg(target_arch = "x86_64")]
    return x86_64::paging::protect_guard_page(phys);
    #[cfg(target_arch = "riscv64")]
    return riscv64::paging::protect_guard_page(phys);
    #[cfg(target_arch = "aarch64")]
    return aarch64::paging::protect_guard_page(phys);
    #[allow(unreachable_code)]
    false
}

/// Read the x86_64 FS.base MSR (IA32_FS_BASE = 0xC0000100).
/// On RISC-V, always returns 0 (no equivalent register).
pub fn read_fs_base() -> u64 {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        let lo: u32; let hi: u32;
        core::arch::asm!(
            "rdmsr",
            in("ecx")  0xC0000100u32,
            out("eax") lo,
            out("edx") hi,
            options(nostack)
        );
        (lo as u64) | ((hi as u64) << 32)
    }
    #[cfg(target_arch = "riscv64")]
    { 0 }
    #[cfg(target_arch = "aarch64")]
    { 0 } // Fase 2: TPIDR_EL0 is the aarch64 equivalent, unused until EL0 exists
}

/// Snapshot of the SysV callee-saved GPRs (rbx, rbp, r12-r15) at the current
/// syscall entry, needed so fork()/clone() can hand them to the child exactly
/// as a real clone() syscall would (it duplicates the whole register file
/// except rax, not just the syscall-argument registers).
/// On RISC-V, always returns zeros (fork() isn't exercised by real userspace yet).
pub fn user_callee_saved_snapshot() -> [u64; 6] {
    #[cfg(target_arch = "x86_64")]
    return x86_64::idt::callee_saved_snapshot();

    #[cfg(target_arch = "riscv64")]
    { [0; 6] }
    #[cfg(target_arch = "aarch64")]
    { [0; 6] } // fork()/clone() aren't exercised by real userspace yet (no EL0)
}

/// Write the x86_64 FS.base MSR. No-op on RISC-V.
pub fn write_fs_base(val: u64) {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!(
            "wrmsr",
            in("ecx")  0xC0000100u32,
            in("eax")  (val & 0xFFFF_FFFF) as u32,
            in("edx")  (val >> 32) as u32,
            options(nostack)
        );
    }
    #[cfg(target_arch = "riscv64")]
    { let _ = val; }
    #[cfg(target_arch = "aarch64")]
    { let _ = val; }
}

/// Flush the TLB entry for a single virtual address.
/// Must be called after adding or modifying a page table entry at `va`.
pub fn flush_tlb_page(va: usize) {
    unsafe {
        #[cfg(target_arch = "x86_64")]
        core::arch::asm!("invlpg [{va}]", va = in(reg) va, options(nostack));
        #[cfg(target_arch = "riscv64")]
        core::arch::asm!("sfence.vma {va}, zero", va = in(reg) va, options(nostack));
        #[cfg(target_arch = "aarch64")]
        {
            let v = (va >> 12) as u64;
            core::arch::asm!("tlbi vaae1is, {v}", "dsb ish", "isb", v = in(reg) v, options(nostack));
        }
    }
}

/// Return raw entropy bits from the best available hardware source.
/// x86_64: RDTSC (universally available on x86_64).  RISC-V: time CSR.
pub fn entropy_seed() -> u64 {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        let lo: u32;
        let hi: u32;
        core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi, options(nostack));
        let tsc = (lo as u64) | ((hi as u64) << 32);
        tsc.wrapping_mul(0x9e3779b97f4a7c15).rotate_right(17)
    }
    #[cfg(target_arch = "riscv64")]
    {
        let t: u64;
        unsafe { core::arch::asm!("csrr {}, time", out(reg) t, options(nostack)) };
        t.wrapping_mul(0x9e3779b97f4a7c15)
    }
    #[cfg(target_arch = "aarch64")]
    {
        let t: u64;
        unsafe { core::arch::asm!("mrs {}, cntvct_el0", out(reg) t, options(nostack)) };
        t.wrapping_mul(0x9e3779b97f4a7c15)
    }
}
