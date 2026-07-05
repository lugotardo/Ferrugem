//! aarch64 exception vector table (EL1).
//!
//! Fase 1 only ever runs kernel code at EL1h (SPSel=1), there is no EL0
//! yet, so only the "current EL, SPx" IRQ/sync vectors (table offset
//! 0x200-0x3FF) are expected to fire for real. Every other slot still gets
//! a valid handler (the same fatal path) since an unpopulated VBAR_EL1 slot
//! that actually fired would branch into garbage and crash unrecoverably
//! with no diagnostic.
//!
//! Like `boot.rs`, the vector table and its entry stubs are hand-written
//! `global_asm!` rather than `#[naked]` functions: they need precise
//! control over full GPR + ELR_EL1/SPSR_EL1 save before any Rust code (with
//! its own register clobbers) can run, and the table itself needs
//! byte-exact 0x80-aligned slots that a normal `fn` can't express.

#[repr(C)]
pub struct TrapFrame {
    pub x: [u64; 31], // x0-x30 (x30 = link register)
    pub elr_el1: u64,
    pub spsr_el1: u64,
    _pad: u64, // keeps the frame size (272 B) a multiple of 16
}

pub fn init() {
    unsafe extern "C" {
        static aarch64_vector_table: u8;
    }
    unsafe {
        let table = &aarch64_vector_table as *const u8 as u64;
        core::arch::asm!("msr vbar_el1, {t}", "isb", t = in(reg) table, options(nostack));
    }
}

#[unsafe(no_mangle)]
extern "C" fn aarch64_irq_handler(_frame: *mut TrapFrame) {
    super::intc::handle();
}

#[unsafe(no_mangle)]
extern "C" fn aarch64_fatal_handler(frame: *mut TrapFrame) -> ! {
    unsafe {
        let esr: u64;
        let far: u64;
        core::arch::asm!("mrs {v}, esr_el1", v = out(reg) esr, options(nostack));
        core::arch::asm!("mrs {v}, far_el1", v = out(reg) far, options(nostack));
        let elr = (*frame).elr_el1;

        super::console::print_str("[TRAP] unhandled aarch64 exception esr=0x");
        print_hex(esr);
        super::console::print_str(" far=0x");
        print_hex(far);
        super::console::print_str(" elr=0x");
        print_hex(elr);
        super::console::print_str("\n");
    }
    loop {
        unsafe { core::arch::asm!("wfi", options(nostack)) }
    }
}

fn print_hex(val: u64) {
    let mut buf = [b'0'; 16];
    for j in 0..16 {
        let nibble = ((val >> (60 - j * 4)) & 0xF) as u8;
        buf[j] = if nibble < 10 { b'0' + nibble } else { b'a' + nibble - 10 };
    }
    if let Ok(s) = core::str::from_utf8(&buf) {
        super::console::print_str(s);
    }
}

