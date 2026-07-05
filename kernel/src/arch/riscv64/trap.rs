/// RISC-V S-mode trap handler.
/// Sets stvec to our trap entry and dispatches exceptions/interrupts.

#[repr(C)]
pub struct TrapFrame {
    pub ra:  u64, pub sp:  u64, pub gp:  u64, pub tp:  u64,
    pub t0:  u64, pub t1:  u64, pub t2:  u64, pub s0:  u64,
    pub s1:  u64, pub a0:  u64, pub a1:  u64, pub a2:  u64,
    pub a3:  u64, pub a4:  u64, pub a5:  u64, pub a6:  u64,
    pub a7:  u64, pub s2:  u64, pub s3:  u64, pub s4:  u64,
    pub s5:  u64, pub s6:  u64, pub s7:  u64, pub s8:  u64,
    pub s9:  u64, pub s10: u64, pub s11: u64, pub t3:  u64,
    pub t4:  u64, pub t5:  u64, pub t6:  u64,
    pub sepc:    u64,  // [31]
    pub sstatus: u64,  // [32] saved at entry; restored before sret so SPP survives context switches
}

// Interrupt cause bits from scause
const INTERRUPT_BIT: u64 = 1 << 63;
const CAUSE_MASK:    u64 = !(1 << 63);

const CAUSE_TIMER:   u64 = 5;  // Supervisor timer interrupt
const CAUSE_EXT:     u64 = 9;  // Supervisor external interrupt
const CAUSE_SYSCALL: u64 = 8;  // Environment call from U-mode (user syscall)
const CAUSE_S_ECALL: u64 = 9;  // Environment call from S-mode (should not happen in our kernel)

