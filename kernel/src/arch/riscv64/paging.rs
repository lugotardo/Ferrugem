/// RISC-V Sv39 paging: identity map first 4 GiB (kernel S-mode) + per-process
/// user address spaces in L2[4] (VA 0x1_0000_0000+).

const PAGE_V: u64 = 1 << 0;
const PAGE_R: u64 = 1 << 1;
const PAGE_W: u64 = 1 << 2;
const PAGE_X: u64 = 1 << 3;
const PAGE_U: u64 = 1 << 4;
const PAGE_A: u64 = 1 << 6;
const PAGE_D: u64 = 1 << 7;

#[repr(C, align(4096))]
pub struct PageTable {
    pub entries: [u64; 512],
}

impl PageTable {
    pub const fn empty() -> Self { Self { entries: [0u64; 512] } }
}

// Root (L2) page table for the kernel identity map.
static mut ROOT: PageTable = PageTable::empty();

pub fn init() {
    unsafe {
        // Identity map 4 × 1 GiB using L2 huge-page leaves (no PAGE_U).
        for i in 0..4usize {
            let ppn = (i as u64) << 18;
            ROOT.entries[i] =
                (ppn << 10) | PAGE_V | PAGE_R | PAGE_W | PAGE_X | PAGE_A | PAGE_D;
        }
        let ppn = (ROOT.entries.as_ptr() as u64) >> 12;
        let satp = (8u64 << 60) | ppn;
        core::arch::asm!("csrw satp, {}", in(reg) satp, options(nostack));
        core::arch::asm!("sfence.vma zero, zero", options(nostack));
    }
}

// ── User virtual address layout ──────────────────────────────────────────────
// L2[4] covers VA 0x1_0000_0000 – 0x1_3FFF_FFFF (1 GiB).
// Code at L2[4]→L1[0]→L0[1], stack at L0[15].
pub const USER_CODE_VA:   usize = 0x1_0000_1000; // L2[4], L1[0], L0[1]
pub const USER_STACK_TOP: usize = 0x1_0001_0000; // RSP starts here (grows down)

const USER_STACK_PAGE_VA: usize = USER_STACK_TOP - 4096; // → L0[15]
const USER_CODE_L0_IDX:   usize = (USER_CODE_VA >> 12) & 511;       // 1
const USER_STACK_L0_IDX:  usize = (USER_STACK_PAGE_VA >> 12) & 511; // 15

// ── Guard pages ──────────────────────────────────────────────────────────────

/// RISC-V guard-page support: canary only (full MMU split is expensive and not
/// needed for the stack-overflow detector).
pub fn protect_guard_page(_phys: usize) -> bool { false }

// ── General-purpose page mapper ──────────────────────────────────────────────

