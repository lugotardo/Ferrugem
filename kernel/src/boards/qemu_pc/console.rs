use crate::arch::x86_64::port;

// COM1 UART mirror all output to serial for QEMU -serial stdio
const COM1: u16 = 0x3F8;

fn serial_write_byte(b: u8) {
    unsafe {
        // Wait for transmit holding register empty (LSR bit 5)
        while port::inb(COM1 + 5) & 0x20 == 0 {}
        port::outb(COM1, b);
    }
}

fn serial_init() {
    unsafe {
        port::outb(COM1 + 1, 0x00); // disable interrupts
        port::outb(COM1 + 3, 0x80); // DLAB on
        port::outb(COM1 + 0, 0x01); // divisor lo: 115200 baud
        port::outb(COM1 + 1, 0x00); // divisor hi
        port::outb(COM1 + 3, 0x03); // 8N1, DLAB off
        port::outb(COM1 + 2, 0x00); // leave FIFO disabled, com1::init() configures it
        port::outb(COM1 + 4, 0x0B); // RTS+DTR
    }
}

// VGA text buffer: 80x25, each cell is (char, attr)
const VGA_ADDR: *mut u16 = 0xB8000 as *mut u16;
const WIDTH: usize = 80;
const HEIGHT: usize = 25;

// Colour: light grey on black
const ATTR: u16 = 0x0700;

// CRTC ports for hardware cursor
const CRTC_ADDR: u16 = 0x3D4;
const CRTC_DATA: u16 = 0x3D5;

static mut COL: usize = 0;
static mut ROW: usize = 0;

pub fn serial_try_read() -> Option<u8> {
    unsafe {
        if port::inb(COM1 + 5) & 0x01 != 0 {
            Some(port::inb(COM1))
        } else {
            None
        }
    }
}

pub fn init() {
    serial_init();
    clear();
}

pub fn clear() {
    for i in 0..(WIDTH * HEIGHT) {
        unsafe { VGA_ADDR.add(i).write_volatile(ATTR | b' ' as u16) };
    }
    unsafe {
        COL = 0;
        ROW = 0;
    }
    update_cursor(0);
}

pub fn print_str(s: &str) {
    for b in s.bytes() {
        print_byte(b);
    }
}

pub fn print_byte(b: u8) {
    // Mirror to serial
    if b == b'\n' { serial_write_byte(b'\r'); }
    serial_write_byte(b);

    unsafe {
        match b {
            b'\n' => newline(),
            b'\r' => COL = 0,
            b'\x08' => backspace(),
            _ => {
                let off = ROW * WIDTH + COL;
                VGA_ADDR.add(off).write_volatile(ATTR | b as u16);
                COL += 1;
                if COL >= WIDTH {
                    newline();
                }
            }
        }
        update_cursor(ROW * WIDTH + COL);
    }
}

unsafe fn newline() {
    COL = 0;
    ROW += 1;
    if ROW >= HEIGHT {
        scroll();
        ROW = HEIGHT - 1;
    }
}

unsafe fn backspace() {
    if COL > 0 {
        COL -= 1;
    } else if ROW > 0 {
        ROW -= 1;
        COL = WIDTH - 1;
    }
    let off = ROW * WIDTH + COL;
    VGA_ADDR.add(off).write_volatile(ATTR | b' ' as u16);
}

unsafe fn scroll() {
    // Move all rows up by one
    for row in 1..HEIGHT {
        for col in 0..WIDTH {
            let src = VGA_ADDR.add(row * WIDTH + col).read_volatile();
            VGA_ADDR.add((row - 1) * WIDTH + col).write_volatile(src);
        }
    }
    // Clear last row
    for col in 0..WIDTH {
        VGA_ADDR.add((HEIGHT - 1) * WIDTH + col).write_volatile(ATTR | b' ' as u16);
    }
}

fn update_cursor(pos: usize) {
    let p = pos as u16;
    unsafe {
        port::outb(CRTC_ADDR, 0x0F);
        port::outb(CRTC_DATA, (p & 0xFF) as u8);
        port::outb(CRTC_ADDR, 0x0E);
        port::outb(CRTC_DATA, ((p >> 8) & 0xFF) as u8);
    }
}
