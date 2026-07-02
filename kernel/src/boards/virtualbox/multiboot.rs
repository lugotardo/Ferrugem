/// Multiboot1 info-structure parser extracts the e820 memory map.
///
/// GRUB/QEMU sets EAX = 0x2BADB002 and EBX = pointer to the MBI struct.
/// boot.s saves both to `multiboot_magic` and `multiboot_info` (32-bit globals).

use crate::memory::mmap::{MemoryMap, RegionKind};

unsafe extern "C" {
    static multiboot_magic: u32;
    static multiboot_info:  u32; // 32-bit physical address of MBI
}

const MB_MAGIC: u32 = 0x2BADB002;

// MBI flags bits
const FLAG_MEM:  u32 = 1 << 0; // mem_lower / mem_upper valid
const FLAG_MMAP: u32 = 1 << 6; // mmap_length / mmap_addr valid

pub fn parse_memory_map() -> MemoryMap {
    let mut map = MemoryMap::empty();
    unsafe {
        if multiboot_magic != MB_MAGIC {
            fallback(&mut map);
            return map;
        }
        let base = multiboot_info as usize;
        let flags = read_u32(base);

        if flags & FLAG_MMAP != 0 {
            let mmap_len  = read_u32(base + 44) as usize;
            let mmap_addr = read_u32(base + 48) as usize;
            parse_mmap_entries(&mut map, mmap_addr, mmap_len);
        } else if flags & FLAG_MEM != 0 {
            // Only upper-memory info available (no full e820 table)
            let mem_upper_kb = read_u32(base + 8) as usize;
            map.add(0x10_0000, mem_upper_kb * 1024, RegionKind::Usable);
        } else {
            fallback(&mut map);
        }
    }
    map
}

fn parse_mmap_entries(map: &mut MemoryMap, mmap_addr: usize, mmap_len: usize) {
    let end = mmap_addr + mmap_len;
    let mut ptr = mmap_addr;
    while ptr + 4 <= end {
        // Each entry: [size:u32][base:u64][length:u64][type:u32]
        // `size` counts bytes after itself (typically 20).
        let entry_size = unsafe { read_u32(ptr) } as usize;
        if entry_size < 20 || ptr + 4 + entry_size > end { break; }

        let base = unsafe { read_u64(ptr + 4)  } as usize;
        let len  = unsafe { read_u64(ptr + 12) } as usize;
        let kind_raw = unsafe { read_u32(ptr + 20) };

        let kind = if kind_raw == 1 { RegionKind::Usable } else { RegionKind::Reserved };
        map.add(base, len, kind);

        ptr += 4 + entry_size;
    }
}

fn fallback(map: &mut MemoryMap) {
    // QEMU -kernel default: 127 MiB above 1 MiB
    map.add(0x10_0000, 127 * 1024 * 1024, RegionKind::Usable);
}

#[inline]
unsafe fn read_u32(addr: usize) -> u32 {
    core::ptr::read_unaligned(addr as *const u32)
}

#[inline]
unsafe fn read_u64(addr: usize) -> u64 {
    core::ptr::read_unaligned(addr as *const u64)
}
