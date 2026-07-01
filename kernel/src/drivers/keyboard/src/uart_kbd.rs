/// UART keyboard scancode → ASCII for RISC-V.
/// UART bytes are already ASCII; printable values pass through unchanged.

pub fn scancode_to_ascii(sc: u8) -> Option<u8> {
    match sc {
        0x20..=0x7E | b'\n' | b'\r' | b'\x08' | b'\t' => Some(sc),
        _ => None,
    }
}