/// Map a single 4 KiB page at `va` → `pa` inside the Sv39 page table rooted
/// at `root_phys`.  `va` must be in L2[4..] (the user range); calls with
/// L2[0..4] (kernel huge-page range) return false immediately.
/// `prot` is a bitmask of `crate::arch::PROT_*` flags.
/// Intermediate L1/L0 page tables are allocated on demand.
pub fn map_user_page(root_phys: u64, va: usize, pa: usize, prot: u32) -> bool {
    let vpn2 = (va >> 30) & 0x1FF;
    if vpn2 < 4 { return false; }    // kernel-only range
    let vpn1 = (va >> 21) & 0x1FF;
    let vpn0 = (va >> 12) & 0x1FF;

    let mut flags = PAGE_V | PAGE_A | PAGE_D;
    if prot & crate::arch::PROT_READ  != 0 { flags |= PAGE_R; }
    if prot & crate::arch::PROT_WRITE != 0 { flags |= PAGE_W; }
    if prot & crate::arch::PROT_EXEC  != 0 { flags |= PAGE_X; }
    if prot & crate::arch::PROT_USER  != 0 { flags |= PAGE_U; }

    unsafe {
        let root = root_phys as *mut u64;

        // Walk / create L2 → L1
        let l1_phys = {
            let e = root.add(vpn2).read();
            if e & PAGE_V != 0 {
                if e & (PAGE_R | PAGE_W | PAGE_X) != 0 { return false; } // leaf (huge page)
                ((e >> 10) << 12) as usize
            } else {
                let new = match crate::memory::alloc_pages(1) {
                    Some(x) => x, None => return false,
                };
                core::ptr::write_bytes(new as *mut u8, 0, 4096);
                root.add(vpn2).write(((new as u64) >> 12 << 10) | PAGE_V);
                new
            }
        };

        // Walk / create L1 → L0
        let l1 = l1_phys as *mut u64;
        let l0_phys = {
            let e = l1.add(vpn1).read();
            if e & PAGE_V != 0 {
                if e & (PAGE_R | PAGE_W | PAGE_X) != 0 { return false; } // leaf (huge page)
                ((e >> 10) << 12) as usize
            } else {
                let new = match crate::memory::alloc_pages(1) {
                    Some(x) => x, None => return false,
                };
                core::ptr::write_bytes(new as *mut u8, 0, 4096);
                l1.add(vpn1).write(((new as u64) >> 12 << 10) | PAGE_V);
                new
            }
        };

        // Write L0 leaf entry
        let l0 = l0_phys as *mut u64;
        l0.add(vpn0).write(((pa as u64) >> 12 << 10) | flags);
    }
    true
}

// ── Per-process address space ────────────────────────────────────────────────

/// Create an empty Sv39 address space: allocates only the L2 root, copies the
/// four kernel huge-page entries (L2[0..4], no PAGE_U), and leaves L2[4..511]
/// zeroed.  `map_user_page` then populates the user range on demand.
pub fn create_empty_process_page_table() -> Option<u64> {
    unsafe {
        let root_phys = crate::memory::alloc_pages(1)?;
        core::ptr::write_bytes(root_phys as *mut u8, 0, 4096);
        let root = root_phys as *mut u64;
        for i in 0..4usize {
            *root.add(i) = ROOT.entries[i]; // shared kernel huge pages, no PAGE_U
        }
        Some(root_phys as u64)
    }
}

/// Create an isolated Sv39 address space for one user process.
///
/// Allocates root (L2) + L1 + L0 (3 frames).  Kernel entries ROOT[0..4] are
/// copied without PAGE_U.  User code and stack pages land in L2[4]/L1[0]/L0[1]
/// and L0[15] with PAGE_U set.
///
/// Returns the physical address of the new root table, or `None` on OOM.
pub fn create_process_page_table(code_phys: usize, stack_phys: usize) -> Option<u64> {
    unsafe {
        let root_phys = crate::memory::alloc_pages(1)?;
        let l1_phys   = crate::memory::alloc_pages(1)?;
        let l0_phys   = crate::memory::alloc_pages(1)?;

        core::ptr::write_bytes(root_phys as *mut u8, 0, 4096);
        core::ptr::write_bytes(l1_phys   as *mut u8, 0, 4096);
        core::ptr::write_bytes(l0_phys   as *mut u8, 0, 4096);

        let root = root_phys as *mut u64;
        let l1   = l1_phys   as *mut u64;
        let l0   = l0_phys   as *mut u64;

        // Share kernel identity map (L2[0..4]) — no PAGE_U.
        for i in 0..4usize {
            *root.add(i) = ROOT.entries[i];
        }

        // L2[4] → per-process L1 (non-leaf, no RWX).
        *root.add(4) = ((l1_phys as u64) >> 12 << 10) | PAGE_V;

        // L1[0] → L0 (non-leaf).
        *l1.add(0) = ((l0_phys as u64) >> 12 << 10) | PAGE_V;

        // L0 leaves: code (RWX+U) and stack (RW+U).
        let code_ppn  = (code_phys  as u64) >> 12;
        let stack_ppn = (stack_phys as u64) >> 12;
        *l0.add(USER_CODE_L0_IDX)  =
            (code_ppn  << 10) | PAGE_V | PAGE_R | PAGE_W | PAGE_X | PAGE_U | PAGE_A | PAGE_D;
        *l0.add(USER_STACK_L0_IDX) =
            (stack_ppn << 10) | PAGE_V | PAGE_R | PAGE_W          | PAGE_U | PAGE_A | PAGE_D;

        Some(root_phys as u64)
    }
}

