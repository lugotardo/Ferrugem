/// Minimal FDT (Flattened Device Tree) parser extracts physical memory regions.
///
/// Only walks the structure block looking for a `/memory` node and reads its
/// `reg` property.  All FDT values are big-endian.

use crate::memory::mmap::{MemoryMap, RegionKind};

unsafe extern "C" {
    // Saved by boot.s before _start is called.
    static riscv_fdt_ptr: usize;
}

const FDT_MAGIC:      u32 = 0xd00dfeed;
const FDT_BEGIN_NODE: u32 = 1;
const FDT_END_NODE:   u32 = 2;
const FDT_PROP:       u32 = 3;
const FDT_NOP:        u32 = 4;
const FDT_END:        u32 = 9;

// ── byte helpers ─────────────────────────────────────────────────────────────

#[inline]
unsafe fn be32_at(p: *const u8) -> u32 {
    u32::from_be_bytes(*(p as *const [u8; 4]))
}

#[inline]
unsafe fn be64_at(p: *const u8) -> u64 {
    u64::from_be_bytes(*(p as *const [u8; 8]))
}

// ── main parser ──────────────────────────────────────────────────────────────

pub fn parse_memory_map() -> MemoryMap {
    let mut map = MemoryMap::empty();
    unsafe {
        let fdt_base = riscv_fdt_ptr;
        if fdt_base == 0 || be32_at(fdt_base as *const u8) != FDT_MAGIC {
            fallback(&mut map);
            return map;
        }

        let hdr        = fdt_base as *const u8;
        let off_struct = be32_at(hdr.add(8))  as usize;
        let off_str    = be32_at(hdr.add(12)) as usize;
        let strings    = hdr.add(off_str);

        // Walk the structure block
        let mut cur   = hdr.add(off_struct) as *const u32;
        let mut depth = 0usize;
        // Address/size cells from the root node (defaults for QEMU virt)
        let mut addr_cells: u32 = 2;
        let mut size_cells: u32 = 2;
        let mut in_memory = false;

        loop {
            let token = u32::from_be(*cur);
            cur = cur.add(1);

            match token {
                FDT_BEGIN_NODE => {
                    // Node name is a null-terminated string at `cur`, aligned to 4.
                    let name = cur as *const u8;
                    let mut nlen = 0usize;
                    while *name.add(nlen) != 0 { nlen += 1; }
                    let aligned = (nlen + 1 + 3) & !3;
                    cur = (name.add(aligned)) as *const u32;

                    let name_bytes = core::slice::from_raw_parts(name, nlen);
                    in_memory = depth == 1
                        && (name_bytes == b"memory"
                            || name_bytes.starts_with(b"memory@"));
                    depth += 1;
                }
                FDT_END_NODE => {
                    depth = depth.saturating_sub(1);
                    in_memory = false;
                }
                FDT_PROP => {
                    let prop_len = u32::from_be(*cur) as usize; cur = cur.add(1);
                    let name_off = u32::from_be(*cur) as usize; cur = cur.add(1);
                    let data = cur as *const u8;
                    let aligned = (prop_len + 3) & !3;
                    cur = data.add(aligned) as *const u32;

                    // Property name from strings block
                    let pname = cstr_bytes(strings.add(name_off));

                    if depth == 1 {
                        if pname == b"#address-cells" && prop_len == 4 {
                            addr_cells = be32_at(data);
                        } else if pname == b"#size-cells" && prop_len == 4 {
                            size_cells = be32_at(data);
                        }
                    }

                    if in_memory && pname == b"reg" {
                        parse_reg(&mut map, data, prop_len, addr_cells, size_cells);
                    }
                }
                FDT_NOP => {}
                FDT_END | _ => break,
            }
        }
    }
    if map.count == 0 { fallback(&mut map); }
    map
}

// ── helpers ──────────────────────────────────────────────────────────────────

unsafe fn parse_reg(
    map:        &mut MemoryMap,
    data:       *const u8,
    len:        usize,
    addr_cells: u32,
    size_cells: u32,
) {
    let entry_bytes = (addr_cells + size_cells) as usize * 4;
    if entry_bytes == 0 { return; }
    let n = len / entry_bytes;
    for i in 0..n {
        let off  = i * entry_bytes;
        let base = read_cells(data.add(off),                         addr_cells);
        let size = read_cells(data.add(off + addr_cells as usize * 4), size_cells);
        map.add(base as usize, size as usize, RegionKind::Usable);
    }
}

unsafe fn read_cells(p: *const u8, cells: u32) -> u64 {
    match cells {
        1 => be32_at(p) as u64,
        2 => be64_at(p),
        _ => be32_at(p) as u64, // fallback
    }
}

unsafe fn cstr_bytes(p: *const u8) -> &'static [u8] {
    let mut len = 0;
    while *p.add(len) != 0 { len += 1; }
    core::slice::from_raw_parts(p, len)
}

fn fallback(map: &mut MemoryMap) {
    // QEMU virt default: 128 MiB at 0x80000000
    map.add(0x8000_0000, 128 * 1024 * 1024, RegionKind::Usable);
}
