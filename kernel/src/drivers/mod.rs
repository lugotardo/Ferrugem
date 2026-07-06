//! Device-class drivers: logic reusable across more than one board (and
//! often more than one ISA), as opposed to `crate::boards`, which holds
//! logic tied to exactly one concrete platform.
//!
//! The dividing line is mechanical: a driver may only branch on
//! `#[cfg(target_arch = "...")]`, never on `#[cfg(feature = "board-...")]`.
//! The moment a piece of code exists only because one specific board has
//! that chip wired up, it belongs in `crate::boards::<board>` instead, even
//! if it duplicates a driver's shape module-for-module. The canonical
//! example is USB: `usb::uhci` here is a PCI-discovered controller shared
//! by every x86_64 board, while the Raspberry Pi 3's on-chip DWC2
//! controller lives entirely under `crate::boards::raspberrypi3::usb`
//! despite mirroring `usb`'s hub/HID/mass-storage modules one for one.

pub mod keyboard;
pub mod ring_buf;
pub mod serial;
pub mod timer;
#[cfg(target_arch = "x86_64")]
pub mod usb;

pub fn init() {
    timer::init();
    #[cfg(target_arch = "x86_64")]
    usb::init();
    keyboard::init();
    serial::init();
}
