//! Board Support Packages (BSPs).
//!
//! Each board owns everything that is specific to one physical/virtual
//! platform (boot glue, linker script, peripheral base addresses, interrupt
//! routing, memory map) and implements the `hal` traits against the chip
//! drivers in `crate::drivers`. Exactly one board per ISA is selected at
//! compile time via a `board-*` Cargo feature (see `kernel/Cargo.toml`);
//! `current` re-exports whichever one is active so the rest of the kernel
//! never needs to know which board it's building for.
//!
//! Populated incrementally as each ISA is migrated off hardcoded
//! `target_arch` dispatch (see `doc/process` / project plan).

#[cfg(all(target_arch = "x86_64", feature = "board-qemu-pc"))]
pub mod qemu_pc;
#[cfg(all(target_arch = "x86_64", feature = "board-qemu-pc"))]
pub use qemu_pc as current;

#[cfg(all(target_arch = "x86_64", feature = "board-virtualbox"))]
pub mod virtualbox;
#[cfg(all(target_arch = "x86_64", feature = "board-virtualbox"))]
pub use virtualbox as current;

#[cfg(target_arch = "riscv64")]
pub mod qemu_virt_riscv64;
#[cfg(target_arch = "riscv64")]
pub use qemu_virt_riscv64 as current;

#[cfg(all(target_arch = "aarch64", feature = "board-qemu-virt-aarch64"))]
pub mod qemu_virt_aarch64;
#[cfg(all(target_arch = "aarch64", feature = "board-qemu-virt-aarch64"))]
pub use qemu_virt_aarch64 as current;

#[cfg(all(target_arch = "aarch64", feature = "board-raspberrypi3"))]
pub mod raspberrypi3;
#[cfg(all(target_arch = "aarch64", feature = "board-raspberrypi3"))]
pub use raspberrypi3 as current;
