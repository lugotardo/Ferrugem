/// 8259A PIC remapped to vectors 0x20-0x2F to avoid conflicts with CPU exceptions.

use crate::arch::x86_64::port;

const PIC1_CMD:  u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_CMD:  u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;

const ICW1_ICW4:  u8 = 0x01;
const ICW1_INIT:  u8 = 0x10;
const ICW4_8086:  u8 = 0x01;
const PIC_EOI:    u8 = 0x20;

pub const PIC1_OFFSET: u8 = 0x20; // IRQ 0-7  → vectors 0x20-0x27
pub const PIC2_OFFSET: u8 = 0x28; // IRQ 8-15 → vectors 0x28-0x2F

pub fn init() {
    unsafe {
        // Save masks
        let mask1 = port::inb(PIC1_DATA);
        let mask2 = port::inb(PIC2_DATA);

        // Start init sequence
        port::outb(PIC1_CMD,  ICW1_INIT | ICW1_ICW4);
        port::outb(PIC2_CMD,  ICW1_INIT | ICW1_ICW4);

        // Vector offsets
        port::outb(PIC1_DATA, PIC1_OFFSET);
        port::outb(PIC2_DATA, PIC2_OFFSET);

        // Cascade: PIC2 on IRQ2
        port::outb(PIC1_DATA, 0x04);
        port::outb(PIC2_DATA, 0x02);

        // 8086 mode
        port::outb(PIC1_DATA, ICW4_8086);
        port::outb(PIC2_DATA, ICW4_8086);

        // Mask everything initially
        port::outb(PIC1_DATA, mask1 | 0xFB); // keep cascade open
        port::outb(PIC2_DATA, mask2 | 0xFF);
    }
}

pub fn eoi(irq: u8) {
    unsafe {
        if irq >= 8 {
            port::outb(PIC2_CMD, PIC_EOI);
        }
        port::outb(PIC1_CMD, PIC_EOI);
    }
}

pub fn unmask(irq: u8) {
    unsafe {
        let (port, bit) = if irq < 8 {
            (PIC1_DATA, irq)
        } else {
            (PIC2_DATA, irq - 8)
        };
        let mask = port::inb(port) & !(1 << bit);
        port::outb(port, mask);
    }
}

pub fn mask(irq: u8) {
    unsafe {
        let (port, bit) = if irq < 8 {
            (PIC1_DATA, irq)
        } else {
            (PIC2_DATA, irq - 8)
        };
        let val = port::inb(port) | (1 << bit);
        port::outb(port, val);
    }
}
