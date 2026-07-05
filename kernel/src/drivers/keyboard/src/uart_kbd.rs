/// UART "keyboard": treats raw serial bytes as already-decoded input, used
/// wherever there's no real keyboard hardware to speak to - RISC-V and
/// QEMU's aarch64 `virt` machine have neither PS/2 nor USB, and the
/// Raspberry Pi 3 falls back to this when no USB keyboard was found at boot.
/// UART bytes are already ASCII; printable/control values pass through
/// unchanged, anything else (e.g. a stray high byte) is dropped.

fn filter(b: u8) -> Option<u8> {
    match b {
        0x20..=0x7E | b'\n' | b'\r' | b'\x08' | b'\t' => Some(b),
        _ => None,
    }
}

pub fn has_input() -> bool {
    crate::drivers::serial::has_input()
}

pub fn read_byte() -> Option<u8> {
    crate::drivers::serial::read_byte().and_then(filter)
}

pub fn read_byte_blocking() -> u8 {
    crate::drivers::serial::read_byte_blocking()
}
