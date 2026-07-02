/// x86_64 cooperative context switch (callee-saved: rbx, rbp, r12-r15).
///
/// Calling convention: System V AMD64 ABI
///   rdi = cur_sp  (*mut u64 receives current RSP after saving)
///   rsi = next_sp (u64     RSP to restore for next task)
///
/// Stack layout a task has before being resumed (from RSP upward):
///   [+0]  r15 = 0
///   [+8]  r14 = 0
///   [+16] r13 = 0
///   [+24] r12 = 0
///   [+32] rbp = 0
///   [+40] rbx = 0
///   [+48] entry_fn  <- ret jumps here

#[unsafe(naked)]
pub unsafe extern "C" fn context_switch(cur_sp: *mut u64, next_sp: u64) {
    core::arch::naked_asm!(
        "push rbx",
        "push rbp",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "mov qword ptr [rdi], rsp",
        "mov rsp, rsi",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbp",
        "pop rbx",
        "ret",
    )
}

/// Naked stub: after `context_switch` does `ret` into this, RSP points at the
/// CPU iretq frame (rip, cs, rflags, rsp, ss) that `task_init_userspace_stack`
/// placed on the kernel stack.  A single `iretq` drops to ring 3.
#[unsafe(naked)]
pub unsafe extern "C" fn user_enter_trampoline() {
    core::arch::naked_asm!("iretq");
}

/// Like `user_enter_trampoline` but zeroes rax first so fork() returns 0 in the child,
/// and restores rdi/rsi/rdx/r10/r8/r9 (the 6 syscall argument registers) plus
/// rbx/rbp/r12-r15 (SysV callee-saved) to their values at the clone()/fork() call.
///
/// A real clone() syscall preserves every register except rax/rcx/r11 across the
/// fork — it duplicates the entire register file, not just the syscall args.
/// libc's `__clone` trampoline relies on this, keeping its entry-point function
/// pointer in r9 (not on the stack) and calling `*r9` in the child after the
/// syscall returns; compiled callee-saved usage (musl's posix_spawn `child()`
/// keeps `errno`-pointer-derived values, argv, etc. in r12-r15/rbx/rbp across the
/// call) depends on the rest. Without restoring all of these, the child either
/// jumps to garbage or dereferences NULL/stale pointers built from zeroed regs.
#[unsafe(naked)]
pub unsafe extern "C" fn fork_enter_trampoline() {
    core::arch::naked_asm!(
        "xor eax, eax",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop r10",
        "pop r8",
        "pop r9",
        "pop rbx",
        "pop rbp",
        "pop r12",
        "pop r13",
        "pop r14",
        "pop r15",
        "iretq",
    );
}

/// Build a kernel stack for a new userspace task.
///
/// When `context_switch` resumes this task it pops 6 saved registers, then
/// `ret`s into `user_enter_trampoline`, which executes `iretq` using the frame
/// that was placed immediately after the saved-register slots.
///
/// Stack layout from `kernel_sp` upward (12 × 8 = 96 bytes):
///   [0]  r15 = 0
///   [1]  r14 = 0
///   [2]  r13 = 0
///   [3]  r12 = 0
///   [4]  rbp = 0
///   [5]  rbx = 0
///   [6]  &user_enter_trampoline   ← context_switch's `ret` target
///   [7]  user_rip                 ┐
///   [8]  USER_CODE_SEL (0x1B)     │
///   [9]  RFLAGS = 0x202           │ iretq frame consumed by the trampoline
///   [10] user_rsp                 │
///   [11] USER_DATA_SEL (0x23)     ┘
pub unsafe fn task_init_userspace_stack(stack: &mut [u8], user_rip: u64, user_rsp: u64) -> u64 {
    use super::gdt::{USER_CODE_SEL, USER_DATA_SEL};
    let top = stack.as_mut_ptr().add(stack.len()) as usize;
    let top = top & !0xF;
    let sp = (top - 12 * 8) as *mut u64;
    sp.add(0).write(0); // r15
    sp.add(1).write(0); // r14
    sp.add(2).write(0); // r13
    sp.add(3).write(0); // r12
    sp.add(4).write(0); // rbp
    sp.add(5).write(0); // rbx
    sp.add(6).write(user_enter_trampoline as u64);
    sp.add(7).write(user_rip);
    sp.add(8).write(USER_CODE_SEL as u64);
    sp.add(9).write(0x202); // RFLAGS: IF=1, reserved bit 1
    sp.add(10).write(user_rsp);
    sp.add(11).write(USER_DATA_SEL as u64);
    sp as u64
}

