/* RISC-V boot stub.
 * OpenSBI jumps here at 0x80200000 with:
 *   a0 = hart id
 *   a1 = device tree (FDT) pointer
 * Stack is not set up yet we do it here before calling _start.
 */

.section .text.boot
.global _boot_start
_boot_start:
    /* Set up kernel stack */
    la    sp, _boot_stack_top

    /* Persist FDT pointer (a1) before any code clobbers it */
    la    t0, riscv_fdt_ptr
    sd    a1, 0(t0)

    /* a0 = hart_id passed through to _start */
    call  _start

1:  wfi
    j     1b

/* ── Boot stack (16 KiB) ─────────────────────────────────────────────── */
.section .bss
.align 12   /* 2^12 = 4096 bytes */
_boot_stack_bottom:
    .skip 16384
_boot_stack_top:

/* ── FDT pointer saved at early boot ─────────────────────────────────── */
.align 8
.global riscv_fdt_ptr
riscv_fdt_ptr:
    .skip 8
