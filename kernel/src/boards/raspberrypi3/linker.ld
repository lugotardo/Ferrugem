OUTPUT_ARCH(aarch64)
ENTRY(_boot_start)

SECTIONS {
    /* Raspberry Pi 3 firmware (and QEMU's `raspi3b` machine, which models
     * it) loads a 64-bit kernel ("kernel8.img") at physical 0x80000 and
     * jumps straight to it, unlike QEMU virt's arbitrary `-kernel` ELF
     * entry point, this address is a firmware convention, not a choice. */
    . = 0x80000;

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
