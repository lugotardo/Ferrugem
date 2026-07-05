use super::pic;
use super::console;

/// 64-bit IDT gate descriptor
#[derive(Clone, Copy)]
#[repr(C, packed)]
struct IdtEntry {
    offset_low:  u16,
    selector:    u16,
    ist:         u8,
    type_attr:   u8,
    offset_mid:  u16,
    offset_high: u32,
    _reserved:   u32,
}

impl IdtEntry {
    const fn missing() -> Self {
        Self {
            offset_low: 0, selector: 0, ist: 0,
            type_attr: 0, offset_mid: 0, offset_high: 0, _reserved: 0,
        }
    }

    fn set(&mut self, handler: u64, sel: u16, ist: u8, attr: u8) {
        self.offset_low  = (handler & 0xFFFF) as u16;
        self.offset_mid  = ((handler >> 16) & 0xFFFF) as u16;
        self.offset_high = (handler >> 32) as u32;
        self.selector    = sel;
        self.ist         = ist;
        self.type_attr   = attr;
        self._reserved   = 0;
    }
}

#[repr(C, packed)]
struct IdtPtr {
    limit: u16,
    base:  u64,
}

static mut IDT: [IdtEntry; 256] = [IdtEntry::missing(); 256];

// ── Per-vector stubs ────────────────────────────────────────────────────────
// Each stub saves all caller-saved registers, calls common_dispatch with the
// vector number, restores, and issues iretq.  Must be naked so the compiler
// does not generate any prologue/epilogue that would corrupt the iret frame.

macro_rules! make_isr {
    ($name:ident, $vec:expr) => {
        #[unsafe(naked)]
        #[unsafe(no_mangle)]
        unsafe extern "C" fn $name() {
            core::arch::naked_asm!(
                "push rax", "push rbx", "push rcx", "push rdx",
                "push rsi", "push rdi", "push rbp",
                "push r8",  "push r9",  "push r10", "push r11",
                "push r12", "push r13", "push r14", "push r15",
                "mov edi, {vec}",
                "call {dispatch}",
                "pop r15", "pop r14", "pop r13", "pop r12",
                "pop r11", "pop r10", "pop r9",  "pop r8",
                "pop rbp", "pop rdi", "pop rsi", "pop rdx",
                "pop rcx", "pop rbx", "pop rax",
                "iretq",
                vec = const $vec,
                dispatch = sym common_dispatch,
            );
        }
    };
}

// CPU exceptions 0-31
make_isr!(isr0,  0);   make_isr!(isr1,  1);   make_isr!(isr2,  2);
make_isr!(isr3,  3);   make_isr!(isr4,  4);   make_isr!(isr5,  5);
make_isr!(isr6,  6);   make_isr!(isr7,  7);   make_isr!(isr8,  8);
make_isr!(isr9,  9);   make_isr!(isr10, 10);  make_isr!(isr11, 11);
make_isr!(isr12, 12);
// isr13 = custom #GP handler; isr14 = custom #PF handler below
make_isr!(isr15, 15);  make_isr!(isr16, 16);  make_isr!(isr17, 17);
make_isr!(isr18, 18);  make_isr!(isr19, 19);  make_isr!(isr20, 20);
make_isr!(isr21, 21);  make_isr!(isr22, 22);  make_isr!(isr23, 23);
make_isr!(isr24, 24);  make_isr!(isr25, 25);  make_isr!(isr26, 26);
make_isr!(isr27, 27);  make_isr!(isr28, 28);  make_isr!(isr29, 29);
make_isr!(isr30, 30);  make_isr!(isr31, 31);

// PIC IRQs 0x20-0x2F
make_isr!(isr32, 32);  make_isr!(isr33, 33);  make_isr!(isr34, 34);
make_isr!(isr35, 35);  make_isr!(isr36, 36);  make_isr!(isr37, 37);
make_isr!(isr38, 38);  make_isr!(isr39, 39);  make_isr!(isr40, 40);
make_isr!(isr41, 41);  make_isr!(isr42, 42);  make_isr!(isr43, 43);
make_isr!(isr44, 44);  make_isr!(isr45, 45);  make_isr!(isr46, 46);
make_isr!(isr47, 47);

