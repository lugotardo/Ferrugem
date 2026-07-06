/// Keyboard driver for aarch64: on real Raspberry Pi 3 hardware, polls
/// whatever USB HID boot-protocol keyboard(s) were found during boot
/// (`boards::raspberrypi3::usb`), decoded through the same shared
/// `KeyboardState` machinery x86_64's PS/2 driver uses, falling back to
/// treating UART bytes as already-decoded input - the same fallback riscv64
/// and QEMU virt aarch64 use unconditionally (no USB stack exists for either).

use crate::drivers::keyboard::src::uart_kbd;

pub fn init() {}

pub fn handle_irq() {}

pub fn has_input() -> bool {
    #[cfg(feature = "board-raspberrypi3")]
    if crate::boards::raspberrypi3::usb::has_key() {
        return true;
    }
    uart_kbd::has_input()
}

pub fn read_byte() -> Option<u8> {
    // Shift+PageUp/PageDown scrollback (see `boards::raspberrypi3::hdmi`/
    // `fbconsole::scroll_view`): only reachable via a USB HID keyboard, the
    // UART fallback below carries already-decoded bytes with no Shift/
    // scancode concept to intercept - a real terminal's own scrollback
    // already covers that path anyway, same reasoning as x86_64's VGA console.
    #[cfg(feature = "board-raspberrypi3")]
    if let Some(dir) = crate::boards::raspberrypi3::usb::take_scroll() {
        crate::boards::raspberrypi3::fbconsole::scroll_view(dir);
    }
    #[cfg(feature = "board-raspberrypi3")]
    if let Some(b) = crate::boards::raspberrypi3::usb::take_key() {
        return Some(b);
    }
    uart_kbd::read_byte()
}

pub fn read_byte_blocking() -> u8 {
    uart_kbd::read_byte_blocking()
}
