/// PS/2 keyboard hardware primitives, i8042 controller, IRQ1, scancode set 1.
/// Byte decoding (scancode set 1 -> `KeyCode`, modifiers, layout) lives in
/// `super::state::KeyboardState`; this module only talks to the hardware.

use crate::arch::x86_64::port;

const PS2_DATA:   u16 = 0x60;
const PS2_STATUS: u16 = 0x64;
const PS2_CMD:    u16 = 0x64;

const STATUS_OBF: u8 = 1 << 0; // Output buffer full (data ready to read)
const STATUS_IBF: u8 = 1 << 1; // Input buffer full (controller busy)

const KBD_CMD_SET_LEDS: u8 = 0xED;
const KBD_ACK: u8 = 0xFA;

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

/// Send the "Set/Reset Status Indicators" command with `mask` (see
/// `KeyboardState::ps2_led_mask`) so the keyboard's own Caps/Num/Scroll Lock
/// LEDs match the toggle state we're tracking in software. Best-effort: a
/// keyboard that doesn't ACK within the timeout is left as-is, this is a
/// cosmetic sync, not something worth blocking input on.
pub fn set_leds(mask: u8) {
    unsafe {
        wait_ibf_clear();
        port::outb(PS2_DATA, KBD_CMD_SET_LEDS);
        if !wait_obf_set() || port::inb(PS2_DATA) != KBD_ACK {
            return;
        }
        wait_ibf_clear();
        port::outb(PS2_DATA, mask & 0x07);
        if wait_obf_set() {
            let _ = port::inb(PS2_DATA); // second ACK; value not otherwise needed
        }
    }
}

unsafe fn wait_ibf_clear() {
    let mut n = 0u32;
    while port::inb(PS2_STATUS) & STATUS_IBF != 0 { n += 1; if n > 100_000 { break; } }
}

/// Returns `false` on timeout instead of spinning forever, so `set_leds`
/// (called from IRQ context, not just `init`) can bail out of a stuck
/// handshake instead of hanging the whole keyboard interrupt handler.
unsafe fn wait_obf_set() -> bool {
    let mut n = 0u32;
    while port::inb(PS2_STATUS) & STATUS_OBF == 0 {
        n += 1;
        if n > 100_000 { return false; }
    }
    true
}