// Generic fallback for vectors 48-255
make_isr!(isr_ignore, 0xFF);

// ── INT 0x80 software syscall entry ────────────────────────────────────────
// Linux x86_64 INT 0x80 ABI: rax=nr, rdi=a0, rsi=a1, rdx=a2
// We rearrange into SysV calling convention before calling syscall_entry_rust.

// Called by isr128 if sys_execve redirected the user return address.
// `stack` points to the base of the saved-register area (S11 = rsp after 11 pushes).
// iretq frame layout from S11:  [+88]=rip  [+96]=cs  [+104]=rflags  [+112]=rsp  [+120]=ss
#[unsafe(no_mangle)]
unsafe extern "C" fn apply_exec_redirect(stack: *mut u64) {
    if let Some((new_ip, new_sp, new_pt)) = crate::syscall::take_exec_ctx() {
        *stack.add(11) = new_ip; // [S11+88]  = user_rip
        *stack.add(14) = new_sp; // [S11+112] = user_rsp
        crate::arch::switch_address_space(new_pt);
    }
}

// syscall_entry_rust(nr, a0, a1, a2, a3, a4, a5, user_ip, user_sp)
// SysV register args: rdi rsi rdx rcx r8 r9 , 6 regs max
// Stack args (at [rsp+8], [rsp+16], [rsp+24]): a5, user_ip, user_sp
#[unsafe(no_mangle)]
unsafe extern "C" fn syscall_entry_rust(
    nr: usize, a0: usize, a1: usize, a2: usize,
    a3: usize, a4: usize,
    // stack-passed: a5 at [rsp+8], user_ip at [rsp+16], user_sp at [rsp+24]
    a5: usize, user_ip: u64, user_sp: u64,
) -> isize {
    crate::syscall::set_user_ctx(user_ip, user_sp);
    crate::syscall::dispatch(nr, a0, a1, a2, a3, a4, a5)
}

// INT 0x80 syscall entry, Linux x86_64 ABI: rax=nr rdi=a0 rsi=a1 rdx=a2 r10=a3 r8=a4 r9=a5
//
// Stack layout after 11 pushes (S11 = rsp, S11 mod 16 = 0):
//   [S11+ 0]=rcx  [+8]=r9(a5)  [+16]=r8(a4)  [+24]=r11  [+32]=r10(a3)
//   [+40]=r15  [+48]=r14  [+56]=r13  [+64]=r12  [+72]=rbp  [+80]=rbx
//   [+88]=user_rip  [+96]=cs  [+104]=rflags  [+112]=user_rsp  [+120]=ss
//
// We call:  syscall_entry_rust(nr, a0, a1, a2, a3, a4, a5, user_ip, user_rsp)
// SysV register args: rdi rsi rdx rcx r8 r9
// Stack args (7th..9th): [rsp+8]=a5  [rsp+16]=user_ip  [rsp+24]=user_rsp
//
// Stack setup (sub 8 FIRST for alignment, then 3 pushes = 32 bytes total):
//   sub rsp,8    (alignment) , rsp = S11-8
//   push user_rsp (9th arg) , rsp = S11-16; load from [rsp+120]=[S11+112]
//   push user_rip (8th arg) , rsp = S11-24; load from [rsp+104]=[S11+88]
//   push r9/a5   (7th arg)  , rsp = S11-32 (mod 16 = 0) ✓
//
// After `call` (ret-addr pushed → rsp=S11-40):
//   [rsp+8]=a5  [rsp+16]=user_rip  [rsp+24]=user_rsp  , 7th, 8th, 9th args ✓
#[unsafe(naked)]
#[unsafe(no_mangle)]
unsafe extern "C" fn isr128() {
    core::arch::naked_asm!(
        "push rbx", "push rbp", "push r12", "push r13", "push r14", "push r15",
        "push r10", "push r11", "push r8",  "push r9",  "push rcx",
        // rsp = S11 (mod 16 = 0); registers still hold user values except r11

        // Save nr (rax) in r11; r11's original value is saved at [S11+24]
        "mov r11, rax",

        // Align first, then push 3 stack args (9th, 8th, 7th)
        "sub rsp, 8",                       // rsp = S11-8, alignment pad
        "mov rax, qword ptr [rsp + 120]", // rax = user_rsp ([S11-8+120]=[S11+112])
        "push rax",                         // rsp = S11-16, 9th arg
        "mov rax, qword ptr [rsp + 104]", // rax = user_rip ([S11-16+104]=[S11+88])
        "push rax",                         // rsp = S11-24, 8th arg
        "push r9",                          // rsp = S11-32, 7th arg (a5 = user r9)
        // (S11-32) mod 16 = 0 ✓ → aligned for call

        // Set up 6 register args: rdi=nr rsi=a0 rdx=a1 rcx=a2 r8=a3 r9=a4
        // r8=a4 (user r8) and r10=a3 (user r10) are still valid in registers.
        "mov r9, r8",     // r9 = a4 (user r8); r9 was a5 but already pushed
        "mov r8, r10",    // r8 = a3 (user r10)
        "mov rcx, rdx",
        "mov rdx, rsi",
        "mov rsi, rdi",
        "mov rdi, r11",   // nr
        "call syscall_entry_rust",

        // Unwind: sub 8 + 3 pushes = 32 bytes; rax = syscall return value
        "add rsp, 32",                     // rsp = S11

        // Apply exec redirect if execve was called (preserves rax)
        "push rax",                        // rsp = S11-8
        "lea rdi, [rsp + 8]",             // rdi = S11 (iretq frame base, user_rip at +88)
        "sub rsp, 8",                      // align: (S11-8) mod 16 = 8 → 0 after sub
        "call apply_exec_redirect",
        "add rsp, 8",
        "pop rax",                         // rsp = S11

        // Restore saved registers and return to user
        "pop rcx", "pop r9",  "pop r8",  "pop r11", "pop r10",
        "pop r15", "pop r14", "pop r13", "pop r12", "pop rbp", "pop rbx",
        "iretq",
    )
}

