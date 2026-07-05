/// Keyboard driver for x86_64: PS/2 hardware feeding the shared
/// `KeyboardState` (modifiers, layout, LED sync), plus - once a UHCI
/// controller is found - a USB HID boot-protocol keyboard feeding the same
/// state (see `crate::drivers::usb`), exactly like `boards::raspberrypi3`
/// does for its DWC2-based USB stack.

use crate::drivers::keyboard::src::{ps2, state::KeyboardState};

static mut STATE: KeyboardState = KeyboardState::new();

pub fn init() {
    ps2::init();
}

/// Called from IRQ1 handler.
pub fn handle_irq() {
    unsafe {
        if let Some(sc) = ps2::try_read() {
            STATE.feed_ps2_byte(sc);
            sync_leds();
            // A task blocked in `block_on_tty` only gets rescheduled to
            // re-check for input via an explicit wake; without this, PS/2
            // bytes pile up in `STATE`'s ring buffer but the blocked reader
            // is never resumed to drain them (see IRQ0 below for the same
            // fix applied to the polled USB HID keyboard).
            crate::scheduler::wake_tty_waiter();
        }
    }
}

fn sync_leds() {
    unsafe {
        if STATE.take_led_dirty() {
            ps2::set_leds(STATE.ps2_led_mask());
        }
    }
}

pub fn has_input() -> bool {
    unsafe {
        if STATE.has_output() {
            return true;
        }
    }
    crate::drivers::usb::has_key()
}

pub fn read_byte() -> Option<u8> {
    unsafe {
        if let Some(b) = STATE.pop_byte() {
            return Some(b);
        }
    }
    crate::drivers::usb::take_key()
}

pub fn read_byte_blocking() -> u8 {
    loop {
        if let Some(b) = read_byte() { return b; }
        unsafe { core::arch::asm!("pause", options(nostack)) };
    }
}
