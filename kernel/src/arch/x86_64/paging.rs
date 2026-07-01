/// x86_64 4-level paging: identity map first 4 GiB (kernel-only) + per-process
/// user address spaces in PML4[1] (VA 0x0000_0080_0000_0000+).

const PAGE_PRESENT:  u64 = 1 << 0;
const PAGE_WRITABLE: u64 = 1 << 1;
const PAGE_USER:     u64 = 1 << 2;
const PAGE_HUGE:     u64 = 1 << 7;

#[repr(C, align(4096))]
pub struct PageTable {
    pub entries: [u64; 512],
}

impl PageTable {
    pub const fn empty() -> Self { Self { entries: [0u64; 512] } }
}

// Kernel page tables: identity-map first 4 GiB using 2 MiB huge pages (ring-0 only).
static mut PML4: PageTable = PageTable::empty();
static mut PDPT: PageTable = PageTable::empty();
static mut PD:   [PageTable; 4] = [const { PageTable::empty() }; 4];

// ── User virtual address layout ─────────────────────────────────────────────
// PML4[1] covers VA 0x0000_0080_0000_0000 – 0x0000_00FF_FFFF_FFFF (512 GiB).
// We place user code at the first available page and the stack one region up.
pub const USER_CODE_VA:   usize = 0x0000_0080_0000_1000; // PML4[1], PDPT[0], PD[0], PT[1]
pub const USER_STACK_TOP: usize = 0x0000_0080_0001_0000; // RSP starts here (grows down)

const USER_STACK_PAGE_VA: usize = USER_STACK_TOP - 4096; // page at PT[15]
const USER_CODE_PT_IDX:   usize = (USER_CODE_VA >> 12) & 511;       // 1
const USER_STACK_PT_IDX:  usize = (USER_STACK_PAGE_VA >> 12) & 511; // 15

pub fn init() {
    unsafe {
        // Kernel identity map: PML4[0] → PDPT → PD[0..3] (no PAGE_USER).
        PML4.entries[0] = (PDPT.entries.as_ptr() as u64) | PAGE_PRESENT | PAGE_WRITABLE;
        for i in 0..4usize {
            PDPT.entries[i] = (PD[i].entries.as_ptr() as u64) | PAGE_PRESENT | PAGE_WRITABLE;
        }
        for gib in 0..4usize {
            for mib in 0..512usize {
                let phys = ((gib as u64) << 30) | ((mib as u64) << 21);
                PD[gib].entries[mib] = phys | PAGE_PRESENT | PAGE_WRITABLE | PAGE_HUGE;
            }
        }
        let pml4_addr = PML4.entries.as_ptr() as u64;
        core::arch::asm!("mov cr3, {}", in(reg) pml4_addr, options(nostack));
    }
}

// ── Per-process address space ────────────────────────────────────────────────

/// Create an isolated address space for one user process.
///
/// Allocates a PML4 + PDPT + PD + PT (4 frames).  The kernel identity map is
/// shared via PML4[0] (same physical PDPT, no USER bit).  User code and stack
/// pages live in PML4[1] with all parent entries marked USER.
///
/// Returns the physical address of the new PML4, or `None` on OOM.
pub fn create_process_page_table(code_phys: usize, stack_phys: usize) -> Option<u64> {
    unsafe {
        let pml4_phys = crate::memory::alloc_pages(1)?;
        let pdpt_phys = crate::memory::alloc_pages(1)?;
        let pd_phys   = crate::memory::alloc_pages(1)?;
        let pt_phys   = crate::memory::alloc_pages(1)?;

        // Zero all four tables.
        core::ptr::write_bytes(pml4_phys as *mut u8, 0, 4096);
        core::ptr::write_bytes(pdpt_phys as *mut u8, 0, 4096);
        core::ptr::write_bytes(pd_phys   as *mut u8, 0, 4096);
        core::ptr::write_bytes(pt_phys   as *mut u8, 0, 4096);

        let pml4 = pml4_phys as *mut u64;
        let pdpt = pdpt_phys as *mut u64;
        let pd   = pd_phys   as *mut u64;
        let pt   = pt_phys   as *mut u64;

        // PML4[0] = kernel identity map (shared PDPT, explicitly no USER bit).
        *pml4.add(0) = PML4.entries[0] & !(PAGE_USER);

        // PML4[1] → per-process PDPT (USER=1).
        *pml4.add(1) = (pdpt_phys as u64) | PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;

        // PDPT[0] → PD (USER=1).
        *pdpt.add(0) = (pd_phys as u64) | PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;

        // PD[0] → PT (USER=1).
        *pd.add(0) = (pt_phys as u64) | PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;

        // PT entries: code and stack pages (USER=1, no NX since NXE not enabled).
        *pt.add(USER_CODE_PT_IDX)  = (code_phys as u64)  | PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;
        *pt.add(USER_STACK_PT_IDX) = (stack_phys as u64) | PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;

        Some(pml4_phys as u64)
    }
}