// ── SYSCALL / SYSRETQ entry ───────────────────────────────────────────────────
//
// musl x86_64 uses the `syscall` instruction, not INT 0x80. SYSCALL does NOT
// switch stacks, so we save the user RSP to a global and load the kernel RSP.
//
// SYSCALL entry state: rcx=user_rip, r11=user_rflags, rsp=user_rsp (unchanged!)
//   rax=nr, rdi=a0, rsi=a1, rdx=a2, r10=a3, r8=a4, r9=a5

/// Kernel RSP for the SYSCALL handler. Updated by gdt::set_rsp0() on every task switch.
#[unsafe(no_mangle)]
pub(crate) static mut SYSCALL_KERNEL_RSP: u64 = 0;
/// Scratch area to save the user RSP on SYSCALL entry.
#[unsafe(no_mangle)]
static mut SYSCALL_USER_RSP: u64 = 0;

/// SysV callee-saved registers, snapshotted (not saved/restored, they're never
/// clobbered by our own asm, so the physical registers stay correct for the
/// parent's own sysretq) on every syscall entry so fork()/clone() can hand them
/// to the child. A real clone() duplicates the *entire* register file except
/// rax; without this, compiled code using rbx/rbp/r12-r15 across the fork call
/// (as musl's posix_spawn child() does) sees zeroed/garbage values in the child.
#[unsafe(no_mangle)]
static mut SYSCALL_USER_RBX: u64 = 0;
#[unsafe(no_mangle)]
static mut SYSCALL_USER_RBP: u64 = 0;
#[unsafe(no_mangle)]
static mut SYSCALL_USER_R12: u64 = 0;
#[unsafe(no_mangle)]
static mut SYSCALL_USER_R13: u64 = 0;
#[unsafe(no_mangle)]
static mut SYSCALL_USER_R14: u64 = 0;
#[unsafe(no_mangle)]
static mut SYSCALL_USER_R15: u64 = 0;

pub(crate) fn callee_saved_snapshot() -> [u64; 6] {
    unsafe { [SYSCALL_USER_RBX, SYSCALL_USER_RBP, SYSCALL_USER_R12, SYSCALL_USER_R13, SYSCALL_USER_R14, SYSCALL_USER_R15] }
}

