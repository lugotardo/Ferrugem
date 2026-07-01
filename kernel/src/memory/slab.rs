/// Slab allocator fixed-size caches for small kernel allocations.
///
/// Size classes (bytes): 8 16 32 64 128 256 512 1024 2048
/// Each cache is a singly-linked free list.  When empty, a new 4 KiB slab is
/// carved from the frame allocator.  Allocations larger than 2048 bytes go
/// directly to the frame allocator (rounded up to whole pages).
///
/// `dealloc` uses the layout size to find the right cache no per-object
/// metadata needed.

const CLASSES: [usize; 9] = [8, 16, 32, 64, 128, 256, 512, 1024, 2048];
const SLAB_PAGES: usize = 1; // one 4 KiB slab per refill
const SLAB_SIZE:  usize = SLAB_PAGES * 4096;

struct Cache {
    head:       *mut usize, // free-list (each free block stores next ptr at offset 0)
    block_size: usize,
}

unsafe impl Send for Cache {}
unsafe impl Sync for Cache {}

impl Cache {
    const fn empty() -> Self { Cache { head: core::ptr::null_mut(), block_size: 0 } }
}

static mut CACHES: [Cache; 9] = [const { Cache::empty() }; 9];

pub fn init() {
    unsafe {
        for (i, &sz) in CLASSES.iter().enumerate() {
            CACHES[i].block_size = sz;
        }
    }
}

fn class_index(size: usize) -> Option<usize> {
    CLASSES.iter().position(|&s| s >= size)
}

fn grow(idx: usize) -> bool {
    let block_size = unsafe { CACHES[idx].block_size };
    let slab = match crate::memory::alloc_pages(SLAB_PAGES) {
        Some(a) => a as *mut u8,
        None    => return false,
    };
    let n = SLAB_SIZE / block_size;
    unsafe {
        for i in 0..n {
            let block = slab.add(i * block_size) as *mut usize;
            // Link to the next block (or existing head for the last one).
            let next = if i + 1 < n {
                slab.add((i + 1) * block_size) as *mut usize
            } else {
                CACHES[idx].head
            };
            *block = next as usize;
        }
        CACHES[idx].head = slab as *mut usize;
    }
    true
}

/// Allocate `size` bytes.  Returns null on OOM.
pub fn alloc(size: usize) -> *mut u8 {
    let size = size.max(core::mem::size_of::<usize>()); // need room for free-list ptr
    match class_index(size) {
        None => {
            // Large: allocate whole pages directly.
            let pages = (size + 4095) / 4096;
            crate::memory::alloc_pages(pages).unwrap_or(0) as *mut u8
        }
        Some(idx) => unsafe {
            if CACHES[idx].head.is_null() && !grow(idx) {
                return core::ptr::null_mut();
            }
            let block        = CACHES[idx].head;
            CACHES[idx].head = *block as *mut usize;
            block as *mut u8
        },
    }
}

/// Return `ptr` (previously allocated with `alloc(size)`) to the appropriate cache.
pub fn dealloc(ptr: *mut u8, size: usize) {
    if ptr.is_null() { return; }
    let size = size.max(core::mem::size_of::<usize>());
    match class_index(size) {
        None => {
            let pages = (size + 4095) / 4096;
            crate::memory::free_pages(ptr as usize, pages);
        }
        Some(idx) => unsafe {
            let block    = ptr as *mut usize;
            *block       = CACHES[idx].head as usize;
            CACHES[idx].head = block;
        },
    }
}
