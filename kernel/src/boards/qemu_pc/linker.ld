OUTPUT_FORMAT(elf64-x86-64)
OUTPUT_ARCH(i386:x86-64)
ENTRY(_boot_start)

SECTIONS {
    . = 0x100000;   /* 1 MiB loaded by GRUB/QEMU multiboot */

    /* PVH ELF note for QEMU direct -kernel boot (read via PT_NOTE, not PT_LOAD). */
    .note.Xen : {
        *(.note.Xen)
    }

    /* Text: .text.boot first so the Multiboot1 header lands at the very start
     * of the LOAD segment (file offset < 8 KiB).  GRUB finds the magic there.
     * _boot_start immediately follows the 12-byte header; ELF e_entry → there. */
    .text : {
        *(.text.boot)
        *(.text*)
    }

    .rodata : {
        *(.rodata*)
    }

    .data : {
        *(.data*)
    }

    .bss : {
        *(COMMON)
        *(.bss*)
        . = ALIGN(4096);
    }
    _kernel_end = .;

    /* Discard C++ unwinding frames.  Do NOT discard .note*, that would nuke
     * .note.Xen (needed by QEMU PVH).  Other .note sections are harmless. */
    /DISCARD/ : { *(.eh_frame) *(.gcc_except_table) }
}
