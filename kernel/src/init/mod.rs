use crate::arch;
use crate::drivers;
use crate::fs;
use crate::memory;
use crate::scheduler;
use crate::shell;
use crate::userspace;

// Embedded musl-linked init binary (x86_64 static PIE, stripped).
// Built from userspace/init/ with `cargo build --target x86_64-unknown-linux-musl --release`.
// Spawned directly from kernel memory — too large for the ramfs (4 KiB limit per inode).
#[cfg(target_arch = "x86_64")]
static INIT_ELF: &[u8] = include_bytes!(
    "../../../userspace/init/target/x86_64-unknown-linux-musl/release/init"
);

pub fn kernel_main() -> ! {
    arch::early_init();
    memory::init();
    arch::interrupts_init();
    drivers::init();
    fs::init();
    scheduler::init();

    #[cfg(target_arch = "x86_64")]
    {
        if crate::elf::is_elf(INIT_ELF) {
            // A musl-linked userspace init is available: run it as PID 1.
            // The kernel shell is not spawned so stdin is not shared.
            scheduler::spawn_elf(INIT_ELF, "/init");
        } else {
            // No ELF init — fall back to the kernel built-in shell.
            scheduler::spawn_fn(shell::run);
            scheduler::spawn_user(&userspace::HELLO_USER);
        }
    }

    #[cfg(target_arch = "riscv64")]
    {
        scheduler::spawn_fn(shell::run);
        scheduler::spawn_user(&userspace::HELLO_USER_RV64);
    }

    // aarch64 Fase 1 is kernel-only (no EL0 userspace yet, see arch/aarch64/
    // paging.rs) — just the kernel shell, no spawn_user/spawn_elf.
    #[cfg(target_arch = "aarch64")]
    {
        scheduler::spawn_fn(shell::run);
    }

    arch::halt();
}
