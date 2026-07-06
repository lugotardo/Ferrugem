/// Static identity-mapped page table (aarch64, Fase 1).
///
/// Two-level table, 39-bit VA space (T0SZ=25): L1 holds one table
/// descriptor per 1 GiB region, each pointing at an L2 table of 2 MiB block
/// descriptors, the same "coarse identity map, no per-process tables yet"
/// approach as `riscv64::paging`'s L2 huge-page kernel map, just one level
/// finer than a single 1 GiB block per region. That extra granularity is
/// needed because some boards (Raspberry Pi 3) interleave MMIO and RAM
/// within the same GiB, which one 1 GiB block descriptor can't express.
/// Real per-process page tables (4 KiB granularity, `map_user_page`, etc.)
/// are Fase 2 work: nothing in Fase 1 ever spawns an EL0 process, so those
/// functions are never actually called.

use crate::boards::current::{MAPPED_END, MMIO_RANGES};

const NUM_L1: usize = MAPPED_END / (1 << 30); // 1 GiB per L1 entry

#[repr(align(4096))]
struct L1Table([u64; 512]);
#[repr(align(4096))]
struct L2Table([u64; 512]);

static mut L1_TABLE: L1Table = L1Table([0; 512]);
static mut L2_TABLES: [L2Table; NUM_L1] = [const { L2Table([0; 512]) }; NUM_L1];

const ATTR_DEVICE:    u64 = 0; // MAIR index 0: Device-nGnRnE (GIC, UART, ...)
const ATTR_NORMAL:    u64 = 1; // MAIR index 1: Normal, Inner/Outer Write-Back
const ATTR_NORMAL_NC: u64 = 2; // MAIR index 2: Normal, Inner/Outer Non-cacheable

const BLOCK_AF:       u64 = 1 << 10; // access flag (required: no HW AF management)
const BLOCK_SH_INNER: u64 = 0b11 << 8;
const BLOCK_VALID:    u64 = 0b01;    // level-2 block descriptor
const TABLE_VALID:    u64 = 0b11;    // level-1 table descriptor

fn is_device(addr: usize) -> bool {
    MMIO_RANGES.iter().any(|&(base, size)| addr >= base && addr < base + size)
}

fn l2_block_entry(addr: usize) -> u64 {
    let attr_idx = if is_device(addr) { ATTR_DEVICE } else { ATTR_NORMAL };
    (addr as u64) | BLOCK_AF | BLOCK_SH_INNER | (attr_idx << 2) | BLOCK_VALID
}

pub fn init() {
    unsafe {
        // Board-supplied `MMIO_RANGES` decides Device vs Normal per 2 MiB
        // block; everything in `[0, MAPPED_END)` outside those ranges is
        // identity-mapped as cacheable RAM.
        for l1 in 0..NUM_L1 {
            let l2 = &mut L2_TABLES[l1];
            for l2i in 0..512 {
                let addr = (l1 << 30) | (l2i << 21);
                l2.0[l2i] = l2_block_entry(addr);
            }
            let l2_phys = core::ptr::addr_of!(l2.0) as u64;
            L1_TABLE.0[l1] = l2_phys | TABLE_VALID;
        }
        // Ensure the page-table stores above are visible to the table walker
        // before it's ever turned loose on them (ISB only orders instruction
        // fetch/execution against system-register writes, it says nothing
        // about when a prior normal-memory store becomes visible to another
        // observer such as the MMU's translation table walker).
        core::arch::asm!("dsb ishst", options(nostack));

        let mair: u64 = (0x44u64 << 16) | (0xFFu64 << 8) | 0x00u64; // idx2=Normal NC, idx1=Normal WB, idx0=Device-nGnRnE
        core::arch::asm!("msr mair_el1, {v}", v = in(reg) mair, options(nostack));

        // T0SZ=25 (39-bit VA), 4 KiB granule, inner shareable, WBWA, 32-bit PA.
        // EPD1 (bit 23) = 1: disable TTBR1_EL1 walks — TTBR1_EL1 is never
        // initialized (its reset value is architecturally UNKNOWN), and
        // nothing in this Fase-1 kernel generates a high-half (bit 63 set)
        // address, so a walk through it must never be attempted.
        let tcr: u64 = 25
            | (0b01 << 8)     // IRGN0 = WBWA
            | (0b01 << 10)    // ORGN0 = WBWA
            | (0b11 << 12)    // SH0   = inner shareable
            | (0b00 << 14)    // TG0   = 4 KiB granule
            | (1u64 << 23)    // EPD1  = 1 (disable TTBR1 walks)
            | (0b000u64 << 32); // IPS = 32-bit PA (plenty for QEMU virt's <4 GiB layout)
        core::arch::asm!("msr tcr_el1, {v}", v = in(reg) tcr, options(nostack));

        let ttbr0 = core::ptr::addr_of!(L1_TABLE.0) as u64;
        core::arch::asm!("msr ttbr0_el1, {v}", v = in(reg) ttbr0, options(nostack));
        core::arch::asm!("isb", options(nostack));

        // Clear any TLB entries a prior boot stage/firmware may have left
        // behind — AArch64 gives no guarantee the TLB is clean at reset or
        // across an EL transition, and this is the first time this table is
        // ever active.
        core::arch::asm!("tlbi vmalle1", "dsb ish", "isb", options(nostack));

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

/// Reprogram the already-mapped 2 MiB block(s) covering `[phys, phys+size)`
/// as Normal Non-cacheable and flush the TLB for that range.
///
/// For hardware buffers whose physical address is only known at runtime —
/// today, the sole caller is `boards::raspberrypi3::hdmi`, whose
/// VideoCore-allocated address can't be listed in the board's compile-time
/// `MMIO_RANGES`. Non-cacheable (rather than Device) so ordinary
/// `core::ptr::copy`/`write_bytes` can still be used on it: Device memory
/// forbids the unaligned/wide accesses a compiler-generated memcpy may
/// emit, while Normal Non-cacheable only drops caching, not addressing
/// mode, writes still land in RAM immediately, visible to the GPU without
/// any CPU-side cache maintenance.
pub fn map_uncached(phys: usize, size: usize) {
    unsafe {
        let start = phys & !((1 << 21) - 1);
        let end = (phys + size + (1 << 21) - 1) & !((1 << 21) - 1);
        let mut addr = start;
        while addr < end {
            let l1 = addr >> 30;
            if l1 >= NUM_L1 {
                break;
            }
            let l2i = (addr >> 21) & 0x1FF;
            L2_TABLES[l1].0[l2i] =
                (addr as u64) | BLOCK_AF | BLOCK_SH_INNER | (ATTR_NORMAL_NC << 2) | BLOCK_VALID;
            addr += 1 << 21;
        }
        core::arch::asm!("dsb ishst", "tlbi vmalle1is", "dsb ish", "isb", options(nostack));
    }
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
/// tables, dynamic frame allocation, and EL0 AP-bit handling, none of
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

/// No fine-grained (4 KiB) kernel mappings yet, the static 2 MiB identity
/// map can't unmap a single page. Callers fall back to canary-only guard
/// protection, exactly as documented on `arch::protect_guard_page`.
pub fn protect_guard_page(_phys: usize) -> bool {
    false
}