/// Physical address of the kernel's PML4 (used when switching back from user tasks).
pub fn kernel_page_table_phys() -> u64 {
    unsafe { PML4.entries.as_ptr() as u64 }
}

/// Load a page table root into CR3 (flushes TLB).
pub fn switch_address_space(pml4_phys: u64) {
    unsafe { core::arch::asm!("mov cr3, {}", in(reg) pml4_phys, options(nostack)); }
}

// ── General-purpose page mapper ──────────────────────────────────────────────

/// Create an empty x86_64 address space: allocates only the PML4, installs the
/// shared kernel identity PDPT at PML4[0] (no PAGE_USER), and leaves PML4[1..]
/// zeroed.  `map_user_page` then populates the user range (PML4[1..]) on demand.
pub fn create_empty_process_page_table() -> Option<u64> {
    unsafe {
        let pml4_phys = crate::memory::alloc_pages(1)?;
        core::ptr::write_bytes(pml4_phys as *mut u8, 0, 4096);
        let pml4 = pml4_phys as *mut u64;
        *pml4.add(0) = PML4.entries[0] & !(PAGE_USER);
        Some(pml4_phys as u64)
    }
}

/// Map a single 4 KiB page at `va` → `pa` inside the PML4 rooted at `pml4_phys`.
/// `va` must be in PML4[1..] (user range ≥ 0x80_0000_0000); PML4[0] returns false.
/// `prot` is a bitmask of `crate::arch::PROT_*` flags.
/// Intermediate PDPT/PD/PT tables are allocated on demand.
pub fn map_user_page(pml4_phys: u64, va: usize, pa: usize, prot: u32) -> bool {
    let pml4_idx = (va >> 39) & 511;
    if pml4_idx == 0 { return false; }
    let pdpt_idx = (va >> 30) & 511;
    let pd_idx   = (va >> 21) & 511;
    let pt_idx   = (va >> 12) & 511;

    let mut flags = PAGE_PRESENT;
    if prot & crate::arch::PROT_WRITE != 0 { flags |= PAGE_WRITABLE; }
    if prot & crate::arch::PROT_USER  != 0 { flags |= PAGE_USER; }

    unsafe {
        let pml4 = pml4_phys as *mut u64;

        let pdpt_phys = {
            let e = pml4.add(pml4_idx).read();
            if e & PAGE_PRESENT != 0 {
                (e & !0xFFF) as usize
            } else {
                let new = match crate::memory::alloc_pages(1) {
                    Some(x) => x, None => return false,
                };
                core::ptr::write_bytes(new as *mut u8, 0, 4096);
                pml4.add(pml4_idx).write(
                    (new as u64) | PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER,
                );
                new
            }
        };

        let pdpt = pdpt_phys as *mut u64;
        let pd_phys = {
            let e = pdpt.add(pdpt_idx).read();
            if e & PAGE_PRESENT != 0 {
                if e & PAGE_HUGE != 0 { return false; }
                (e & !0xFFF) as usize
            } else {
                let new = match crate::memory::alloc_pages(1) {
                    Some(x) => x, None => return false,
                };
                core::ptr::write_bytes(new as *mut u8, 0, 4096);
                pdpt.add(pdpt_idx).write(
                    (new as u64) | PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER,
                );
                new
            }
        };

        let pd = pd_phys as *mut u64;
        let pt_phys_val = {
            let e = pd.add(pd_idx).read();
            if e & PAGE_PRESENT != 0 {
                if e & PAGE_HUGE != 0 { return false; }
                (e & !0xFFF) as usize
            } else {
                let new = match crate::memory::alloc_pages(1) {
                    Some(x) => x, None => return false,
                };
                core::ptr::write_bytes(new as *mut u8, 0, 4096);
                pd.add(pd_idx).write(
                    (new as u64) | PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER,
                );
                new
            }
        };

        let pt = pt_phys_val as *mut u64;
        pt.add(pt_idx).write((pa as u64 & !0xFFF) | flags);
    }
    true
}

