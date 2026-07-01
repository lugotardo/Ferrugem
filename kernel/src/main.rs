#![no_std]
#![no_main]
#![cfg_attr(target_arch = "x86_64", feature(abi_x86_interrupt))]
#![feature(alloc_error_handler)]
#![feature(naked_functions)]
// Rust 2024: these two lints are deny-by-default; suppress until each module
// is hardened with UnsafeCell / explicit unsafe blocks.
#![allow(static_mut_refs)]
#![allow(unsafe_op_in_unsafe_fn)]

extern crate alloc;

mod arch;
mod drivers;
mod elf;
mod version;
mod fs;
mod init;
mod ipc;
mod memory;
mod process;
mod scheduler;
mod security;
mod shell;
mod syscall;
mod userspace;
mod vfs;

#[cfg(feature = "net")]
mod net;

use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    init::kernel_main()
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    arch::console_print_str("\n[KERNEL PANIC] ");
    if let Some(msg) = info.message().as_str() {
        arch::console_print_str(msg);
    }
    arch::console_print_str("\n");
    arch::halt()
}
