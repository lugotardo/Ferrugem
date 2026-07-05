//! USB host stack for real Raspberry Pi 3 hardware: brings up the BCM2837's
//! on-chip DWC2 controller, enumerates whatever's attached (on a real board
//! that's always the onboard LAN9514 hub feeding the 4 physical USB-A
//! ports), and wires up every HID boot-protocol keyboard found into
//! `crate::drivers::keyboard` (see `drivers::keyboard::arch::aarch64`).
//! Ports are rechecked periodically after boot too, so plugging or
//! unplugging a keyboard later is picked up without a reboot (see
//! `hub::poll_hotplug`).
//!
//! Entirely polling-based, control- and interrupt-transfers only, see
//! `dwc2.rs`'s module docs for why. Best-effort like every other real-vs-
//! QEMU difference this board handles: QEMU's `raspi3b` machine has no
//! DWC2 model at all, so `init` logs why and returns immediately there,
//! leaving the UART console fully usable either way.

mod dwc2;
mod hub;
pub mod hid;
mod protocol;

/// Every `HOTPLUG_POLL_PERIOD` calls to `has_key`/`take_key`, also rescan
/// the root hub's ports for a connection change (see `hub::poll_hotplug`).
/// That's a handful of USB control transfers, cheap but not free, so it's
/// not worth doing on literally every keystroke poll (which happens far
/// more often than a cable actually gets plugged in) - but low enough that
/// plugging in a keyboard after boot (no keyboard attached yet is the
/// common real-hardware case: `init` still fully brings up the DWC2 core
/// and registers the root hub's ports even with nothing plugged into any of
/// them) is noticed within a fraction of a second of actual shell/syscall
/// input polling, not after thousands of keystrokes.
const HOTPLUG_POLL_PERIOD: u32 = 256;
static mut POLL_COUNT: u32 = 0;

pub fn init() {
    if !dwc2::init() {
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