/// Handle execve redirect for the SYSCALL (SYSRETQ) return path.
/// Updates the saved user_rip slot and switches address space.
#[unsafe(no_mangle)]
unsafe extern "C" fn apply_exec_redirect_syscall(saved_rcx: *mut u64) {
    if let Some((new_ip, new_sp, new_pt)) = crate::syscall::take_exec_ctx() {
        *saved_rcx = new_ip;
        SYSCALL_USER_RSP = new_sp;
        crate::arch::switch_address_space(new_pt);
    }
}

/// SYSCALL entry, 64-bit Linux ABI.
///
/// Linux syscall ABI: rcx=user_rip, r11=user_rflags are destroyed by SYSCALL
/// hardware. ALL other registers must be preserved by the kernel (including
/// rdi/rsi/rdx/r10/r8/r9 which we use as argument registers for the Rust handler).
///
/// Stack frame layout (K = SYSCALL_KERNEL_RSP, 16-byte aligned):
///   [K- 8] = user_rip    ← rcx  (restored to rcx for sysretq)
///   [K-16] = user_rflags ← r11  (restored to r11 for sysretq)
///   [K-24] = user's rdi  ← a0   (restored after call)
///   [K-32] = user's rsi  ← a1   (restored after call)
///   [K-40] = user's rdx  ← a2   (restored after call)
///   [K-48] = user's r8   ← a4   (restored after call)
///   [K-56] = user's r9   ← a5   (restored after call)
///   [K-64] = user's r10  ← a3   (restored after call)
///   [K-72] = padding     ← alignment pad (sub rsp, 8)
///   [K-80] = user_sp     ← 9th arg to syscall_entry_rust
///   [K-88] = user_ip     ← 8th arg
///   [K-96] = a5          ← 7th arg; rsp=K-96 at call (16-aligned) ✓
///
/// 8 saves (64B) + 1 pad (8B) + 3 stack args (24B) = 96B → K-96 mod 16 = 0 ✓
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn syscall_entry_asm() {
    core::arch::naked_asm!(
        // Save user RSP; switch to kernel RSP
        "mov qword ptr [rip + {u_rsp}], rsp",
        "mov rsp, qword ptr [rip + {k_rsp}]",
        // Snapshot callee-saved regs (untouched by the rest of this stub, this is a
        // read, not a save/restore) so fork()/clone() can hand them to the child.
        "mov qword ptr [rip + {sv_rbx}], rbx",
        "mov qword ptr [rip + {sv_rbp}], rbp",
        "mov qword ptr [rip + {sv_r12}], r12",
        "mov qword ptr [rip + {sv_r13}], r13",
        "mov qword ptr [rip + {sv_r14}], r14",
        "mov qword ptr [rip + {sv_r15}], r15",
        // Save all registers that the Linux syscall ABI requires us to preserve.
        // rcx/r11 are destroyed by SYSCALL hardware; the rest we clobber when
        // remapping arguments for the Rust handler.
        "push rcx",   // [K- 8] user_rip   (rcx set by hardware to next insn)
        "push r11",   // [K-16] user_rflags (r11 set by hardware to user RFLAGS)
        "push rdi",   // [K-24] user's rdi = a0
        "push rsi",   // [K-32] user's rsi = a1
        "push rdx",   // [K-40] user's rdx = a2
        "push r8",    // [K-48] user's r8  = a4
        "push r9",    // [K-56] user's r9  = a5
        "push r10",   // [K-64] user's r10 = a3 , rsp=K-64 (mod16=0)
        // Push 3 stack args for syscall_entry_rust (args 7..9).
        // SysV AMD64: before call, [rsp]=arg7, [rsp+8]=arg8, [rsp+16]=arg9.
        // Push in reverse (9th first); need rsp=K-96 (mod16=0) at call.
        "sub rsp, 8",                               // [K-72] pad; rsp=K-72 (mod16=8)
        "push qword ptr [rip + {u_rsp}]",           // [K-80] user_sp  (9th arg)
        "push rcx",                                  // [K-88] user_ip  (8th arg; rcx=user_rip ✓)
        "push r9",                                   // [K-96] a5       (7th arg; r9=user's r9 ✓)
        // rsp=K-96 (mod16=0), 16-aligned for call ✓
        // Remap to SysV register args for syscall_entry_rust:
        //   rdi=nr, rsi=a0, rdx=a1, rcx=a2, r8=a3, r9=a4
        "mov r9,  r8",   // r9  = a4 (user's r8)
        "mov r8,  r10",  // r8  = a3 (user's r10)
        "mov rcx, rdx",  // rcx = a2 (user's rdx)
        "mov rdx, rsi",  // rdx = a1 (user's rsi)
        "mov rsi, rdi",  // rsi = a0 (user's rdi)
        "mov rdi, rax",  // rdi = nr (syscall number from rax)
        "call {handler}",
        // Unwind: 1 pad + 3 args = 32 bytes; rsp → K-64
        "add rsp, 32",
        // Stack: [K-64]=r10 [K-56]=r9 [K-48]=r8 [K-40]=rdx [K-32]=rsi [K-24]=rdi [K-16]=r11 [K-8]=rcx
        // Call exec_redir with &saved_user_rip. rsp=K-64 (mod16=0); sub+push aligns.
        "sub rsp, 8",           // rsp=K-72 (mod16=8)
        "push rax",             // [K-80] save return value; rsp=K-80 (mod16=0) ✓
        "lea rdi, [rsp + 72]",  // K-80+72 = K-8 → &saved_user_rip ✓
        "call {exec_redir}",
        "pop rax",              // rsp=K-72; restore return value
        "add rsp, 8",           // rsp=K-64; undo alignment pad
        // Restore all saved registers in reverse push order
        "pop r10",   // rsp=K-56
        "pop r9",    // rsp=K-48
        "pop r8",    // rsp=K-40
        "pop rdx",   // rsp=K-32
        "pop rsi",   // rsp=K-24
        "pop rdi",   // rsp=K-16
        "pop r11",   // rsp=K-8;  r11=user_rflags
        "or r11, 0x200",         // force IF=1 (user mode always has interrupts enabled)
        "pop rcx",   // rsp=K;    rcx=user_rip (used by sysretq)
        // Restore user RSP and return to ring 3
        "mov rsp, qword ptr [rip + {u_rsp}]",
        "sysretq",
        handler    = sym syscall_entry_rust,
        exec_redir = sym apply_exec_redirect_syscall,
        u_rsp      = sym SYSCALL_USER_RSP,
        k_rsp      = sym SYSCALL_KERNEL_RSP,
        sv_rbx     = sym SYSCALL_USER_RBX,
        sv_rbp     = sym SYSCALL_USER_RBP,
        sv_r12     = sym SYSCALL_USER_R12,
        sv_r13     = sym SYSCALL_USER_R13,
        sv_r14     = sym SYSCALL_USER_R14,
        sv_r15     = sym SYSCALL_USER_R15,
    );
}

