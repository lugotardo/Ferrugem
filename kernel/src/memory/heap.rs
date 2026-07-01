/// Kernel heap slab-backed GlobalAlloc with live-usage statistics.
///
/// alloc/dealloc delegate to `slab::{alloc,dealloc}`.
/// The stats counters give the shell `mem` command something to display.

use core::alloc::{GlobalAlloc, Layout};

// Exposed so memory::heap_stats() can fill HeapStats::total.
pub const HEAP_SIZE: usize = 0; // dynamic no fixed pool; reported as 0

static mut HEAP_USED:   usize = 0;
static mut HEAP_ALLOCS: usize = 0;

pub fn init() {
    // Slab caches are initialised by slab::init() called after this.
}

pub fn used_bytes()  -> usize { unsafe { HEAP_USED   } }
pub fn alloc_count() -> usize { unsafe { HEAP_ALLOCS } }

pub fn free_bytes() -> usize {
    // Dynamic report available physical frames as a proxy.
    crate::memory::bitmap::free_count() * 4096
}

// ── GlobalAlloc impl ──────────────────────────────────────────────────────────

struct KernelHeap;

unsafe impl GlobalAlloc for KernelHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // Satisfy alignment by bumping size to the next aligned size class.
        let size = layout.size().max(layout.align());
        let ptr  = crate::memory::slab::alloc(size);
        if !ptr.is_null() {
            HEAP_USED   += size;
            HEAP_ALLOCS += 1;
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let size = layout.size().max(layout.align());
        HEAP_USED   = HEAP_USED.saturating_sub(size);
        HEAP_ALLOCS = HEAP_ALLOCS.saturating_sub(1);
        crate::memory::slab::dealloc(ptr, size);
    }
}

#[global_allocator]
static ALLOCATOR: KernelHeap = KernelHeap;

#[alloc_error_handler]
fn alloc_error(layout: Layout) -> ! {
    panic!("alloc error: size={} align={}", layout.size(), layout.align());
}
