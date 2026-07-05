//! USB host stack for x86_64 boards: brings up a UHCI controller found on
//! the PCI bus (`uhci.rs`), enumerates its 2 root ports, and wires up every
//! HID boot-protocol keyboard found into `crate::drivers::keyboard` (see
//! `drivers::keyboard::arch::x86_64`) - the x86 counterpart of
//! `boards::raspberrypi3::usb`. Ports are rechecked periodically after boot
//! too, so plugging or unplugging a keyboard later is picked up without a
//! reboot (see `hub::poll_hotplug`).
//!
//! Best-effort like the RPi3 stack: a machine with no UHCI controller at
//! all (e.g. QEMU `q35` started without `-usb`) just means `init` logs why
//! and returns, leaving PS/2 (and the serial console) fully usable either
//! way - see `uhci.rs`'s module doc comment for what's actually implemented.

mod protocol;
mod uhci;
mod hub;
mod hid;

/// See `boards::raspberrypi3::usb::HOTPLUG_POLL_PERIOD` - same reasoning,
/// a handful of PORTSC/control-transfer reads is cheap but not worth doing
/// on every single keystroke poll.
const HOTPLUG_POLL_PERIOD: u32 = 4096;
static mut POLL_COUNT: u32 = 0;

pub fn init() {
    if !uhci::init() {
        return;
    }
    hub::enumerate_root();
}

fn maybe_poll_hotplug() {
    unsafe {
        POLL_COUNT = POLL_COUNT.wrapping_add(1);
        if POLL_COUNT % HOTPLUG_POLL_PERIOD == 0 {
            hub::poll_hotplug();
        }
    }
}

pub fn has_key() -> bool {
    maybe_poll_hotplug();
    hid::has_key()
}

pub fn take_key() -> Option<u8> {
    maybe_poll_hotplug();
    hid::take_key()
}
