/// RISC-V cooperative context switch (callee-saved: ra + s0-s11).
///
/// Calling convention: RISC-V LP64 ABI
///   a0 = cur_sp  (*mut u64 receives current SP after saving)
///   a1 = next_sp (u64     SP to restore for next task)
///
/// Stack layout when a task is suspended (from SP upward):
///   [+0]   ra    <- ret jumps here on resume
///   [+8]   s0
///   [+16]  s1
///   [+24]  s2
///   [+32]  s3
///   [+40]  s4
///   [+48]  s5
///   [+56]  s6
///   [+64]  s7
///   [+72]  s8
///   [+80]  s9
///   [+88]  s10
///   [+96]  s11

#[unsafe(naked)]
pub unsafe extern "C" fn context_switch(cur_sp: *mut u64, next_sp: u64) {
    core::arch::naked_asm!(
        // Save callee-saved regs + ra onto current stack
        "addi sp, sp, -104",
        "sd ra,    0*8(sp)",
        "sd s0,    1*8(sp)",
        "sd s1,    2*8(sp)",
        "sd s2,    3*8(sp)",
        "sd s3,    4*8(sp)",
        "sd s4,    5*8(sp)",
        "sd s5,    6*8(sp)",
        "sd s6,    7*8(sp)",
        "sd s7,    8*8(sp)",
        "sd s8,    9*8(sp)",
        "sd s9,   10*8(sp)",
        "sd s10,  11*8(sp)",
        "sd s11,  12*8(sp)",
        // *cur_sp = sp
        "sd sp, 0(a0)",
        // sp = next_sp
        "mv sp, a1",
        // Restore callee-saved regs + ra from next stack
        "ld ra,    0*8(sp)",
        "ld s0,    1*8(sp)",
        "ld s1,    2*8(sp)",
        "ld s2,    3*8(sp)",
        "ld s3,    4*8(sp)",
        "ld s4,    5*8(sp)",
        "ld s5,    6*8(sp)",
        "ld s6,    7*8(sp)",
        "ld s7,    8*8(sp)",
        "ld s8,    9*8(sp)",
        "ld s9,   10*8(sp)",
        "ld s10,  11*8(sp)",
        "ld s11,  12*8(sp)",
        "addi sp, sp, 104",
        "ret",
    )
}

/// Initialise a new task's kernel stack so `context_switch` can resume it at `entry`.
/// Returns the initial `kernel_sp` to store in the task struct.
///
/// # Safety
/// `stack` must be a valid, exclusively-owned byte buffer with at least 104 bytes.
pub unsafe fn task_init_stack(stack: &mut [u8], entry: fn() -> !) -> u64 {
    let top = stack.as_mut_ptr().add(stack.len()) as usize;
    let top = top & !0xF; // 16-byte align down
    // 13 slots × 8 bytes = 104 bytes: [ra, s0-s11]
    let sp = (top - 13 * 8) as *mut u64;
    sp.add(0).write(entry as u64); // ra = entry function
    for i in 1..=12usize {
        sp.add(i).write(0); // s0-s11 = 0
    }
    sp as u64
}

/// Store the kernel stack pointer in sscratch for future user→kernel trap handling.
/// Pass 0 for kernel tasks (sscratch=0 signals S-mode origin in trap_entry).
pub fn set_kernel_stack(sp: u64) {
    unsafe {
        core::arch::asm!("csrw sscratch, {}", in(reg) sp, options(nostack));
    }
}

/// Trampoline entered via `ret` from context_switch when a user task is first
/// scheduled.  At this point context_switch has restored:
///   s0 = user stack pointer
///   s1 = user entry point (sepc)
///   s2 = target sstatus (SPIE=1, SPP=0 → sret enters U-mode with interrupts on)
/// RSP/sp is the kernel stack top (above the context frame, unused from here on).
#[unsafe(naked)]
pub unsafe extern "C" fn user_enter_trampoline() -> ! {
    core::arch::naked_asm!(
        "csrw sepc,    s1",  // return address for sret = user entry point
        "csrw sstatus, s2",  // set SPP=0 (U-mode), SPIE=1
        "mv   sp,      s0",  // switch to user stack
        "sret",              // enter U-mode
    )
}

/// Build a kernel stack for a new userspace task so `context_switch` falls
/// through into `user_enter_trampoline` and then `sret`s to U-mode.
///
/// context_switch restores 13 slots ([ra, s0..s11]); after `addi sp,sp,104`
/// and `ret` to ra (= trampoline):
///   s0  = user_rsp  → trampoline does `mv sp, s0`
///   s1  = user_rip  → trampoline does `csrw sepc, s1`
///   s2  = sstatus   → trampoline does `csrw sstatus, s2`  (SPIE=1, SPP=0)
///   s3-s11 = 0
pub unsafe fn task_init_userspace_stack(stack: &mut [u8], user_rip: u64, user_rsp: u64) -> u64 {
    let top = stack.as_mut_ptr().add(stack.len()) as usize;
    let top = top & !0xF;
    let sp = (top - 13 * 8) as *mut u64;
    sp.add(0).write(user_enter_trampoline as u64); // ra
    sp.add(1).write(user_rsp);                      // s0
    sp.add(2).write(user_rip);                      // s1
    sp.add(3).write((1 << 5) | (1 << 18));          // s2: SPIE=1, SPP=0, SUM=1
    for i in 4..=12usize { sp.add(i).write(0); }   // s3-s11
    sp as u64
}
