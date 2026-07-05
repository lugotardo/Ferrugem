//! BSP: aarch64, QEMU `virt` machine (`-machine virt,gic-version=2`).
//!
//! GICv2 interrupt routing (`gic.rs`); UART/timer/memory-map primitives are
//! shared aarch64 SoC-generic code in `arch::aarch64` (PL011 + ARM generic
//! timer are the same IP blocks on every board this kernel targets so far —
//! only their base addresses/IRQ routing differ, which is why they aren't
//! duplicated per board the way x86_64's PC platform files are).

pub mod gic;
// Cross-board neutral name `arch::aarch64` re-exports and dispatches
// through, see the comment on that re-export for why.
pub use gic as intc;

use crate::memory::mmap::{MemoryMap, RegionKind};

/// Fase 1 hardcodes the QEMU virt default (128 MiB at 0x4000_0000) instead
/// of parsing the DTB `arch::aarch64::boot::aarch64_fdt_ptr` stashed at
/// boot, real FDT parsing (same approach as `riscv64::fdt`) is Fase 2 work.
pub fn parse_memory_map() -> MemoryMap {
    let mut map = MemoryMap::empty();
    map.add(0x4000_0000, 128 * 1024 * 1024, RegionKind::Usable);
    map
}

/// Lowest usable RAM address and the Fase-1 fallback size, consumed by
/// `memory::bitmap` when no better memory map is available.
pub const RAM_BASE: usize = 0x4000_0000;
pub const RAM_FALLBACK_SIZE: usize = 128 * 1024 * 1024;

/// Physical MMIO ranges to map as Device memory; everything else up to
/// `MAPPED_END` is mapped Normal (RAM). QEMU virt puts the GICv2, PL011,
/// and flash all below RAM, so the whole first GiB is Device.
pub const MMIO_RANGES: &[(usize, usize)] = &[(0x0000_0000, 0x4000_0000)];
pub const MAPPED_END: usize = 0x1_0000_0000; // first 4 GiB

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
    fn init() { gic::init(); }
    fn enable(id: u32) { gic::enable(id); }
    fn disable(id: u32) { gic::disable(id); }
    fn eoi(id: u32) { gic::eoi(id); }
}
