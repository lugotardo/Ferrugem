/// RISC-V SBI timer, arms the supervisor timer via OpenSBI.

use crate::arch::riscv64::sbi;

pub fn init() {
    let time: u64;
    unsafe {
        core::arch::asm!("csrr {}, time", out(reg) time, options(nostack));
    }
    sbi::set_timer(time + 100_000);
    // Enable supervisor timer interrupt (STIE = bit 5)
    unsafe {
        core::arch::asm!(
            "li t0, 0x20",
            "csrs sie, t0",
            out("t0") _,
            options(nostack)
        );
    }
}