/// Deep-copy an x86_64 user address space.
///
/// PML4[0] (kernel identity) is shared verbatim.  All user pages (PML4[1..])
/// get fresh physical frames with copied contents.  Intermediate tables
/// (PDPT, PD, PT) are freshly allocated.  Huge pages are shared (not split).
///
/// Returns the physical address of the new PML4, or `None` on OOM.
pub fn clone_user_page_table(src_pml4_phys: u64) -> Option<u64> {
    unsafe {
        let new_pml4_phys = crate::memory::alloc_pages(1)?;
        core::ptr::write_bytes(new_pml4_phys as *mut u8, 0, 4096);
        let src_pml4 = src_pml4_phys as *const u64;
        let new_pml4 = new_pml4_phys as *mut u64;

        // Kernel entry shared
        *new_pml4.add(0) = *src_pml4.add(0);

        for i in 1..512usize {
            let e4 = *src_pml4.add(i);
            if e4 & PAGE_PRESENT == 0 { continue; }

            let src_pdpt = (e4 & !0xFFF) as *const u64;
            let new_pdpt_phys = crate::memory::alloc_pages(1)?;
            core::ptr::write_bytes(new_pdpt_phys as *mut u8, 0, 4096);
            let new_pdpt = new_pdpt_phys as *mut u64;

            for j in 0..512usize {
                let e3 = *src_pdpt.add(j);
                if e3 & PAGE_PRESENT == 0 { continue; }
                if e3 & PAGE_HUGE != 0 { *new_pdpt.add(j) = e3; continue; } // 1 GiB shared

                let src_pd = (e3 & !0xFFF) as *const u64;
                let new_pd_phys = crate::memory::alloc_pages(1)?;
                core::ptr::write_bytes(new_pd_phys as *mut u8, 0, 4096);
                let new_pd = new_pd_phys as *mut u64;

                for k in 0..512usize {
                    let e2 = *src_pd.add(k);
                    if e2 & PAGE_PRESENT == 0 { continue; }
                    if e2 & PAGE_HUGE != 0 { *new_pd.add(k) = e2; continue; } // 2 MiB shared

                    let src_pt = (e2 & !0xFFF) as *const u64;
                    let new_pt_phys = crate::memory::alloc_pages(1)?;
                    core::ptr::write_bytes(new_pt_phys as *mut u8, 0, 4096);
                    let new_pt = new_pt_phys as *mut u64;

                    for l in 0..512usize {
                        let e1 = *src_pt.add(l);
                        if e1 & PAGE_PRESENT == 0 { continue; }
                        let src_data = (e1 & !0xFFF) as usize;
                        let new_data = crate::memory::alloc_pages(1)?;
                        core::ptr::copy_nonoverlapping(src_data as *const u8, new_data as *mut u8, 4096);
                        *new_pt.add(l) = (new_data as u64 & !0xFFF) | (e1 & 0xFFF);
                    }
                    *new_pd.add(k) = (new_pt_phys as u64 & !0xFFF) | (e2 & 0xFFF);
                }
                *new_pdpt.add(j) = (new_pd_phys as u64 & !0xFFF) | (e3 & 0xFFF);
            }
            *new_pml4.add(i) = (new_pdpt_phys as u64 & !0xFFF) | (e4 & 0xFFF);
        }
        Some(new_pml4_phys as u64)
    }
}

// ── Guard pages ─────────────────────────────────────────────────────────────

/// Mark the 4 KiB page at `phys` (kernel identity map) as not-present.
/// Used for kernel-stack guard pages; operates on the shared kernel PDPT/PD.
pub fn protect_guard_page(phys: usize) -> bool {
    if phys >= 4 * 1024 * 1024 * 1024 { return false; }

    let gib    = (phys >> 30) & 3;
    let pd_idx = (phys >> 21) & 511;
    let pt_idx = (phys >> 12) & 511;

    unsafe {
        let pd_entry = &mut PD[gib].entries[pd_idx];

        if *pd_entry & PAGE_HUGE != 0 {
            let pt_phys = match crate::memory::alloc_pages(1) {
                Some(a) => a,
                None    => return false,
            };
            let pt = pt_phys as *mut u64;
            let huge_base = phys & !((2 << 20) - 1);
            for i in 0..512usize {
                *pt.add(i) = ((huge_base + i * 4096) as u64) | PAGE_PRESENT | PAGE_WRITABLE;
            }
            *pd_entry = (pt_phys as u64) | PAGE_PRESENT | PAGE_WRITABLE;
            reload_cr3();
        }

        let pt_base = (*pd_entry & !0xFFF) as *mut u64;
        *pt_base.add(pt_idx) = 0;
        core::arch::asm!("invlpg [{0}]", in(reg) phys, options(nostack));
    }
    true
}

#[inline]
unsafe fn reload_cr3() {
    let cr3: u64;
    core::arch::asm!("mov {0}, cr3", out(reg) cr3, options(nostack));
    core::arch::asm!("mov cr3, {0}", in(reg) cr3, options(nostack));
}
