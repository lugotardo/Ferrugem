/// PS/2 keyboard hardware primitives — i8042 controller, IRQ1, scancode set 1.
/// Ring buffer and scheduler wakeup live in arch/x86_64.rs.

use crate::arch::x86_64::port;

const PS2_DATA:   u16 = 0x60;
const PS2_STATUS: u16 = 0x64;
const PS2_CMD:    u16 = 0x64;

const STATUS_OBF: u8 = 1 << 0; // Output buffer full (data ready to read)
const STATUS_IBF: u8 = 1 << 1; // Input buffer full (controller busy)

pub fn init() {
    unsafe {
        // Flush stale output buffer byte.
        if port::inb(PS2_STATUS) & STATUS_OBF != 0 { let _ = port::inb(PS2_DATA); }

        // Read controller command byte, enable keyboard IRQ + clock.
        wait_ibf_clear();
        port::outb(PS2_CMD, 0x20);
        wait_obf_set();
        let cmd = port::inb(PS2_DATA);
        let new_cmd = (cmd | 0x01) & !0x10;

        wait_ibf_clear();
        port::outb(PS2_CMD, 0x60);
        wait_ibf_clear();
        port::outb(PS2_DATA, new_cmd);

        // Enable scanning: send 0xF4, wait for ACK (0xFA).
        wait_ibf_clear();
        port::outb(PS2_DATA, 0xF4);
        wait_obf_set();
        let _ = port::inb(PS2_DATA);
    }
}

/// Reads one scancode from the controller if data is available and it's not a
/// mouse byte (bit 5 of status). Returns None if no data or mouse byte.
pub fn try_read() -> Option<u8> {
    unsafe {
        let st = port::inb(PS2_STATUS);
        if st & STATUS_OBF == 0 { return None; }
        let sc = port::inb(PS2_DATA);
        if st & 0x20 != 0 { return None; } // auxiliary (mouse) byte
        Some(sc)
    }
}

unsafe fn wait_ibf_clear() {
    let mut n = 0u32;
    while port::inb(PS2_STATUS) & STATUS_IBF != 0 { n += 1; if n > 100_000 { break; } }
}

unsafe fn wait_obf_set() {
    let mut n = 0u32;
    while port::inb(PS2_STATUS) & STATUS_OBF == 0 { n += 1; if n > 100_000 { break; } }
}

// US QWERTY scancode set 1 → ASCII (key-down only, unshifted).
static SCANCODE_MAP: [u8; 58] = [
    0, 0,
    b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'0', b'-', b'=', b'\x08',
    b'\t', b'q', b'w', b'e', b'r', b't', b'y', b'u', b'i', b'o', b'p', b'[', b']', b'\n',
    0,           // left ctrl
    b'a', b's', b'd', b'f', b'g', b'h', b'j', b'k', b'l', b';', b'\'', b'`',
    0,           // left shift
    b'\\', b'z', b'x', b'c', b'v', b'b', b'n', b'm', b',', b'.', b'/',
    0,           // right shift
    0, 0, b' ',
];

pub fn scancode_to_ascii(sc: u8) -> Option<u8> {
    if sc & 0x80 != 0 { return None; } // key-release
    let idx = sc as usize;
    if idx < SCANCODE_MAP.len() {
        let c = SCANCODE_MAP[idx];
        if c != 0 { Some(c) } else { None }
    } else {
        None
    }
}
