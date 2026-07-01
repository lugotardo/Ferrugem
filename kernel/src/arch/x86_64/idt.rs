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
make_isr!(isr12, 12);  make_isr!(isr13, 13);  // isr14 = custom #PF handler below
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

// syscall_entry_rust(nr, a0, a1, a2, a3, user_ip, user_sp)
// SysV 7-arg: rdi rsi rdx rcx r8 r9 [rsp+8]
#[unsafe(no_mangle)]
unsafe extern "C" fn syscall_entry_rust(
    nr: usize, a0: usize, a1: usize, a2: usize,
    a3: usize, user_ip: u64, user_sp: u64,
) -> isize {
    crate::syscall::set_user_ctx(user_ip, user_sp);
    crate::syscall::dispatch(nr, a0, a1, a2, a3)
}

// INT 0x80 syscall entry — Linux x86_64 ABI: rax=nr rdi=a0 rsi=a1 rdx=a2 r10=a3
//
// Stack layout after 11 pushes (S11 = rsp):
//   [S11+ 0]=rcx  [+8]=r9  [+16]=r8  [+24]=r11  [+32]=r10(a3)
//   [+40]=r15  [+48]=r14  [+56]=r13  [+64]=r12  [+72]=rbp  [+80]=rbx
//   [+88]=user_rip  [+96]=cs  [+104]=rflags  [+112]=user_rsp  [+120]=ss
//
// Alignment: kernel stack top is 16-byte aligned; CPU iretq push = 40 bytes
// → initial_rsp mod 16 = 8; after 11 pushes (88 bytes) → S11 mod 16 = 0.
#[unsafe(naked)]
#[unsafe(no_mangle)]
unsafe extern "C" fn isr128() {
    core::arch::naked_asm!(
        "push rbx", "push rbp", "push r12", "push r13", "push r14", "push r15",
        "push r10", "push r11", "push r8",  "push r9",  "push rcx",
        // rsp = S11 (mod 16 = 0)

        // Save nr (rax) in r11 — r11's original value is already saved at [S11+24]
        "mov r11, rax",

        // Push user_rsp as 7th argument (will sit at [callee_rsp+8] after call)
        "mov rax, qword ptr [rsp + 112]",  // user_rsp = [S11+112]
        "push rax",                         // rsp = S12 (mod 16 = 8)

        // Align stack: S12 mod 16 = 8, need 0 before call
        "sub rsp, 8",                       // rsp = S13 (mod 16 = 0)

        // Set up register args (SysV 7-arg convention):
        //   rdi=nr  rsi=a0  rdx=a1  rcx=a2  r8=a3  r9=user_rip  [rsp+8]=user_rsp
        // After sub 8: [S13+48]=[S11+32]=orig r10(a3)  [S13+104]=[S11+88]=user_rip
        "mov r9,  qword ptr [rsp + 104]",  // user_rip → r9
        "mov r8,  qword ptr [rsp + 48]",   // a3 = orig r10 → r8
        "mov rcx, rdx",
        "mov rdx, rsi",
        "mov rsi, rdi",
        "mov rdi, r11",                    // nr → rdi
        "call syscall_entry_rust",

        // Clean up 7th-arg push + alignment pad; rax = syscall return value
        "add rsp, 16",                     // rsp = S11

        // Apply exec redirect if execve was called (preserves rax across the call)
        "push rax",                        // save retval; rsp = S11-8
        "lea rdi, [rsp + 8]",             // rdi = S11 (iretq frame base)
        "sub rsp, 8",                      // align for call: (S11-8) mod 16 = 8 → 0 after sub
        "call apply_exec_redirect",
        "add rsp, 8",
        "pop rax",                         // restore retval; rsp = S11

        // Restore saved registers and return to user
        "pop rcx", "pop r9",  "pop r8",  "pop r11", "pop r10",
        "pop r15", "pop r14", "pop r13", "pop r12", "pop rbp", "pop rbx",
        "iretq",
    )
}

// ── #PF (vector 14) — custom handler ────────────────────────────────────────
//
// The CPU pushes an error code for #PF, which the generic make_isr! macro does
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
        "push rax",               // save all GPRs we use (13 pushes = 104 bytes)
        "push rbx",
        "push rsi",
        "push rdi",
        "push rbp",
        "push r8",  "push r9",  "push r10",
        "push r11", "push r12", "push r13", "push r14", "push r15",
        "mov  rdi, rcx",          // 1st arg: faulting VA
        "mov  rsi, rdx",          // 2nd arg: error_code
        "call {handler}",
        "pop  r15", "pop  r14", "pop  r13", "pop  r12",
        "pop  r11", "pop  r10", "pop  r9",  "pop  r8",
        "pop  rbp", "pop  rdi", "pop  rsi", "pop  rbx", "pop  rax",
        "iretq",
        handler = sym pagefault_handler_x86,
    )
}

// Called by isr14. error_code bit 2 = user-mode fault.
extern "C" fn pagefault_handler_x86(va: u64, error_code: u64) {
    if error_code & 4 == 0 {
        // Fault from kernel mode — panic
        console::print_str("\n[EXCEPTION] Kernel Page Fault\n");
        loop { unsafe { core::arch::asm!("hlt") } }
    }
    if !crate::scheduler::handle_user_page_fault(va) {
        // Unresolvable user fault (bad address or OOM) — SIGSEGV
        crate::scheduler::exit_current(139); // 128 + SIGSEGV(11)
        // exit_current calls schedule() which context-switches to another task.
        // This loop is only reached if schedule() returns, which means
        // there are no other runnable tasks — spin until the next timer tick.
        loop { unsafe { core::arch::asm!("hlt") } }
    }
    // Fault resolved — iretq in isr14 retries the faulting instruction.
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