/// Same layout as `task_init_userspace_stack` but uses `fork_enter_trampoline`
/// so the child sees rax=0 when it enters userspace (fork returns 0 in child),
/// with rdi/rsi/rdx/r10/r8/r9 and rbx/rbp/r12-r15 restored to their values at
/// the clone()/fork() call (see `fork_enter_trampoline` for why this matters).
///
/// `callee_saved` is [rbx, rbp, r12, r13, r14, r15].
///
/// Stack layout from `kernel_sp` upward (24 × 8 = 192 bytes):
///   [0]  r15 = 0
///   [1]  r14 = 0
///   [2]  r13 = 0
///   [3]  r12 = 0
///   [4]  rbp = 0
///   [5]  rbx = 0                  ← these 6 are consumed by context_switch's own
///                                    pop sequence, unrelated to the real values below
///   [6]  &fork_enter_trampoline   ← context_switch's `ret` target
///   [7]  rdi (a0)                 ┐
///   [8]  rsi (a1)                 │
///   [9]  rdx (a2)                 │ popped by fork_enter_trampoline
///   [10] r10 (a3)                 │
///   [11] r8  (a4)                 │
///   [12] r9  (a5)                 │
///   [13] rbx (callee_saved[0])    │
///   [14] rbp (callee_saved[1])    │
///   [15] r12 (callee_saved[2])    │
///   [16] r13 (callee_saved[3])    │
///   [17] r14 (callee_saved[4])    │
///   [18] r15 (callee_saved[5])    ┘ before the iretq below
///   [19] user_rip                 ┐
///   [20] USER_CODE_SEL (0x1B)     │
///   [21] RFLAGS = 0x202           │ iretq frame
///   [22] user_rsp                 │
///   [23] USER_DATA_SEL (0x23)     ┘
#[allow(clippy::too_many_arguments)]
pub unsafe fn task_init_fork_stack(
    stack: &mut [u8], user_rip: u64, user_rsp: u64,
    a0: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64,
    callee_saved: [u64; 6],
) -> u64 {
    use super::gdt::{USER_CODE_SEL, USER_DATA_SEL};
    let top = stack.as_mut_ptr().add(stack.len()) as usize;
    let top = top & !0xF;
    let sp = (top - 24 * 8) as *mut u64;
    sp.add(0).write(0); // r15
    sp.add(1).write(0); // r14
    sp.add(2).write(0); // r13
    sp.add(3).write(0); // r12
    sp.add(4).write(0); // rbp
    sp.add(5).write(0); // rbx
    sp.add(6).write(fork_enter_trampoline as u64);
    sp.add(7).write(a0);
    sp.add(8).write(a1);
    sp.add(9).write(a2);
    sp.add(10).write(a3);
    sp.add(11).write(a4);
    sp.add(12).write(a5);
    sp.add(13).write(callee_saved[0]);
    sp.add(14).write(callee_saved[1]);
    sp.add(15).write(callee_saved[2]);
    sp.add(16).write(callee_saved[3]);
    sp.add(17).write(callee_saved[4]);
    sp.add(18).write(callee_saved[5]);
    sp.add(19).write(user_rip);
    sp.add(20).write(USER_CODE_SEL as u64);
    sp.add(21).write(0x202);
    sp.add(22).write(user_rsp);
    sp.add(23).write(USER_DATA_SEL as u64);
    sp as u64
}

/// Initialise a new task's kernel stack so `context_switch` can resume it at `entry`.
/// Returns the initial `kernel_sp` value to store in the `Task`.
///
/// # Safety
/// `stack` must be a valid, exclusively-owned byte buffer.
pub unsafe fn task_init_stack(stack: &mut [u8], entry: fn() -> !) -> u64 {
    let top = stack.as_mut_ptr().add(stack.len()) as usize;
    let top = top & !0xF; // 16-byte align down
    // 8 slots × 8 bytes = 64 bytes: [r15, r14, r13, r12, rbp, rbx, entry_fn, _pad]
    // After 6 pops + ret, RSP = top - 8, satisfying the SysV ABI requirement
    // that RSP % 16 == 8 at function entry (as if entered via a `call`).
    let sp = (top - 8 * 8) as *mut u64;
    for i in 0..6usize {
        sp.add(i).write(0);
    }
    sp.add(6).write(entry as u64);
    sp.add(7).write(0); // padding slot never executed
    sp as u64
}
