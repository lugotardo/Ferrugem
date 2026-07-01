OUTPUT_ARCH(riscv)
ENTRY(_boot_start)

SECTIONS {
    . = 0x80200000;   /* OpenSBI loads kernel here on QEMU virt */

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

    /DISCARD/ : { *(.eh_frame) *(.note*) }
}
