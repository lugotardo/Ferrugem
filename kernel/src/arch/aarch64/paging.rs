/// Static identity-mapped page table (aarch64, Fase 1).
///
/// Single-level (L1) table of 1 GiB block descriptors, 39-bit VA space
/// (T0SZ=25) — the same "coarse identity map, no per-process tables yet"
/// approach as `riscv64::paging`'s L2 huge-page kernel map. Real
/// per-process page tables (4 KiB granularity, `map_user_page`, etc.) are
/// Fase 2 work: nothing in Fase 1 ever spawns an EL0 process, so those
/// functions are never actually called.

const NUM_BLOCKS: usize = 4; // identity-map the first 4 GiB (low MMIO + RAM)

#[repr(align(4096))]
struct L1Table([u64; 512]);

static mut L1_TABLE: L1Table = L1Table([0; 512]);

const ATTR_DEVICE: u64 = 0; // MAIR index 0: Device-nGnRnE (GIC, UART, ...)
const ATTR_NORMAL: u64 = 1; // MAIR index 1: Normal, Inner/Outer Write-Back

const BLOCK_AF:       u64 = 1 << 10; // access flag (required: no HW AF management)
const BLOCK_SH_INNER: u64 = 0b11 << 8;
const BLOCK_VALID:    u64 = 0b01;    // level-1 block descriptor

fn block_entry(gib_index: usize, attr_idx: u64) -> u64 {
    let phys = (gib_index as u64) << 30;
    phys | BLOCK_AF | BLOCK_SH_INNER | (attr_idx << 2) | BLOCK_VALID
}

pub fn init() {
    unsafe {
        // 0x0000_0000-0x3FFF_FFFF: GICv2, PL011, flash, and everything else
        // QEMU's virt board puts below RAM.
        L1_TABLE.0[0] = block_entry(0, ATTR_DEVICE);
        // 0x4000_0000 upward: RAM.
        for i in 1..NUM_BLOCKS {
            L1_TABLE.0[i] = block_entry(i, ATTR_NORMAL);
        }

        let mair: u64 = (0xFFu64 << 8) | 0x00u64; // idx1=Normal WB, idx0=Device-nGnRnE
        core::arch::asm!("msr mair_el1, {v}", v = in(reg) mair, options(nostack));

        // T0SZ=25 (39-bit VA), 4 KiB granule, inner shareable, WBWA, 32-bit PA.
        let tcr: u64 = 25
            | (0b01 << 8)     // IRGN0 = WBWA
            | (0b01 << 10)    // ORGN0 = WBWA
            | (0b11 << 12)    // SH0   = inner shareable
            | (0b00 << 14)    // TG0   = 4 KiB granule
            | (0b000u64 << 32); // IPS = 32-bit PA (plenty for QEMU virt's <4 GiB layout)
        core::arch::asm!("msr tcr_el1, {v}", v = in(reg) tcr, options(nostack));

        let ttbr0 = core::ptr::addr_of!(L1_TABLE.0) as u64;
        core::arch::asm!("msr ttbr0_el1, {v}", v = in(reg) ttbr0, options(nostack));
        core::arch::asm!("isb", options(nostack));

        // Enable MMU (M) + data cache (C) + instruction cache (I).
        let mut sctlr: u64;
        core::arch::asm!("mrs {v}, sctlr_el1", v = out(reg) sctlr, options(nostack));
        sctlr |= (1 << 0) | (1 << 2) | (1 << 12);
        core::arch::asm!("msr sctlr_el1, {v}", "isb", v = in(reg) sctlr, options(nostack));
    }
}

pub fn kernel_page_table_phys() -> u64 {
    unsafe { core::ptr::addr_of!(L1_TABLE.0) as u64 }
}

pub fn switch_address_space(pt_phys: u64) {
    unsafe {
        core::arch::asm!(
            "msr ttbr0_el1, {v}",
            "isb",
            "tlbi vmalle1is",
            "dsb ish",
            "isb",
            v = in(reg) pt_phys,
            options(nostack)
        );
    }
}

/// Fase 2 work: per-process page tables need 4 KiB-granularity L2/L3
/// tables, dynamic frame allocation, and EL0 AP-bit handling — none of
/// which exist yet since Fase 1 never creates an EL0 process.
pub fn create_process_page_table(_code_phys: usize, _stack_phys: usize) -> Option<u64> {
    unimplemented!("aarch64 fase 2: per-process page tables not implemented yet")
}

pub fn create_empty_process_page_table() -> Option<u64> {
    unimplemented!("aarch64 fase 2: per-process page tables not implemented yet")
}

pub fn clone_user_page_table(_src_phys: u64) -> Option<u64> {
    unimplemented!("aarch64 fase 2: per-process page tables not implemented yet")
}

pub fn map_user_page(_pt_phys: u64, _va: usize, _pa: usize, _prot: u32) -> bool {
    unimplemented!("aarch64 fase 2: per-process page tables not implemented yet")
}

/// No fine-grained (4 KiB) kernel mappings yet — the static 1 GiB identity
/// map can't unmap a single page. Callers fall back to canary-only guard
/// protection, exactly as documented on `arch::protect_guard_page`.
pub fn protect_guard_page(_phys: usize) -> bool {
    false
}