// ── #GP (vector 13), custom handler ─────────────────────────────────────────
//
// CPU pushes error_code, then RIP/CS/RFLAGS/RSP/SS.
// We read those before any register save so the Rust handler gets the full picture.
// If the fault came from user mode (CS & 3 == 3), kill the task; else halt.
#[unsafe(naked)]
#[unsafe(no_mangle)]
unsafe extern "C" fn isr13() {
    core::arch::naked_asm!(
        // [rsp+0]=error_code, [rsp+8]=RIP, [rsp+16]=CS, [rsp+24]=RFLAGS, [rsp+32]=RSP_user
        "push rax", "push rbx", "push rcx", "push rdx",
        "push rsi", "push rdi", "push rbp",
        "push r8", "push r9", "push r10", "push r11",
        "push r12", "push r13", "push r14", "push r15",
        "mov rdi, rsp",              // arg1 = &saved_gprs (top of the 15 pushes)
        "lea rsi, [rsp + 15*8]",     // arg2 = &error_code (right after the pushes)
        "call {handler}",
        "0: hlt",
        "jmp 0b",
        handler = sym gpf_handler,
    );
}

#[repr(C)]
struct SavedGprs {
    r15: u64, r14: u64, r13: u64, r12: u64,
    r11: u64, r10: u64, r9: u64, r8: u64,
    rbp: u64, rdi: u64, rsi: u64,
    rdx: u64, rcx: u64, rbx: u64, rax: u64,
}

