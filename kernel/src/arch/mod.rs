#[cfg(target_arch = "x86_64")]
pub mod x86_64;
#[cfg(target_arch = "x86_64")]
pub use x86_64::{early_init, interrupts_init, halt, parse_memory_map};

#[cfg(target_arch = "riscv64")]
pub mod riscv64;
#[cfg(target_arch = "riscv64")]
pub use riscv64::{early_init, interrupts_init, halt, parse_memory_map};

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

#[cfg(target_arch = "riscv64")]
pub const USER_CODE_VA:   usize = riscv64::paging::USER_CODE_VA;
#[cfg(target_arch = "riscv64")]
pub const USER_STACK_TOP: usize = riscv64::paging::USER_STACK_TOP;
/// Base VA where PIE ELFs are loaded (start of user address space = L2[4]).
#[cfg(target_arch = "riscv64")]
pub const USER_BASE_VA: usize = 0x1_0000_0000;

// ── Per-process page table management ────────────────────────────────────────

/// Allocate an isolated page table for a new user process mapping `code_phys`
/// and `stack_phys` at the canonical user VAs.  Returns the physical address
/// of the root table, or `None` on OOM.
pub fn create_process_page_table(code_phys: usize, stack_phys: usize) -> Option<u64> {
    #[cfg(target_arch = "x86_64")]
    return x86_64::paging::create_process_page_table(code_phys, stack_phys);

    #[cfg(target_arch = "riscv64")]
    return riscv64::create_process_page_table(code_phys, stack_phys);

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

    #[allow(unreachable_code)]
    false
}

/// Physical address of the kernel's root page table (PML4 / Sv39 root).
pub fn kernel_page_table_phys() -> u64 {
    #[cfg(target_arch = "x86_64")]
    return x86_64::paging::kernel_page_table_phys();

    #[cfg(target_arch = "riscv64")]
    return riscv64::kernel_page_table_phys();

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
}

/// Print a string via the arch-specific early console (VGA+COM1 or SBI).
pub fn console_print_str(s: &str) {
    #[cfg(target_arch = "x86_64")]
    x86_64::console::print_str(s);
    #[cfg(target_arch = "riscv64")]
    riscv64::console::print_str(s);
}

/// Return the architecture name string.
pub fn name() -> &'static str {
    #[cfg(target_arch = "x86_64")]  { "x86_64"  }
    #[cfg(target_arch = "riscv64")] { "riscv64" }
}

/// Clear the console (VGA clear on x86_64, ANSI escape on RISC-V).
pub fn console_clear() {
    #[cfg(target_arch = "x86_64")]
    x86_64::console::clear();
    #[cfg(target_arch = "riscv64")]
    riscv64::console::print_str("\x1b[2J\x1b[H");
}

/// Initialise arch-specific paging (called from memory::init).
pub fn paging_init() {
    #[cfg(target_arch = "x86_64")]
    x86_64::paging::init();
    #[cfg(target_arch = "riscv64")]
    riscv64::paging::init();
}

/// Mark a 4 KiB physical page as not-present (guard page).
/// Returns `true` if the MMU entry was successfully removed, `false` on any
/// failure (OOM, unsupported on this arch) callers fall back to canary only.
pub fn protect_guard_page(phys: usize) -> bool {
    #[cfg(target_arch = "x86_64")]
    return x86_64::paging::protect_guard_page(phys);
    #[cfg(target_arch = "riscv64")]
    return riscv64::paging::protect_guard_page(phys);
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
}

/// Flush the TLB entry for a single virtual address.
/// Must be called after adding or modifying a page table entry at `va`.
pub fn flush_tlb_page(va: usize) {
    unsafe {
        #[cfg(target_arch = "x86_64")]
        core::arch::asm!("invlpg [{va}]", va = in(reg) va, options(nostack));
        #[cfg(target_arch = "riscv64")]
        core::arch::asm!("sfence.vma {va}, zero", va = in(reg) va, options(nostack));
    }
}

/// Return raw entropy bits from the best available hardware source.
/// x86_64: RDRAND.  RISC-V: time CSR mixed with a constant.
pub fn entropy_seed() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        let mut val: u64 = 0;
        unsafe {
            for _ in 0..10 {
                let ok: u8;
                core::arch::asm!(
                    "rdrand {0}", "setc {1}",
                    out(reg) val, out(reg_byte) ok,
                    options(nostack)
                );
                if ok != 0 { break; }
            }
        }
        val
    }
    #[cfg(target_arch = "riscv64")]
    {
        let t: u64;
        unsafe { core::arch::asm!("csrr {}, time", out(reg) t, options(nostack)) };
        t.wrapping_mul(0x9e3779b97f4a7c15)
    }
}

// HAL traits implemented per arch
pub trait Timer {
    fn init();
    fn ticks() -> u64;
}

pub trait InterruptController {
    fn enable(id: u32);
    fn disable(id: u32);
    fn eoi(id: u32);
}

pub trait Console {
    fn write_byte(b: u8);
    fn write_str(s: &str) {
        for b in s.bytes() {
            Self::write_byte(b);
        }
    }
}
