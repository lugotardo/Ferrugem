//! BSP: Raspberry Pi 3 (BCM2837), including QEMU's `-M raspi3b` machine
//! (which emulates it closely enough to serve as the day-to-day test target
//!, see the Makefile's `raspberrypi3` goal).
//!
//! Unlike QEMU virt, real Raspberry Pi 3 hardware needs GPIO pin muxing
//! before its UART is reachable (`gpio.rs`) and has no GIC, so interrupt
//! routing goes through the legacy BCM2835 controller plus the per-core
//! "ARM local" block instead (`intc.rs`).

pub mod gpio;
pub mod intc;
pub mod mailbox;
pub mod hdmi;
pub mod fbconsole;
pub mod usb;
mod font5x7;

use crate::memory::mmap::{MemoryMap, RegionKind};

/// BCM2837 peripherals (UART, GPIO, legacy interrupt controller, ...) sit
/// at this physical base, distinct from the "ARM local"/QA7 block, which
/// is always at the fixed physical address `0x4000_0000` regardless.
pub const PERIPHERAL_BASE: usize = 0x3F00_0000;

/// Fase 1 hardcodes a conservative RAM size instead of querying the real
/// amount via a VideoCore mailbox call, real mailbox support (and FDT
/// parsing, since the Pi 3 firmware can also hand off a DTB) is Fase 2
/// work, matching the same hardcode-for-now approach `qemu_virt_aarch64`
/// and `riscv64::fdt`'s fallback path already take.
pub fn parse_memory_map() -> MemoryMap {
    let mut map = MemoryMap::empty();
    map.add(RAM_BASE, RAM_FALLBACK_SIZE, RegionKind::Usable);
    map
}

pub const RAM_BASE: usize = 0x0;
pub const RAM_FALLBACK_SIZE: usize = 128 * 1024 * 1024;

/// Physical MMIO ranges to map as Device memory; everything else up to
/// `MAPPED_END` is mapped Normal (RAM). Both ranges below fall inside the
/// same first GiB as RAM itself, the reason `arch::aarch64::paging` maps
/// at 2 MiB granularity instead of one 1 GiB block like `qemu_virt_aarch64`
/// can get away with.
pub const MMIO_RANGES: &[(usize, usize)] = &[
    (PERIPHERAL_BASE, 0x0100_0000), // 0x3F00_0000-0x3FFF_FFFF: UART, GPIO, legacy IC, ...
    (0x4000_0000, 0x0000_1000),     // ARM local / QA7 block
];
pub const MAPPED_END: usize = 0x8000_0000; // first 2 GiB

/// Zero-sized marker used only to anchor the `hal::*` trait impls below.
#[allow(dead_code)]
pub struct Board;

impl crate::hal::Console for Board {
    fn init() { crate::arch::aarch64::console::init(); }
    fn write_byte(b: u8) { crate::arch::aarch64::console::print_byte(b); }
}

impl crate::hal::Timer for Board {
    fn init() { crate::drivers::timer::init(); }
    fn rearm() { crate::drivers::timer::rearm(); }
}

impl crate::hal::InterruptController for Board {
    fn init() { intc::init(); }
    fn enable(id: u32) { intc::enable(id); }
    fn disable(id: u32) { intc::disable(id); }
    fn eoi(id: u32) { intc::eoi(id); }
}