extern "C" fn gpf_handler(gprs: *const SavedGprs, frame: *const u64) {
    let (error_code, rip, cs) = unsafe { (*frame, *frame.add(1), *frame.add(2)) };
    let g = unsafe { &*gprs };
    console::print_str("\n[#GP] err=0x");
    print_hex(error_code);
    console::print_str(" rip=0x");
    print_hex(rip);
    console::print_str(" cs=0x");
    print_hex(cs);
    console::print_str("\n  rax=0x"); print_hex(g.rax);
    console::print_str(" rbx=0x"); print_hex(g.rbx);
    console::print_str(" rcx=0x"); print_hex(g.rcx);
    console::print_str(" rdx=0x"); print_hex(g.rdx);
    console::print_str("\n  rsi=0x"); print_hex(g.rsi);
    console::print_str(" rdi=0x"); print_hex(g.rdi);
    console::print_str(" rbp=0x"); print_hex(g.rbp);
    console::print_str("\n  r8=0x");  print_hex(g.r8);
    console::print_str(" r9=0x");  print_hex(g.r9);
    console::print_str(" r10=0x"); print_hex(g.r10);
    console::print_str(" r11=0x"); print_hex(g.r11);
    console::print_str("\n  r12=0x"); print_hex(g.r12);
    console::print_str(" r13=0x"); print_hex(g.r13);
    console::print_str(" r14=0x"); print_hex(g.r14);
    console::print_str(" r15=0x"); print_hex(g.r15);
    console::print_str("\n");

    if cs & 3 == 3 {
        let user_rsp = unsafe { *frame.add(4) };
        console::print_str("  user_rsp=0x"); print_hex(user_rsp);
        console::print_str("\n");
        let p = user_rsp as *const u64;
        for i in 0..4i64 {
            console::print_str("  [rsp");
            if i >= 0 { console::print_str("+"); }
            print_hex((i * 8) as u64);
            console::print_str("]=0x");
            print_hex(unsafe { *p.offset(i as isize) });
            console::print_str("\n");
        }
        // Follow (%rsp) -> the suspected "fa"-like pointer, dump around it too.
        let fa = unsafe { *p };
        if fa != 0 {
            console::print_str("  fa=0x"); print_hex(fa); console::print_str("\n");
            let fap = fa as *const u64;
            for i in 0..4i64 {
                console::print_str("  fa[+");
                print_hex((i * 8) as u64);
                console::print_str("]=0x");
                print_hex(unsafe { *fap.offset(i as isize) });
                console::print_str("\n");
            }
        }
    }

    if cs & 3 == 3 {
        // User-mode fault: kill the process
        crate::scheduler::exit_current(139);
    }
    loop { unsafe { core::arch::asm!("hlt") } }
}

// ── #PF (vector 14), custom handler ─────────────────────────────────────────
//
// CPU pushes an error code for #PF, which the generic make_isr! macro does
// not handle (iretq would pop the error code as the return RIP and crash).
// This stub:
//   1. Reads CR2 (faulting VA) before any call could clobber it.
//   2. Pops the error code so RSP points directly at [RIP][CS][RFLAGS][RSP][SS].
//   3. Saves 13 GPRs, calls pagefault_handler_x86(va, error_code), restores.
//   4. iretq returns to the faulting instruction (retry) or to the next task.
//
// Stack on entry (from ring 3, RSP = A, A mod 16 = 0):
//   [+0]  error_code
//   [+8]  user_rip
//   [+16] CS
//   [+24] RFLAGS
//   [+32] user_rsp
//   [+40] SS
//
// After `pop rdx` (A+8):  RSP → user_rip  (aligned +8, correct for 13 pushes → aligned)
// 13 pushes (104 bytes): RSP = A+8-104 = A-96; (A-96) mod 16 = 0 → aligned before `call`.
#[unsafe(naked)]
#[unsafe(no_mangle)]
unsafe extern "C" fn isr14() {
    core::arch::naked_asm!(
        "mov  rcx, cr2",          // rcx = faulting VA (must read before any call)
        "pop  rdx",               // rdx = error_code; RSP → [user_rip …]
        "mov  r14, [rsp]",        // r14 = user_rip (peek before any pushes change RSP)
        "push rax",               // save all GPRs we use (13 pushes = 104 bytes)
        "push rbx",
        "push rsi",
        "push rdi",
        "push rbp",
        "push r8",  "push r9",  "push r10",
        "push r11", "push r12", "push r13", "push r14", "push r15",
        "mov  rdi, rcx",          // 1st arg: faulting VA
        "mov  rsi, rdx",          // 2nd arg: error_code
        "mov  rdx, r14",          // 3rd arg: user_rip
        "mov  rcx, rsp",          // 4th arg: &saved_gprs (top of the 13 pushes)
        "call {handler}",
        "pop  r15", "pop  r14", "pop  r13", "pop  r12",
        "pop  r11", "pop  r10", "pop  r9",  "pop  r8",
        "pop  rbp", "pop  rdi", "pop  rsi", "pop  rbx", "pop  rax",
        "iretq",
        handler = sym pagefault_handler_x86,
    )
}

