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
    check_scroll();
    unsafe {
        if let Some(b) = STATE.pop_byte() {
            return Some(b);
        }
    }
    crate::drivers::usb::take_key()
}

/// Applies any pending Shift+PageUp(-1)/PageDown(+1) scrollback request
/// (see `KeyboardState::take_scroll`). Piggybacking on `read_byte` - the
/// only per-iteration check in the shell's stdin read loop
/// (`syscall::sys_read`) - means this needs no separate timer or IRQ hook to
/// stay responsive.
///
/// Drains *both* transports unconditionally, not `ps2.take_scroll().or_else
/// (usb::take_scroll)`: PS/2 and USB HID keep independent `KeyboardState`s
/// (see the module doc comment - "the same state" is aspirational, not
/// actual), and under QEMU's default `qemu-pc` flags (chipset PS/2 *and*
/// `-device usb-kbd` both live at once, see `Makefile`) a single physical
/// keypress reaches both and gets decoded twice. `or_else` only calls
/// `usb::take_scroll` when PS/2 came back empty, so a duplicate sitting in
/// the USB side survives to fire on a *later*, unrelated call - a real
/// keypress could scroll one page now and a phantom second page moments
/// later. Calling both every time and folding the results with `or`
/// discards that duplicate in the same tick it arrived, at the cost of
/// coalescing two genuinely distinct simultaneous scroll requests (from two
/// different physical keyboards) into one - an acceptable trade for a
/// feature that's just "one page more" either way.
fn check_scroll() {
    let ps2_dir = unsafe { STATE.take_scroll() };
    let usb_dir = crate::drivers::usb::take_scroll();
    if let Some(dir) = ps2_dir.or(usb_dir) {
        crate::boards::current::console::scroll_view(dir);
    }
}

pub fn read_byte_blocking() -> u8 {
    loop {
        if let Some(b) = read_byte() { return b; }
        unsafe { core::arch::asm!("pause", options(nostack)) };
    }
}
