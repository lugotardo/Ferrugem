/// Bitmap physical-memory manager.
/// Each bit represents one 4 KiB frame: 0 = free, 1 = used.
/// Initialized from the parsed memory map; kernel image region is marked used.

use crate::memory::mmap::{MemoryMap, RegionKind};

// Support up to 4 GiB / 4 KiB = 1 M frames.
const MAX_FRAMES: usize = 1 << 20;

static mut BITMAP: [u64; MAX_FRAMES / 64] = [0u64; MAX_FRAMES / 64];
static mut MEM_BASE:     usize = 0;
static mut TOTAL_FRAMES: usize = 0;
static mut FREE_FRAMES:  usize = 0;

unsafe extern "C" { static _kernel_end: u8; }

pub fn init_from_mmap(mmap: &MemoryMap) {
    // Pick the largest usable region at or above 1 MiB (skip BIOS-reserved low RAM).
    #[cfg(target_arch = "x86_64")]
    let min_base: usize = 0x10_0000;
    #[cfg(target_arch = "riscv64")]
    let min_base: usize = 0x8000_0000;
    #[cfg(target_arch = "aarch64")]
    let min_base: usize = 0x4000_0000;

    let region = match mmap.largest_usable(min_base) {
        Some(r) => r,
        None    => { init_fallback(); return; }
    };

    let frames = (region.size / 4096).min(MAX_FRAMES);
    unsafe {
        MEM_BASE     = region.base;
        TOTAL_FRAMES = frames;
        // All frames start free (bitmap is BSS-zeroed).
        FREE_FRAMES  = frames;

        // Mark frames occupied by the kernel image as used.
        let kend = &_kernel_end as *const u8 as usize;
        if kend > region.base {
            let used_frames = ((kend - region.base) + 4095) / 4096;
            let used_frames = used_frames.min(frames);
            for f in 0..used_frames { mark_used(f); }
        }
    }
}

fn init_fallback() {
    #[cfg(target_arch = "x86_64")]
    unsafe { MEM_BASE = 0x10_0000; TOTAL_FRAMES = (127 * 1024 * 1024) / 4096; FREE_FRAMES = TOTAL_FRAMES; }
    #[cfg(target_arch = "riscv64")]
    unsafe { MEM_BASE = 0x8000_0000; TOTAL_FRAMES = (128 * 1024 * 1024) / 4096; FREE_FRAMES = TOTAL_FRAMES; }
    #[cfg(target_arch = "aarch64")]
    unsafe { MEM_BASE = 0x4000_0000; TOTAL_FRAMES = (128 * 1024 * 1024) / 4096; FREE_FRAMES = TOTAL_FRAMES; }
}

fn mark_used(frame: usize) {
    unsafe {
        let w = frame / 64;
        let b = frame % 64;
        if BITMAP[w] & (1 << b) == 0 {
            BITMAP[w] |= 1 << b;
            FREE_FRAMES = FREE_FRAMES.saturating_sub(1);
        }
    }
}

/// Allocate a single 4 KiB frame. Returns frame index or `None`.
pub fn alloc_frame() -> Option<usize> {
    alloc_frames(1)
}

/// Allocate `n` **contiguous** frames. Returns the first frame index or `None`.
pub fn alloc_frames(n: usize) -> Option<usize> {
    if n == 0 { return Some(0); }
    unsafe {
        let total = TOTAL_FRAMES;
        let mut run_start = 0usize;
        let mut run_len   = 0usize;
        let mut f = 0usize;
        while f < total {
            // Fast-skip fully-used words
            let word = f / 64;
            if BITMAP[word] == u64::MAX {
                run_start = (word + 1) * 64;
                run_len   = 0;
                f = run_start;
                continue;
            }
            if BITMAP[word] & (1 << (f % 64)) == 0 {
                if run_len == 0 { run_start = f; }
                run_len += 1;
                if run_len == n {
                    for i in 0..n { mark_used(run_start + i); }
                    return Some(run_start);
                }
            } else {
                run_len = 0;
            }
            f += 1;
        }
        None
    }
}

pub fn free_frame(frame: usize) {
    free_frames(frame, 1);
}

pub fn free_frames(first: usize, n: usize) {
    unsafe {
        for i in 0..n {
            let f = first + i;
            if f < TOTAL_FRAMES {
                let w = f / 64;
                let b = f % 64;
                if BITMAP[w] & (1 << b) != 0 {
                    BITMAP[w] &= !(1u64 << b);
                    FREE_FRAMES += 1;
                }
            }
        }
    }
}

/// Convert frame index → physical address.
pub fn frame_to_addr(frame: usize) -> usize {
    unsafe { MEM_BASE + frame * 4096 }
}

/// Convert physical address → frame index (panics if below MEM_BASE).
pub fn addr_to_frame(addr: usize) -> usize {
    unsafe { (addr - MEM_BASE) / 4096 }
}

pub fn free_count()  -> usize { unsafe { FREE_FRAMES  } }
pub fn total_count() -> usize { unsafe { TOTAL_FRAMES } }
pub fn mem_base()    -> usize { unsafe { MEM_BASE     } }