pub fn init() {
    unsafe {
        let handler = trap_entry as usize;
        // Direct mode (bit 1:0 = 00): all traps go to trap_entry
        core::arch::asm!("csrw stvec, {}", in(reg) handler, options(nostack));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trap_handler(frame: &mut TrapFrame) {
    let scause: u64;
    unsafe { core::arch::asm!("csrr {}, scause", out(reg) scause, options(nostack)) };

    let is_interrupt = (scause & INTERRUPT_BIT) != 0;
    let cause = scause & CAUSE_MASK;

    if is_interrupt {
        match cause {
            CAUSE_TIMER => {
                crate::scheduler::tick();
                // Re-arm timer: add 100_000 cycles
                let time: u64;
                unsafe { core::arch::asm!("csrr {}, time", out(reg) time, options(nostack)) };
                super::sbi::set_timer(time + 100_000);
                // Clear stip by clearing sie.STIE momentarily handled by OpenSBI
            }
            CAUSE_EXT => {
                super::plic::handle();
            }
            _ => {}
        }
    } else {
        match cause {
            CAUSE_SYSCALL => {
                crate::syscall::set_user_ctx(frame.sepc + 4, frame.sp);
                let ret = crate::syscall::dispatch(
                    frame.a7 as usize,
                    frame.a0 as usize,
                    frame.a1 as usize,
                    frame.a2 as usize,
                    frame.a3 as usize,
                    frame.a4 as usize,
                    frame.a5 as usize,
                );
                frame.a0 = ret as u64;
                // execve replaces the address space and redirects the return PC/SP
                if let Some((new_ip, new_sp, new_pt)) = crate::syscall::take_exec_ctx() {
                    frame.sepc = new_ip;
                    frame.sp   = new_sp;
                    crate::arch::switch_address_space(new_pt);
                } else {
                    frame.sepc += 4;
                }
            }
            CAUSE_S_ECALL => {
                // S-mode ecall: should be handled by OpenSBI, not us. Skip it.
                frame.sepc += 4;
            }
            // ── page faults: 12=instruction, 13=load, 15=store/AMO ─────────
            12 | 13 | 15 => {
                let stval: u64;
                unsafe { core::arch::asm!("csrr {}, stval", out(reg) stval, options(nostack)) };
                // sstatus.SPP (bit 8): 0 = fault from U-mode, 1 = from S-mode
                let from_user = (frame.sstatus >> 8) & 1 == 0;
                if from_user && crate::scheduler::handle_user_page_fault(stval) {
                    // Mapped successfully, sepc unchanged, faulting instruction retries
                } else if from_user {
                    // Unresolvable user fault (invalid address or OOM) → SIGSEGV
                    crate::scheduler::exit_current(139); // 128 + SIGSEGV(11)
                    loop { unsafe { core::arch::asm!("wfi", options(nostack)) } }
                } else {
                    // Kernel page fault, panic
                    super::console::print_str("[TRAP] kernel page fault sepc=0x");
                    let val = frame.sepc;
                    let mut buf = [b'0'; 16];
                    for j in 0..16 {
                        let nibble = ((val >> (60 - j * 4)) & 0xF) as u8;
                        buf[j] = if nibble < 10 { b'0' + nibble } else { b'a' + nibble - 10 };
                    }
                    if let Ok(s) = core::str::from_utf8(&buf) { super::console::print_str(s); }
                    super::console::print_str(" stval=0x");
                    let mut buf2 = [b'0'; 16];
                    for j in 0..16 {
                        let nibble = ((stval >> (60 - j * 4)) & 0xF) as u8;
                        buf2[j] = if nibble < 10 { b'0' + nibble } else { b'a' + nibble - 10 };
                    }
                    if let Ok(s) = core::str::from_utf8(&buf2) { super::console::print_str(s); }
                    super::console::print_str("\n");
                    loop { unsafe { core::arch::asm!("wfi", options(nostack)) } }
                }
            }
        _ => {
                super::console::print_str("[TRAP] unhandled exception cause=");
                let scause_val = scause & CAUSE_MASK;
                let sepc_val: u64;
                unsafe { core::arch::asm!("csrr {}, sepc", out(reg) sepc_val, options(nostack)) };
                let stval: u64;
                unsafe { core::arch::asm!("csrr {}, stval", out(reg) stval, options(nostack)) };
                let print_hex = |label: &str, val: u64| {
                    super::console::print_str(label);
                    let mut buf = [b'0'; 16];
                    for j in 0..16 {
                        let nibble = ((val >> (60 - j * 4)) & 0xF) as u8;
                        buf[j] = if nibble < 10 { b'0' + nibble } else { b'a' + nibble - 10 };
                    }
                    if let Ok(s) = core::str::from_utf8(&buf) { super::console::print_str(s); }
                };
                let mut n = scause_val;
                let mut buf = [b'0'; 20];
                let mut i = 20usize;
                if n == 0 { i -= 1; buf[i] = b'0'; }
                while n > 0 { i -= 1; buf[i] = b'0' + (n % 10) as u8; n /= 10; }
                if let Ok(s) = core::str::from_utf8(&buf[i..]) { super::console::print_str(s); }
                super::console::print_str(" sepc=0x");
                print_hex("", sepc_val);
                super::console::print_str(" stval=0x");
                print_hex("", stval);
                super::console::print_str("\n");
                loop { unsafe { core::arch::asm!("wfi", options(nostack)) } }
            }
        }
    }
}

/// Assembly trap entry handling both S-mode and U-mode traps.
///
/// Invariant maintained by the scheduler and trap_entry itself:
///   sscratch = 0           while executing in S-mode (kernel)
///   sscratch = kstack_top  while executing in U-mode (userspace task)
///
/// On entry: if we came from U-mode, `sp` is the user stack (unusable).
///           The csrrw swap lets us detect this and switch to the kernel stack.
#[unsafe(naked)]
#[unsafe(no_mangle)]
unsafe extern "C" fn trap_entry() {
    core::arch::naked_asm!(
        // ── detect privilege origin ────────────────────────────────────────
        // Swap sp ↔ sscratch atomically.
        // From U-mode: sscratch held kstack_top → sp = kstack_top, sscratch = user_sp
        // From S-mode: sscratch held 0          → sp = 0,          sscratch = kernel_sp
        "csrrw sp, sscratch, sp",
        "bnez  sp, 1f",               // sp != 0 → came from U-mode

        // ── S-mode path ────────────────────────────────────────────────────
        "csrrw sp, sscratch, sp",     // undo: sp = kernel sp, sscratch = 0
        "addi  sp, sp, -{frame_size}",
        "sd    t0,  4*8(sp)",
        "addi  t0,  sp, {frame_size}",// t0 = original kernel sp
        "sd    ra,  0*8(sp)",
        "sd    t0,  1*8(sp)",         // saved sp (kernel)
        "sd    gp,  2*8(sp)",
        "sd    tp,  3*8(sp)",
        // t0 already saved at [4]
        "j     2f",

        // ── U-mode path ────────────────────────────────────────────────────
        // sp = kstack_top, sscratch = user_sp, t0 = user t0 (still in t0)
        "1:",
        "addi  sp, sp, -{frame_size}",
        "sd    t0,  4*8(sp)",         // save user t0 before clobbering t0
        "csrrw t0, sscratch, zero",   // t0 = user_sp; sscratch = 0 (now in S-mode)
        "sd    ra,  0*8(sp)",
        "sd    t0,  1*8(sp)",         // saved sp (user)
        "sd    gp,  2*8(sp)",
        "sd    tp,  3*8(sp)",
        // t0 at [4] already saved

        // ── common register save ───────────────────────────────────────────
        "2:",
        "sd    t1,  5*8(sp)",  "sd  t2,  6*8(sp)",  "sd  s0,  7*8(sp)",
        "sd    s1,  8*8(sp)",  "sd  a0,  9*8(sp)",  "sd  a1, 10*8(sp)",
        "sd    a2, 11*8(sp)",  "sd  a3, 12*8(sp)",  "sd  a4, 13*8(sp)",
        "sd    a5, 14*8(sp)",  "sd  a6, 15*8(sp)",  "sd  a7, 16*8(sp)",
        "sd    s2, 17*8(sp)",  "sd  s3, 18*8(sp)",  "sd  s4, 19*8(sp)",
        "sd    s5, 20*8(sp)",  "sd  s6, 21*8(sp)",  "sd  s7, 22*8(sp)",
        "sd    s8, 23*8(sp)",  "sd  s9, 24*8(sp)",  "sd s10, 25*8(sp)",
        "sd   s11, 26*8(sp)",  "sd  t3, 27*8(sp)",  "sd  t4, 28*8(sp)",
        "sd    t5, 29*8(sp)",  "sd  t6, 30*8(sp)",
        "csrr  t0, sepc",
        "sd    t0, 31*8(sp)",
        "csrr  t0, sstatus",          // save sstatus.SPP so context-switches can't corrupt it
        "sd    t0, 32*8(sp)",
        "mv    a0, sp",
        "call  trap_handler",

        // ── restore sepc and sstatus from frame ───────────────────────────
        "ld    t0, 31*8(sp)",
        "csrw  sepc, t0",
        // Restore saved sstatus (including SPP) so sret returns to the privilege
        // level that triggered THIS trap, even after a context-switch.
        "ld    t0, 32*8(sp)",
        "csrw  sstatus, t0",

        // ── check return privilege (sstatus.SPP, bit 8) ───────────────────
        // t0 still holds saved sstatus; isolate SPP without re-reading the CSR.
        "andi  t0, t0, 256",          // SPP=0 → U-mode, SPP=1 → S-mode
        "bnez  t0, 3f",               // SPP=1 → returning to S-mode, skip
        // Returning to U-mode: sscratch = kernel stack top for next trap
        "addi  t0, sp, {frame_size}",
        "csrw  sscratch, t0",

        // ── common register restore ───────────────────────────────────────
        "3:",
        "ld    ra,  0*8(sp)",
        // sp restored last from frame[1] (may be user sp or kernel sp)
        "ld    gp,  2*8(sp)",  "ld  tp,  3*8(sp)",  "ld  t0,  4*8(sp)",
        "ld    t1,  5*8(sp)",  "ld  t2,  6*8(sp)",  "ld  s0,  7*8(sp)",
        "ld    s1,  8*8(sp)",  "ld  a0,  9*8(sp)",  "ld  a1, 10*8(sp)",
        "ld    a2, 11*8(sp)",  "ld  a3, 12*8(sp)",  "ld  a4, 13*8(sp)",
        "ld    a5, 14*8(sp)",  "ld  a6, 15*8(sp)",  "ld  a7, 16*8(sp)",
        "ld    s2, 17*8(sp)",  "ld  s3, 18*8(sp)",  "ld  s4, 19*8(sp)",
        "ld    s5, 20*8(sp)",  "ld  s6, 21*8(sp)",  "ld  s7, 22*8(sp)",
        "ld    s8, 23*8(sp)",  "ld  s9, 24*8(sp)",  "ld s10, 25*8(sp)",
        "ld   s11, 26*8(sp)",  "ld  t3, 27*8(sp)",  "ld  t4, 28*8(sp)",
        "ld    t5, 29*8(sp)",  "ld  t6, 30*8(sp)",
        "ld    sp,  1*8(sp)",  // restore sp (user or kernel)
        "sret",
        frame_size = const core::mem::size_of::<TrapFrame>(),
    );
}
