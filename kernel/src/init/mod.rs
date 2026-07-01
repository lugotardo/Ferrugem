use crate::arch;
use crate::drivers;
use crate::fs;
use crate::memory;
use crate::scheduler;
use crate::shell;
use crate::userspace;

pub fn kernel_main() -> ! {
    arch::early_init();
    memory::init();
    arch::interrupts_init();
    drivers::init();
    fs::init();
    scheduler::init();
    scheduler::spawn_fn(shell::run);
    #[cfg(target_arch = "x86_64")]
    scheduler::spawn_user(&userspace::HELLO_USER);
    #[cfg(target_arch = "riscv64")]
    scheduler::spawn_user(&userspace::HELLO_USER_RV64);
    loop { arch::halt(); }
}
