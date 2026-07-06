//! Shared BSP pieces for `boards::qemu_pc` and `boards::virtualbox`: both are
//! plain IBM-PC-compatible machines (Multiboot boot, VGA+COM1 console, 8259
//! PIC, e820 memory map) and, until they actually diverge, kept every one of
//! these files byte-for-byte identical in each board's own directory. Living
//! here once instead means a change no longer has to be copy-pasted twice to
//! stay in sync; each board re-exports these modules under its own name (see
//! `qemu_pc::console`/`virtualbox::console` etc.) so nothing calling
//! `boards::current::console` needs to know this module exists.
//!
//! `linker.ld` (not a Rust module, wired up via `build.rs::linker_dir`
//! instead) was identical too and moved here for the same reason. `boot.s`
//! stays per-board: VirtualBox's `boot.s` already diverged from `qemu_pc`'s
//! (2 MiB paging instead of one 1 GiB huge page, to keep the legacy VGA/BIOS
//! MMIO hole out of a huge "RAM" mapping VT-x won't tolerate).

pub mod console;
pub mod multiboot;
pub mod pic;
