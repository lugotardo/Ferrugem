pub mod bitmap;
pub mod bump;
pub mod heap;
pub mod mmap;
pub mod slab;

use bump::BumpAllocator;
use mmap::MemoryMap;

// ── early bump allocator (before bitmap PMM is ready) ────────────────────────

// Will be reset to start just after the kernel image once _kernel_end is known.
static mut BUMP: BumpAllocator = BumpAllocator::new(0x0010_0000, 0x0800_0000);

// Set to true after bitmap::init_from_mmap completes.
static mut BITMAP_READY: bool = false;

pub fn init() {
    // 1. Parse the hardware memory map.
    let mmap: MemoryMap = crate::arch::parse_memory_map();

    // 2. Initialise the bitmap PMM from the real map (marks kernel image used).
    bitmap::init_from_mmap(&mmap);
    unsafe { BITMAP_READY = true; }

    // 3. Reset bump allocator to start right after the kernel image so that
    //    any pre-bitmap early allocs don't overlap the image.
    unsafe extern "C" { static _kernel_end: u8; }
    unsafe {
        let kend  = (&_kernel_end as *const u8 as usize + 4095) & !4095;
        let limit = bitmap::mem_base() + bitmap::total_count() * 4096;
        BUMP = BumpAllocator::new(kend, limit);
    }

    // 4. Heap / slab init (uses alloc_pages internally).
    heap::init();
    slab::init();

    // 5. Arch paging.
    crate::arch::paging_init();
}

// ── page allocator (public API) ───────────────────────────────────────────────

/// Allocate `pages` contiguous 4 KiB physical frames.
/// Returns physical address (= virtual address with identity map) or `None`.
pub fn alloc_pages(pages: usize) -> Option<usize> {
    unsafe {
        if BITMAP_READY {
            let frame = bitmap::alloc_frames(pages)?;
            Some(bitmap::frame_to_addr(frame))
        } else {
            BUMP.alloc(pages * 4096)
        }
    }
}

/// Return `pages` contiguous frames starting at `addr` to the bitmap PMM.
/// No-op for bump-allocated memory (those frames are never reclaimed).
pub fn free_pages(addr: usize, pages: usize) {
    unsafe {
        if BITMAP_READY {
            let first = bitmap::addr_to_frame(addr);
            bitmap::free_frames(first, pages);
        }
    }
}

// ── heap stats ────────────────────────────────────────────────────────────────

pub struct HeapStats {
    pub used:   usize,
    pub free:   usize,
    pub total:  usize,
    pub allocs: usize,
}

pub fn heap_stats() -> HeapStats {
    HeapStats {
        used:   heap::used_bytes(),
        free:   heap::free_bytes(),
        total:  heap::HEAP_SIZE,
        allocs: heap::alloc_count(),
    }
}
