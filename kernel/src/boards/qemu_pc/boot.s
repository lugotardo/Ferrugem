/* x86_64 boot stub  AT&T syntax (GNU as).
 *
 * Multiboot2 header is placed at the start of .text.boot (< 32 KiB into file).
 * QEMU ≥ 6.0 and GRUB2 both accept multiboot2 with a 64-bit ELF.
 * EAX = 0x36D76289 (MB2 magic) on entry; multiboot.rs fallback handles it.
 *
 * Layout inside .text.boot:
 *   [0x00] multiboot2 header (24 bytes)
 *   [0x18] _boot_start  ← ELF e_entry (32-bit protected mode entry)
 */

/* Multiboot2: magic + arch + header_length + checksum, then end tag. */
.set MB2_MAGIC,  0xE85250D6
.set MB2_ARCH,   0             /* 0 = i386 protected mode */
.set MB2_LEN,    24            /* 16 bytes header + 8 bytes end tag */
.set MB2_CSUM,   (-(MB2_MAGIC + MB2_ARCH + MB2_LEN))

/* ── PVH ELF note (used by QEMU ≥ 6.0 direct -kernel boot) ───────────── */
.section .note.Xen, "a"
.align 4
    .long 4             /* namesz = len("Xen\0") */
    .long 4             /* descsz = sizeof(u32) */
    .long 18            /* type = XEN_ELFNOTE_PHYS32_ENTRY */
    .ascii "Xen\0"
    .long _boot_start

/* ── Multiboot2 header + 32-bit entry (single .text.boot section) ─────── */
.section .text.boot
.code32
.align 8

/* Multiboot2 header, must be within first 32 KiB of the file.
 * Placed at the very start of .text.boot. */
mb2_header_start:
    .long MB2_MAGIC
    .long MB2_ARCH
    .long MB2_LEN
    .long MB2_CSUM
    /* end tag: type=0, flags=0, size=8 */
    .word 0
    .word 0
    .long 8
mb2_header_end:

/* 32-bit protected-mode entry point.
 * ELF e_entry (set by ENTRY(_boot_start)) points here.
 * GRUB sets EAX = 0x2BADB002, EBX → MBI, then jumps here.           */
.global _boot_start
_boot_start:
    cli
    movl  $_boot_stack_top, %esp

    /* Save multiboot magic and info pointer */
    movl  %eax, multiboot_magic
    movl  %ebx, multiboot_info

    /* 1. PML4[0] → PDPT */
    movl  $_boot_pdpt, %eax
    orl   $3, %eax
    movl  %eax, _boot_pml4

    /* 2. PDPT[0] → 1 GiB identity huge page */
    movl  $0x83, _boot_pdpt

    /* 3. CR3 ← PML4 */
    movl  $_boot_pml4, %eax
    movl  %eax, %cr3

    /* 4. CR4.PAE = 1 */
    movl  %cr4, %eax
    orl   $(1 << 5), %eax
    movl  %eax, %cr4

    /* 5. IA32_EFER.LME = 1 */
    movl  $0xC0000080, %ecx
    rdmsr
    orl   $(1 << 8), %eax
    wrmsr

    /* 6. Load temporary 64-bit GDT */
    lgdt  _boot_gdt_ptr

    /* 7. CR0: enable paging + protected mode + NE (required by VT-x FIXED0) */
    movl  %cr0, %eax
    orl   $(1 << 31) | (1 << 5) | 1, %eax
    movl  %eax, %cr0

    /* 8. Far-jump to 64-bit code segment */
    ljmpl $0x08, $_start64

/* ── 64-bit entry ──────────────────────────────────────────────────────── */
.code64
_start64:
    movw  $0x10, %ax
    movw  %ax, %ds
    movw  %ax, %es
    movw  %ax, %fs
    movw  %ax, %gs
    movw  %ax, %ss

    movq  $_boot_stack_top, %rsp
    call  _start

1:  hlt
    jmp   1b

/* ── Temporary 64-bit GDT ──────────────────────────────────────────────── */
.align 8
_boot_gdt:
    .quad 0x0000000000000000    /* null */
    .quad 0x00AF9A000000FFFF    /* 0x08: 64-bit kernel code */
    .quad 0x00CF92000000FFFF    /* 0x10: kernel data */
_boot_gdt_end:

_boot_gdt_ptr:
    .word _boot_gdt_end - _boot_gdt - 1
    .long _boot_gdt

/* ── Temporary page tables (in .bss, zeroed by ELF loader) ────────────── */
.section .bss
.align 4096
_boot_pml4: .skip 4096
_boot_pdpt: .skip 4096

/* ── Boot stack (16 KiB) ──────────────────────────────────────────────── */
.align 16
_boot_stack_bottom:
    .skip 16384
_boot_stack_top:

/* ── Saved multiboot values ───────────────────────────────────────────── */
.section .data
.global multiboot_magic
.global multiboot_info
multiboot_magic: .long 0
multiboot_info:  .long 0