#[repr(C)]
struct PfSavedGprs {
    r15: u64, r14: u64, r13: u64, r12: u64,
    r11: u64, r10: u64, r9: u64, r8: u64,
    rbp: u64, rdi: u64, rsi: u64, rbx: u64, rax: u64,
}

pub(crate) fn print_hex(val: u64) {
    let digits = b"0123456789abcdef";
    let mut buf = [0u8; 16];
    for i in 0..16 {
        buf[15 - i] = digits[((val >> (i * 4)) & 0xf) as usize];
    }
    console::print_str(unsafe { core::str::from_utf8_unchecked(&buf) });
}

// Called by isr14. error_code bit 2 = user-mode fault.
extern "C" fn pagefault_handler_x86(va: u64, error_code: u64, rip: u64, gprs: *const PfSavedGprs) {
    if error_code & 4 == 0 {
        // Fault from kernel mode, panic
        console::print_str("\n[EXCEPTION] Kernel Page Fault\n");
        loop { unsafe { core::arch::asm!("hlt") } }
    }
    if !crate::scheduler::handle_user_page_fault(va) {
        // Unresolvable user fault, print VA/error so we can diagnose, then SIGSEGV
        console::print_str("[#PF] va=0x");
        print_hex(va);
        console::print_str(" err=0x");
        print_hex(error_code);
        console::print_str(" rip=0x");
        print_hex(rip);
        let fsb = crate::arch::read_fs_base();
        console::print_str(" fs_base=0x");
        print_hex(fsb);
        console::print_str(" *fs_base=0x");
        print_hex(unsafe { *(fsb as *const u64) });
        console::print_str("\n");
        let g = unsafe { &*gprs };
        console::print_str("  rax=0x"); print_hex(g.rax);
        console::print_str(" rbx=0x"); print_hex(g.rbx);
        console::print_str(" rsi=0x"); print_hex(g.rsi);
        console::print_str(" rdi=0x"); print_hex(g.rdi);
        console::print_str("\n  rbp=0x"); print_hex(g.rbp);
        console::print_str(" r8=0x");  print_hex(g.r8);
        console::print_str(" r9=0x");  print_hex(g.r9);
        console::print_str(" r10=0x"); print_hex(g.r10);
        console::print_str("\n  r11=0x"); print_hex(g.r11);
        console::print_str(" r12=0x"); print_hex(g.r12);
        console::print_str(" r13=0x"); print_hex(g.r13);
        console::print_str(" r14=0x"); print_hex(g.r14);
        console::print_str(" r15=0x"); print_hex(g.r15);
        console::print_str("\n");
        crate::scheduler::exit_current(139); // 128 + SIGSEGV(11)
        loop { unsafe { core::arch::asm!("hlt") } }
    }
    // Fault resolved, iretq in isr14 retries the faulting instruction.
}

