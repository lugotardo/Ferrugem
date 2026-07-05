/// aarch64 cooperative context switch (callee-saved per AAPCS64: x19-x28,
/// x29 (FP), x30 (LR)). Same role as `riscv64::context` (ra + s0-s11).
///
/// Calling convention: AAPCS64
///   x0 = cur_sp  (*mut u64 receives current SP after saving)
///   x1 = next_sp (u64     SP to restore for next task)
///
/// Stack layout when a task is suspended (from SP upward):
///   [+0]   x30 (LR)  <- `ret` resumes here
///   [+8]   x29 (FP)
///   [+16]  x19
///   [+24]  x20
///   [+32]  x21
///   [+40]  x22
///   [+48]  x23
///   [+56]  x24
///   [+64]  x25
///   [+72]  x26
///   [+80]  x27
///   [+88]  x28

#[unsafe(naked)]
pub unsafe extern "C" fn context_switch(cur_sp: *mut u64, next_sp: u64) {
    core::arch::naked_asm!(
        "sub sp, sp, #96",
        "stp x30, x29, [sp, #0]",
        "stp x19, x20, [sp, #16]",
        "stp x21, x22, [sp, #32]",
        "stp x23, x24, [sp, #48]",
        "stp x25, x26, [sp, #64]",
        "stp x27, x28, [sp, #80]",
        "mov x2, sp",   // SP can't be a store operand directly
        "str x2, [x0]", // *cur_sp = sp
        "mov sp, x1",   // sp = next_sp
        "ldp x30, x29, [sp, #0]",
        "ldp x19, x20, [sp, #16]",
        "ldp x21, x22, [sp, #32]",
        "ldp x23, x24, [sp, #48]",
        "ldp x25, x26, [sp, #64]",
        "ldp x27, x28, [sp, #80]",
        "add sp, sp, #96",
        "ret",
    )
}

/// Initialise a new task's kernel stack so `context_switch` can resume it at `entry`.
/// Returns the initial `kernel_sp` to store in the task struct.
///
/// # Safety
/// `stack` must be a valid, exclusively-owned byte buffer with at least 96 bytes.
pub unsafe fn task_init_stack(stack: &mut [u8], entry: fn() -> !) -> u64 {
    let top = stack.as_mut_ptr().add(stack.len()) as usize;
    let top = top & !0xF; // 16-byte align down
    let sp = (top - 12 * 8) as *mut u64;
    sp.add(0).write(entry as u64); // x30 (LR) = entry
    for i in 1..12usize {
        sp.add(i).write(0); // x29, x19-x28 = 0
    }
    sp as u64
}

/// Store the kernel stack pointer for future EL0->EL1 trap handling.
/// Fase 1 has no EL0 tasks yet, so this only stores the value for later use.
pub fn set_kernel_stack(sp: u64) {
    unsafe {
        core::arch::asm!("msr tpidr_el1, {sp}", sp = in(reg) sp, options(nostack));
    }
}

/// Userspace (EL0) task entry is Fase 2 work: it needs SVC-based syscall
/// entry, ELR_EL1/SPSR_EL1 configured for an EL0 return, and a real
/// per-process TTBR0_EL1, none of which exist yet (see `paging.rs`).
pub unsafe fn task_init_userspace_stack(_stack: &mut [u8], _user_rip: u64, _user_rsp: u64) -> u64 {
    unimplemented!("aarch64 fase 2: EL0 userspace not implemented yet")
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn task_init_fork_stack(
    _stack: &mut [u8], _user_rip: u64, _user_rsp: u64,
    _a0: u64, _a1: u64, _a2: u64, _a3: u64, _a4: u64, _a5: u64,
    _callee_saved: [u64; 6],
) -> u64 {
    unimplemented!("aarch64 fase 2: EL0 userspace not implemented yet")
}
