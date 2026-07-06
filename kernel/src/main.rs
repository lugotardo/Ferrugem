#![no_std]
#![no_main]
#![cfg_attr(target_arch = "x86_64", feature(abi_x86_interrupt))]
#![feature(alloc_error_handler)]
#![feature(naked_functions)]
// In-kernel unit test harness (see src/testing.rs); only active under
// `cargo test`, where `_start` runs the collected `#[test_case]`s instead
// of `init::kernel_main()`.
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, test_runner(crate::testing::test_runner))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]
// Rust 2024: these two lints are deny-by-default; suppress until each module
// is hardened with UnsafeCell / explicit unsafe blocks.
#![allow(static_mut_refs)]
#![allow(unsafe_op_in_unsafe_fn)]

extern crate alloc;

mod arch;
mod boards;
mod drivers;
mod elf;
mod version;
mod fs;
mod hal;
mod init;
mod ipc;
mod memory;
mod process;
mod scheduler;
mod security;
mod shell;
mod syscall;
#[cfg(test)]
mod testing;
mod userspace;
mod vfs;

#[cfg(feature = "net")]
mod net;

use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    #[cfg(test)]
    {
        // Enough bring-up for tests to use the console and the heap
        // (`alloc`), skipping drivers/fs/scheduler/shell entirely.
        arch::early_init();
        memory::init();
        test_main();
        arch::halt();
    }
    #[cfg(not(test))]
    {
        init::kernel_main()
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    arch::console_print_str("\n[KERNEL PANIC] ");
    if let Some(msg) = info.message().as_str() {
        arch::console_print_str(msg);
    }
    arch::console_print_str("\n");
    arch::halt()
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    testing::test_panic(info)
}
