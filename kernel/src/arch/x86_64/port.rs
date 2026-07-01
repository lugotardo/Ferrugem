/// Raw I/O port access x86_64 only.
///
/// Safety: caller must ensure port and value are valid for the hardware.
#[inline]
pub unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!(
        "out dx, al",
        in("dx") port,
        in("al") val,
        options(nomem, nostack, preserves_flags)
    );
}

#[inline]
pub unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    core::arch::asm!(
        "in al, dx",
        out("al") val,
        in("dx") port,
        options(nomem, nostack, preserves_flags)
    );
    val
}

#[inline]
pub unsafe fn outw(port: u16, val: u16) {
    core::arch::asm!(
        "out dx, ax",
        in("dx") port,
        in("ax") val,
        options(nomem, nostack, preserves_flags)
    );
}

#[inline]
pub unsafe fn inw(port: u16) -> u16 {
    let val: u16;
    core::arch::asm!(
        "in ax, dx",
        out("ax") val,
        in("dx") port,
        options(nomem, nostack, preserves_flags)
    );
    val
}