/// Deep-copy an Sv39 user address space.
///
/// Kernel entries (L2[0..4]) are shared verbatim.  All user pages (L2[4..])
/// get fresh physical frames with copied contents.  Intermediate tables
/// (L1, L0) are also freshly allocated.  Huge pages (rare in user space)
/// are shared rather than split.
///
/// Returns the physical address of the new root, or `None` on OOM.
pub fn clone_user_page_table(src_phys: u64) -> Option<u64> {
    unsafe {
        let new_root_phys = crate::memory::alloc_pages(1)?;
        core::ptr::write_bytes(new_root_phys as *mut u8, 0, 4096);
        let src_root = src_phys as *const u64;
        let new_root = new_root_phys as *mut u64;

        // Kernel entries shared verbatim
        for i in 0..4usize { *new_root.add(i) = *src_root.add(i); }

        for vpn2 in 4..512usize {
            let e2 = *src_root.add(vpn2);
            if e2 & PAGE_V == 0 { continue; }
            // 1 GiB huge page — share physical
            if e2 & (PAGE_R | PAGE_W | PAGE_X) != 0 { *new_root.add(vpn2) = e2; continue; }

            let src_l1_phys = ((e2 >> 10) << 12) as usize;
            let new_l1_phys = crate::memory::alloc_pages(1)?;
            core::ptr::write_bytes(new_l1_phys as *mut u8, 0, 4096);
            let src_l1 = src_l1_phys as *const u64;
            let new_l1 = new_l1_phys as *mut u64;

            for vpn1 in 0..512usize {
                let e1 = *src_l1.add(vpn1);
                if e1 & PAGE_V == 0 { continue; }
                // 2 MiB huge page — share
                if e1 & (PAGE_R | PAGE_W | PAGE_X) != 0 { *new_l1.add(vpn1) = e1; continue; }

                let src_l0_phys = ((e1 >> 10) << 12) as usize;
                let new_l0_phys = crate::memory::alloc_pages(1)?;
                core::ptr::write_bytes(new_l0_phys as *mut u8, 0, 4096);
                let src_l0 = src_l0_phys as *const u64;
                let new_l0 = new_l0_phys as *mut u64;

                for vpn0 in 0..512usize {
                    let e0 = *src_l0.add(vpn0);
                    if e0 & PAGE_V == 0 { continue; }
                    let src_data = ((e0 >> 10) << 12) as usize;
                    let new_data = crate::memory::alloc_pages(1)?;
                    core::ptr::copy_nonoverlapping(src_data as *const u8, new_data as *mut u8, 4096);
                    let flags = e0 & 0x3FF; // V R W X U G A D RSW (low 10 bits)
                    *new_l0.add(vpn0) = ((new_data as u64) >> 12 << 10) | flags;
                }
                *new_l1.add(vpn1) = ((new_l0_phys as u64) >> 12 << 10) | PAGE_V;
            }
            *new_root.add(vpn2) = ((new_l1_phys as u64) >> 12 << 10) | PAGE_V;
        }
        Some(new_root_phys as u64)
    }
}

/// Physical address of the kernel's root page table.
pub fn kernel_page_table_phys() -> u64 {
    unsafe { ROOT.entries.as_ptr() as u64 }
}

/// Switch to an Sv39 address space (write SATP + sfence.vma).
pub fn switch_address_space(root_phys: u64) {
    unsafe {
        let ppn  = root_phys >> 12;
        let satp = (8u64 << 60) | ppn;
        core::arch::asm!("csrw satp, {}", in(reg) satp, options(nostack));
        core::arch::asm!("sfence.vma zero, zero", options(nostack));
    }
}
