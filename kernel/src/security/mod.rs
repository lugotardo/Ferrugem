/// Security subsystem ASLR seed, capability bits, NX enforcement (v0.2+).

/// Simple capability bitmask for a process.
pub type Caps = u64;

pub const CAP_ROOT:       Caps = 1 << 0;
pub const CAP_KILL:       Caps = 1 << 1;
pub const CAP_NET_BIND:   Caps = 1 << 2;
pub const CAP_SYS_ADMIN:  Caps = 1 << 3;
pub const CAP_FS_WRITE:   Caps = 1 << 4;

pub fn check(caps: Caps, required: Caps) -> bool {
    caps & required == required
}

/// ASLR: generate a random page-aligned offset in the user address space.
pub fn aslr_offset() -> usize {
    (crate::arch::entropy_seed() & 0x0000_7FFF_FFFF_F000) as usize
}
