/// Minimal SBI (Supervisor Binary Interface) call wrappers.
/// Uses ECALL to communicate with OpenSBI.

#[allow(dead_code)]
pub const EXT_CONSOLE: usize = 0x01;   // legacy putchar
pub const EXT_BASE:    usize = 0x10;
pub const EXT_TIME:    usize = 0x54494D45;
pub const EXT_IPI:     usize = 0x735049;
pub const EXT_HSM:     usize = 0x48534D;

pub struct SbiRet {
    pub error: isize,
    pub value: usize,
}

#[inline]
pub fn ecall(ext: usize, fid: usize, a0: usize, a1: usize, a2: usize) -> SbiRet {
    let error: isize;
    let value: usize;
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a7") ext,
            in("a6") fid,
            inlateout("a0") a0 => error,
            inlateout("a1") a1 => value,
            in("a2") a2,
            options(nostack)
        );
    }
    SbiRet { error, value }
}

/// Legacy console putchar (EXT 0x01)
pub fn putchar(c: u8) {
    ecall(EXT_CONSOLE, 0, c as usize, 0, 0);
}

/// Set next timer event (nanoseconds from now converted by OpenSBI)
pub fn set_timer(stime_val: u64) {
    ecall(EXT_TIME, 0, stime_val as usize, 0, 0);
}

/// Hart start (HSM extension)
pub fn hart_start(hart_id: usize, start_addr: usize, opaque: usize) -> isize {
    ecall(EXT_HSM, 0, hart_id, start_addr, opaque).error
}