extern "C" fn common_dispatch(vector: u8) {
    match vector {
        // CPU exceptions
        0  => halt_exception("Divide by Zero"),
        1  => {} // Debug ignore
        2  => halt_exception("NMI"),
        3  => {} // Breakpoint ignore
        4  => halt_exception("Overflow"),
        5  => halt_exception("Bound Range"),
        6  => halt_exception("Invalid Opcode"),
        7  => halt_exception("Device Not Available"),
        8  => halt_exception("Double Fault"),
        13 => halt_exception("General Protection Fault"),
        14 => halt_exception("Page Fault"),
        _u8 @ 0..=31 => halt_exception("CPU Exception"),

        // IRQ0 PIT timer
        32 => {
            crate::scheduler::tick();
            // USB HID keyboard input (`drivers::usb::hid`) is polled, not
            // IRQ-driven (UHCI runs with USBINTR=0), so a task blocked in
            // `block_on_tty` would otherwise only ever be woken by IRQ1
            // (PS/2) or IRQ4 (serial) traffic and never get a chance to
            // re-poll USB. Waking it every tick here is a safe no-op when
            // nothing is actually waiting (`wake_tty_waiter` no-ops with no
            // waiter) — mirrors `boards::raspberrypi3::intc::handle` doing
            // the same for its own polled DWC2 USB HID keyboard.
            crate::scheduler::wake_tty_waiter();
            pic::eoi(0);
        }
        // IRQ1 PS/2 keyboard
        33 => {
            crate::drivers::keyboard::handle_irq();
            pic::eoi(1);
        }
        // IRQ4 COM1 serial RX
        36 => {
            crate::drivers::serial::handle_irq();
            pic::eoi(4);
        }
        // Other PIC IRQs send EOI and ignore
        34..=47 => {
            pic::eoi(vector - 32);
        }

        _ => {} // ignore unknown
    }
}

fn halt_exception(msg: &'static str) {
    console::print_str("\n[EXCEPTION] ");
    console::print_str(msg);
    console::print_str("\n");
    loop { unsafe { core::arch::asm!("hlt") } }
}

pub fn init() {
    // Table of per-vector handlers
    let handlers: [u64; 48] = [
        isr0  as u64, isr1  as u64, isr2  as u64, isr3  as u64,
        isr4  as u64, isr5  as u64, isr6  as u64, isr7  as u64,
        isr8  as u64, isr9  as u64, isr10 as u64, isr11 as u64,
        isr12 as u64, isr13 as u64, isr14 as u64, isr15 as u64,
        isr16 as u64, isr17 as u64, isr18 as u64, isr19 as u64,
        isr20 as u64, isr21 as u64, isr22 as u64, isr23 as u64,
        isr24 as u64, isr25 as u64, isr26 as u64, isr27 as u64,
        isr28 as u64, isr29 as u64, isr30 as u64, isr31 as u64,
        isr32 as u64, isr33 as u64, isr34 as u64, isr35 as u64,
        isr36 as u64, isr37 as u64, isr38 as u64, isr39 as u64,
        isr40 as u64, isr41 as u64, isr42 as u64, isr43 as u64,
        isr44 as u64, isr45 as u64, isr46 as u64, isr47 as u64,
    ];

    // P=1, DPL=0, 64-bit interrupt gate (clears IF)
    let attr_int  = 0x8E_u8;
    // P=1, DPL=0, 64-bit trap gate (does NOT clear IF)
    let attr_trap = 0x8F_u8;

    unsafe {
        for (i, &h) in handlers.iter().enumerate() {
            IDT[i].set(h, 0x08, 0, attr_int);
        }
        // Fill 48-255 with the generic ignore handler
        for i in 48..256usize {
            IDT[i].set(isr_ignore as u64, 0x08, 0, attr_int);
        }

        // Vector 0x80 software syscall (INT 0x80).
        // DPL=3 allows ring-3 code to invoke it; trap gate preserves IF.
        // attr: P=1, DPL=3, 64-bit trap gate = 0xEF
        IDT[0x80].set(isr128 as u64, 0x08, 0, 0xEF);

        let ptr = IdtPtr {
            limit: (core::mem::size_of_val(&IDT) - 1) as u16,
            base:  IDT.as_ptr() as u64,
        };
        core::arch::asm!("lidt [{0}]", in(reg) &ptr, options(nostack));
    }
}
