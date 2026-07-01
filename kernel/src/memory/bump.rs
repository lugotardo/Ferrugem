/// Simple bump allocator allocates upward, never frees.
/// Used during early boot before the bitmap PMM is ready.

pub struct BumpAllocator {
    start:   usize,
    end:     usize,
    current: usize,
}

impl BumpAllocator {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end, current: start }
    }

    pub fn reset(&mut self) {
        self.current = self.start;
    }

    /// Allocate `size` bytes aligned to 4096.
    pub fn alloc(&mut self, size: usize) -> Option<usize> {
        let aligned = (self.current + 4095) & !4095;
        let next = aligned.checked_add(size)?;
        if next > self.end {
            return None;
        }
        self.current = next;
        Some(aligned)
    }

    pub fn used(&self) -> usize {
        self.current - self.start
    }
}
