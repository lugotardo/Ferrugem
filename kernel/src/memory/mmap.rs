/// Arch-neutral physical memory map, populated by e820 (x86_64) or FDT (RISC-V).

pub const MAX_REGIONS: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RegionKind { Usable, Reserved }

#[derive(Clone, Copy)]
pub struct MemoryRegion {
    pub base: usize,
    pub size: usize,
    pub kind: RegionKind,
}

impl MemoryRegion {
    pub const fn zero() -> Self {
        Self { base: 0, size: 0, kind: RegionKind::Reserved }
    }
    pub fn end(&self) -> usize { self.base.saturating_add(self.size) }
}

pub struct MemoryMap {
    pub regions: [MemoryRegion; MAX_REGIONS],
    pub count:   usize,
}

impl MemoryMap {
    pub const fn empty() -> Self {
        Self { regions: [const { MemoryRegion::zero() }; MAX_REGIONS], count: 0 }
    }

    pub fn add(&mut self, base: usize, size: usize, kind: RegionKind) {
        if size > 0 && self.count < MAX_REGIONS {
            self.regions[self.count] = MemoryRegion { base, size, kind };
            self.count += 1;
        }
    }

    pub fn iter(&self) -> &[MemoryRegion] { &self.regions[..self.count] }

    /// Largest contiguous usable region with base >= `min_base`.
    pub fn largest_usable(&self, min_base: usize) -> Option<MemoryRegion> {
        let mut best: Option<MemoryRegion> = None;
        for r in self.iter() {
            if r.kind != RegionKind::Usable { continue; }
            let base = if r.base < min_base { min_base } else { r.base };
            if base >= r.end() { continue; }
            let size = r.end() - base;
            let trimmed = MemoryRegion { base, size, kind: RegionKind::Usable };
            if best.map_or(true, |b| size > b.size) { best = Some(trimmed); }
        }
        best
    }
}
