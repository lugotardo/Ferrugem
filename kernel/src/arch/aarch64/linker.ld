OUTPUT_ARCH(aarch64)
ENTRY(_boot_start)

SECTIONS {
    /* RAM on QEMU virt starts at 0x4000_0000; kernel is loaded 512 KiB in,
     * matching the offset convention used by most bare-metal aarch64/virt
     * examples (QEMU jumps straight to the ELF entry point via -kernel, so
     * the offset itself is just convention, not a hard requirement). */
    . = 0x40080000;

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

    . = ALIGN(4096);
    __bss_start = .;
    .bss : {
        *(COMMON)
        *(.bss*)
        . = ALIGN(4096);
    }
    __bss_end = .;
    _kernel_end = .;

    /DISCARD/ : { *(.eh_frame) *(.note*) }
}
