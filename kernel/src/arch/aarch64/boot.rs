//! aarch64 boot entry point.
//!
//! Unlike x86_64/riscv64, this is NOT assembled by `build.rs` from a
//! separate `boot.s` — there is no `aarch64-*-as`/`clang` cross-assembler
//! available in every dev environment. `global_asm!` is compiled by
//! rustc's own LLVM backend (which already targets `aarch64-unknown-none`
//! natively), so the boot stub has no external toolchain dependency.
//!
//! QEMU's `virt` machine jumps here via `-kernel` with:
//!   x0 = pointer to the device tree blob (DTB)
//!   x1-x3 = 0
//! at whatever EL the board defaults to. QEMU virt starts at EL2 (with
//! `virtualization=on`) or EL1; QEMU's `raspi3b` machine — unlike real
//! Raspberry Pi 3 firmware, which drops to EL2 before jumping to the
//! kernel — resets the CPU straight into EL3. We defensively drop from
//! whichever of EL3/EL2 we're handed down to EL1 since this kernel never
//! uses TrustZone or virtualization.
//!
//! Unlike QEMU virt's default `-kernel` boot (one vCPU unless `-smp` is
//! given), real Raspberry Pi 3 firmware — and QEMU's `raspi3b` machine,
//! which models it — releases all 4 cores at this same entry point
//! simultaneously. Only core 0 may proceed past the check below; the rest
//! park in `wfe` forever (no SMP support yet — see `scheduler`), since
//! letting every core race through `.bss` zeroing and into `kernel_main`
//! would corrupt shared kernel state.

core::arch::global_asm!(
    ".section .text.boot",
    ".global _boot_start",
    "_boot_start:",
    "    mrs  x1, mpidr_el1",
    "    and  x1, x1, #0xFF",           // Aff0 = core ID (single cluster)
    "    cbz  x1, 0f",                  // core 0 -> continue boot
    "9:",
    "    wfe",
    "    b    9b",                      // secondary cores -> park forever
    "0:",
    "    mov  x19, x0",                 // preserve DTB pointer across EL setup

    "    mrs  x0, CurrentEL",
    "    lsr  x0, x0, #2",
    "    cmp  x0, #3",
    "    b.eq 5f",                      // EL3 (QEMU raspi3b) -> full EL3 drop
    "    cmp  x0, #2",
    "    b.ne 1f",                      // already at EL1 (or lower) -> skip drop

    // ── EL2 -> EL1 drop ──────────────────────────────────────────────────
    "    mov  x0, #1",
    "    lsl  x0, x0, #31",
    "    msr  hcr_el2, x0",             // HCR_EL2.RW = 1: EL1 runs AArch64

    "    mov  x0, #3",
    "    msr  cnthctl_el2, x0",         // EL1 may access physical timer/counter
    "    msr  cntvoff_el2, xzr",

    "    mov  x0, #0x0800",
    "    movk x0, #0x30d0, lsl #16",
    "    msr  sctlr_el1, x0",           // known-good EL1 reset state (MMU/caches off)

    "    msr  cptr_el2, xzr",           // don't trap FP/SIMD access to EL2

    "    mov  x0, #0x3c5",              // EL1h, DAIF all masked
    "    msr  spsr_el2, x0",
    "    adr  x0, 1f",
    "    msr  elr_el2, x0",
    "    eret",

    // ── EL3 -> EL1 drop (skips EL2 entirely — architecturally valid; we
    // never run a hypervisor, so there's nothing EL2 needs to do) ─────────
    "5:",
    "    mov  x0, #0x401",              // SCR_EL3: NS=1, RW=1 (lower ELs AArch64)
    "    msr  scr_el3, x0",

    // SCR_EL3.NS=1 puts us in the "non-secure EL2 present" case, where
    // HCR_EL2.RW (not SCR_EL3.RW) selects EL1's execution state — same
    // bit the EL2->EL1 path above sets, needed here too.
    "    mov  x0, #1",
    "    lsl  x0, x0, #31",
    "    msr  hcr_el2, x0",

    "    mov  x0, #3",
    "    msr  cnthctl_el2, x0",         // EL1 may access physical timer/counter
    "    msr  cntvoff_el2, xzr",

    "    mov  x0, #0x0800",
    "    movk x0, #0x30d0, lsl #16",
    "    msr  sctlr_el1, x0",           // known-good EL1 reset state (MMU/caches off)

    "    msr  cptr_el2, xzr",           // don't trap FP/SIMD access to EL2
    "    msr  cptr_el3, xzr",           // don't trap FP/SIMD access to EL3

    "    mov  x0, #0x3c5",              // EL1h, DAIF all masked
    "    msr  spsr_el3, x0",
    "    adr  x0, 1f",
    "    msr  elr_el3, x0",
    "    eret",

    // ── EL1 from here on ─────────────────────────────────────────────────
    "1:",
    // LLVM's aarch64 codegen uses NEON registers for plain array/struct
    // memset-style code (e.g. `[0u8; N]`), not just explicit float math —
    // without this, the very first such instruction anywhere in kernel_main
    // takes a synchronous "FP/SIMD access disabled" exception.
    "    mov  x0, #(0b11 << 20)",
    "    msr  cpacr_el1, x0",
    "    isb",

    "    adrp x0, _boot_stack_top",
    "    add  x0, x0, :lo12:_boot_stack_top",
    "    mov  sp, x0",

    "    adrp x1, aarch64_fdt_ptr",
    "    add  x1, x1, :lo12:aarch64_fdt_ptr",
    "    str  x19, [x1]",               // stash DTB pointer for later (Fase 2)

    // zero .bss (ELF loading via `-kernel` does not do this for us)
    "    adrp x0, __bss_start",
    "    add  x0, x0, :lo12:__bss_start",
    "    adrp x1, __bss_end",
    "    add  x1, x1, :lo12:__bss_end",
    "2:",
    "    cmp  x0, x1",
    "    b.ge 3f",
    "    str  xzr, [x0], #8",
    "    b    2b",
    "3:",

    "    bl   _start",
    "4:",
    "    wfi",
    "    b    4b",

    // ── Boot stack (16 KiB) ──────────────────────────────────────────────
    ".section .bss",
    ".align 12",
    "_boot_stack_bottom:",
    "    .skip 16384",
    "_boot_stack_top:",

    // ── DTB pointer saved at early boot (unused until Fase 2 FDT parsing) ─
    ".align 3",
    ".global aarch64_fdt_ptr",
    "aarch64_fdt_ptr:",
    "    .skip 8",
);
