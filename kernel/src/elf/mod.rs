/// Minimal ELF64 loader. Parses PT_LOAD segments and loads them into a process
/// page table via `crate::arch::map_user_page`.

const ELFMAG:     [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELFCLASS64: u8      = 2;
const ELFDATA2LSB: u8     = 1;
const ET_DYN:     u16     = 3;  // Position-independent executable (PIE)
const PT_LOAD:    u32     = 1;
const PF_X:       u32     = 1;
const PF_W:       u32     = 2;
const PF_R:       u32     = 4;

// ELF64 header byte offsets
const EI_CLASS:      usize = 4;
const EI_DATA:       usize = 5;
const E_TYPE_OFF:    usize = 16;
const E_ENTRY_OFF:   usize = 24;
const E_PHOFF_OFF:   usize = 32;
const E_PHENTSIZE_OFF: usize = 54;
const E_PHNUM_OFF:   usize = 56;
const EHDR_MIN_SIZE: usize = 64;

// ELF64 program header byte offsets
const PH_TYPE_OFF:   usize = 0;
const PH_FLAGS_OFF:  usize = 4;
const PH_OFFSET_OFF: usize = 8;
const PH_VADDR_OFF:  usize = 16;
const PH_FILESZ_OFF: usize = 32;
const PH_MEMSZ_OFF:  usize = 40;
const PHDR_MIN_SIZE: usize = 56;

#[inline]
fn rd16(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([d[o], d[o + 1]])
}

#[inline]
fn rd32(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

#[inline]
fn rd64(d: &[u8], o: usize) -> u64 {
    u64::from_le_bytes([
        d[o],     d[o+1], d[o+2], d[o+3],
        d[o+4],   d[o+5], d[o+6], d[o+7],
    ])
}

/// Returns true if `data` looks like a valid 64-bit little-endian ELF.
pub fn is_elf(data: &[u8]) -> bool {
    data.len() >= EHDR_MIN_SIZE
        && data[..4] == ELFMAG
        && data[EI_CLASS] == ELFCLASS64
        && data[EI_DATA] == ELFDATA2LSB
}

/// Load ELF64 segments from `data` into the address space rooted at `pt_phys`.
///
/// Supports ET_EXEC (absolute VAs must be in the user range) and ET_DYN (PIE):
/// ET_DYN ELFs are relocated so their lowest PT_LOAD page lands at `USER_BASE_VA`.
///
/// Returns `(entry_va, heap_start_va)` on success where `heap_start_va` is the
/// first page-aligned VA past the last loaded segment (suitable as initial brk).
/// Returns `None` on any error (bad ELF, OOM, VA conflict, out-of-bounds data).
pub fn load(data: &[u8], pt_phys: u64) -> Option<(usize, usize)> {
    if !is_elf(data) { return None; }

    let etype     = rd16(data, E_TYPE_OFF);
    let entry     = rd64(data, E_ENTRY_OFF) as usize;
    let phoff     = rd64(data, E_PHOFF_OFF) as usize;
    let phentsize = rd16(data, E_PHENTSIZE_OFF) as usize;
    let phnum     = rd16(data, E_PHNUM_OFF) as usize;

    if phentsize < PHDR_MIN_SIZE || phnum == 0 { return None; }
    if phoff.checked_add(phnum.checked_mul(phentsize)?)? > data.len() { return None; }

    // Compute the lowest page-aligned PT_LOAD vaddr (used for the load bias).
    let mut min_va = usize::MAX;
    for i in 0..phnum {
        let ph = phoff + i * phentsize;
        if rd32(data, ph + PH_TYPE_OFF) == PT_LOAD {
            let va = rd64(data, ph + PH_VADDR_OFF) as usize & !0xFFF;
            if va < min_va { min_va = va; }
        }
    }
    if min_va == usize::MAX { return None; }

    // ET_DYN: relocate lowest PT_LOAD page to USER_BASE_VA.
    // ET_EXEC: keep absolute VAs (bias = 0; caller's ELF must target user range).
    let bias: isize = if etype == ET_DYN {
        crate::arch::USER_BASE_VA as isize - min_va as isize
    } else {
        0
    };

    // Map each PT_LOAD segment; track the highest mapped VA for heap_start.
    let mut max_va: usize = 0;

    for i in 0..phnum {
        let ph = phoff + i * phentsize;
        if rd32(data, ph + PH_TYPE_OFF) != PT_LOAD { continue; }

        let p_vaddr  = rd64(data, ph + PH_VADDR_OFF)  as usize;
        let p_filesz = rd64(data, ph + PH_FILESZ_OFF) as usize;
        let p_memsz  = rd64(data, ph + PH_MEMSZ_OFF)  as usize;
        let p_offset = rd64(data, ph + PH_OFFSET_OFF) as usize;
        let p_flags  = rd32(data, ph + PH_FLAGS_OFF);

        if p_memsz == 0 { continue; }
        if p_offset.saturating_add(p_filesz) > data.len() { return None; }

        let seg_va     = (p_vaddr as isize + bias) as usize;
        let page_start = seg_va & !0xFFF;
        let page_end   = seg_va.checked_add(p_memsz)?.checked_add(0xFFF)? & !0xFFF;
        let n_pages    = (page_end - page_start) / 4096;
        let prot       = pf_to_prot(p_flags);

        if page_end > max_va { max_va = page_end; }

        for pi in 0..n_pages {
            let page_va   = page_start + pi * 4096;
            let page_phys = crate::memory::alloc_pages(1)?;
            unsafe { core::ptr::write_bytes(page_phys as *mut u8, 0, 4096); }

            // Copy the portion of file data that falls within this page.
            let copy_va_lo = page_va.max(seg_va);
            let copy_va_hi = (page_va + 4096).min(seg_va + p_filesz);
            if copy_va_lo < copy_va_hi {
                let dst_byte_off = copy_va_lo - page_va;
                let src_byte_off = p_offset + (copy_va_lo - seg_va);
                let len          = copy_va_hi - copy_va_lo;
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        data.as_ptr().add(src_byte_off),
                        (page_phys + dst_byte_off) as *mut u8,
                        len,
                    );
                }
            }

            if !crate::arch::map_user_page(pt_phys, page_va, page_phys, prot) {
                return None;
            }
        }
    }

    let final_entry = (entry as isize + bias) as usize;
    Some((final_entry, max_va))
}

fn pf_to_prot(pf: u32) -> u32 {
    let mut p = crate::arch::PROT_USER;
    if pf & PF_R != 0 { p |= crate::arch::PROT_READ; }
    if pf & PF_W != 0 { p |= crate::arch::PROT_WRITE; }
    if pf & PF_X != 0 { p |= crate::arch::PROT_EXEC; }
    p
}
