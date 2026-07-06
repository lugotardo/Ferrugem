//! In-kernel unit test harness, wired up only for `cargo test` builds (see
//! the `#![cfg_attr(test, ...)]` attributes and the `_start` split in
//! `main.rs`). Each `#[test_case]` runs with `arch::early_init()` +
//! `memory::init()` already done (console + heap available), but no
//! drivers/fs/scheduler — it's meant for logic that doesn't need a fully
//! booted kernel (allocators, VFS, parsers, ...).
//!
//! Pass/fail is reported to the host by writing to QEMU's isa-debug-exit
//! device, which is x86_64 PC-only; `make test-x86` / `scripts/
//! qemu-test-runner.sh` is the only board wired up to run these today.

use crate::arch;
use core::panic::PanicInfo;

#[repr(u32)]
enum QemuExitCode {
    Success = 0x10,
    Failed = 0x11,
}

fn exit_qemu(code: QemuExitCode) -> ! {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        arch::x86_64::port::outl(0xf4, code as u32);
    }
    // Only x86_64 has isa-debug-exit wired up (see module docs); other
    // arches just halt so the QEMU process can be killed by the runner.
    arch::halt()
}

/// A single `#[test_case]` item. Blanket-implemented for any `Fn()`, which
/// is what a bare `fn some_test() { ... }` item is.
pub trait TestCase {
    fn run(&self);
}

impl<T: Fn()> TestCase for T {
    fn run(&self) {
        arch::console_print_str(core::any::type_name::<T>());
        arch::console_print_str(" ... ");
        self();
        arch::console_print_str("ok\n");
    }
}

pub fn test_runner(tests: &[&dyn TestCase]) {
    arch::console_print_str("running tests\n");
    for test in tests {
        test.run();
    }
    exit_qemu(QemuExitCode::Success);
}

/// Used as the `#[cfg(test)]` `#[panic_handler]` in main.rs: a failing
/// assertion inside a `#[test_case]` panics, which must fail the QEMU
/// process instead of hanging in `arch::halt()` forever.
pub fn test_panic(info: &PanicInfo) -> ! {
    arch::console_print_str("FAILED\n");
    arch::console_print_str("panic: ");
    if let Some(msg) = info.message().as_str() {
        arch::console_print_str(msg);
    }
    arch::console_print_str("\n");
    exit_qemu(QemuExitCode::Failed)
}

#[test_case]
fn trivial_assertion() {
    assert_eq!(1 + 1, 2);
}