core::arch::global_asm!(
    ".section .text",
    ".align 11", // 2^11 = 2048-byte alignment required for VBAR_EL1
    ".global aarch64_vector_table",
    "aarch64_vector_table:",

    // ── Current EL, SP0 (unused: we always run EL1h) ────────────────────
    ".balign 0x80", "b vec_fatal", // Synchronous
    ".balign 0x80", "b vec_fatal", // IRQ
    ".balign 0x80", "b vec_fatal", // FIQ
    ".balign 0x80", "b vec_fatal", // SError

    // ── Current EL, SPx = EL1h (this is what actually fires) ────────────
    ".balign 0x80", "b vec_fatal", // Synchronous (bug if it ever fires: no SVC path yet)
    ".balign 0x80", "b vec_irq",   // IRQ
    ".balign 0x80", "b vec_fatal", // FIQ
    ".balign 0x80", "b vec_fatal", // SError

    // ── Lower EL, AArch64 (unused: no EL0 in Fase 1) ────────────────────
    ".balign 0x80", "b vec_fatal",
    ".balign 0x80", "b vec_fatal",
    ".balign 0x80", "b vec_fatal",
    ".balign 0x80", "b vec_fatal",

    // ── Lower EL, AArch32 (unused: no AArch32 support) ──────────────────
    ".balign 0x80", "b vec_fatal",
    ".balign 0x80", "b vec_fatal",
    ".balign 0x80", "b vec_fatal",
    ".balign 0x80", "b vec_fatal",

    "vec_irq:",
    "    sub sp, sp, #272",
    "    str x0,  [sp, #0*8]",  "str x1,  [sp, #1*8]",  "str x2,  [sp, #2*8]",
    "    str x3,  [sp, #3*8]",  "str x4,  [sp, #4*8]",  "str x5,  [sp, #5*8]",
    "    str x6,  [sp, #6*8]",  "str x7,  [sp, #7*8]",  "str x8,  [sp, #8*8]",
    "    str x9,  [sp, #9*8]",  "str x10, [sp, #10*8]", "str x11, [sp, #11*8]",
    "    str x12, [sp, #12*8]", "str x13, [sp, #13*8]", "str x14, [sp, #14*8]",
    "    str x15, [sp, #15*8]", "str x16, [sp, #16*8]", "str x17, [sp, #17*8]",
    "    str x18, [sp, #18*8]", "str x19, [sp, #19*8]", "str x20, [sp, #20*8]",
    "    str x21, [sp, #21*8]", "str x22, [sp, #22*8]", "str x23, [sp, #23*8]",
    "    str x24, [sp, #24*8]", "str x25, [sp, #25*8]", "str x26, [sp, #26*8]",
    "    str x27, [sp, #27*8]", "str x28, [sp, #28*8]", "str x29, [sp, #29*8]",
    "    str x30, [sp, #30*8]",
    "    mrs x0, elr_el1",
    "    str x0,  [sp, #31*8]",
    "    mrs x0, spsr_el1",
    "    str x0,  [sp, #32*8]",
    "    mov x0, sp",
    "    bl  aarch64_irq_handler",
    "    ldr x0,  [sp, #31*8]",
    "    msr elr_el1, x0",
    "    ldr x0,  [sp, #32*8]",
    "    msr spsr_el1, x0",
    "    ldr x1,  [sp, #1*8]",  "ldr x2,  [sp, #2*8]",  "ldr x3,  [sp, #3*8]",
    "    ldr x4,  [sp, #4*8]",  "ldr x5,  [sp, #5*8]",  "ldr x6,  [sp, #6*8]",
    "    ldr x7,  [sp, #7*8]",  "ldr x8,  [sp, #8*8]",  "ldr x9,  [sp, #9*8]",
    "    ldr x10, [sp, #10*8]", "ldr x11, [sp, #11*8]", "ldr x12, [sp, #12*8]",
    "    ldr x13, [sp, #13*8]", "ldr x14, [sp, #14*8]", "ldr x15, [sp, #15*8]",
    "    ldr x16, [sp, #16*8]", "ldr x17, [sp, #17*8]", "ldr x18, [sp, #18*8]",
    "    ldr x19, [sp, #19*8]", "ldr x20, [sp, #20*8]", "ldr x21, [sp, #21*8]",
    "    ldr x22, [sp, #22*8]", "ldr x23, [sp, #23*8]", "ldr x24, [sp, #24*8]",
    "    ldr x25, [sp, #25*8]", "ldr x26, [sp, #26*8]", "ldr x27, [sp, #27*8]",
    "    ldr x28, [sp, #28*8]", "ldr x29, [sp, #29*8]", "ldr x30, [sp, #30*8]",
    "    ldr x0,  [sp, #0*8]",
    "    add sp, sp, #272",
    "    eret",

    "vec_fatal:",
    "    sub sp, sp, #272",
    "    str x0,  [sp, #0*8]",  "str x1,  [sp, #1*8]",  "str x2,  [sp, #2*8]",
    "    str x3,  [sp, #3*8]",  "str x4,  [sp, #4*8]",  "str x5,  [sp, #5*8]",
    "    str x6,  [sp, #6*8]",  "str x7,  [sp, #7*8]",  "str x8,  [sp, #8*8]",
    "    str x9,  [sp, #9*8]",  "str x10, [sp, #10*8]", "str x11, [sp, #11*8]",
    "    str x12, [sp, #12*8]", "str x13, [sp, #13*8]", "str x14, [sp, #14*8]",
    "    str x15, [sp, #15*8]", "str x16, [sp, #16*8]", "str x17, [sp, #17*8]",
    "    str x18, [sp, #18*8]", "str x19, [sp, #19*8]", "str x20, [sp, #20*8]",
    "    str x21, [sp, #21*8]", "str x22, [sp, #22*8]", "str x23, [sp, #23*8]",
    "    str x24, [sp, #24*8]", "str x25, [sp, #25*8]", "str x26, [sp, #26*8]",
    "    str x27, [sp, #27*8]", "str x28, [sp, #28*8]", "str x29, [sp, #29*8]",
    "    str x30, [sp, #30*8]",
    "    mrs x0, elr_el1",
    "    str x0,  [sp, #31*8]",
    "    mrs x0, spsr_el1",
    "    str x0,  [sp, #32*8]",
    "    mov x0, sp",
    "    bl  aarch64_fatal_handler",
    "9:  wfi",
    "    b   9b",
);
