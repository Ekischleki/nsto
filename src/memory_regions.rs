use crate::memory::MemoryRegion;

pub struct RwRam<const WIDTH: usize> {
    mem: [u8; WIDTH],
    start: usize,
    mapped_width: usize,
}

impl<const WIDTH: usize> RwRam<WIDTH> {
    pub fn with_mirrored_width(start: usize, mapped_width: usize) -> Self {
        Self {
            mem: [0; WIDTH],
            start,
            mapped_width,
        }
    }

    fn justify_address(&self, idx: usize) -> usize {
        idx % WIDTH
    }
}

impl<const WIDTH: usize> MemoryRegion for RwRam<WIDTH> {
    fn region_range(&self) -> std::range::Range<usize> {
        (self.start..(self.start + self.mapped_width)).into()
    }

    fn write(&mut self, rel_idx: usize, data: u8) {
        self.mem[self.justify_address(rel_idx)] = data;
    }

    fn read(&mut self, rel_idx: usize) -> u8 {
        self.mem[self.justify_address(rel_idx)]
    }
}

pub struct Rom<const PHYSICAL_SIZE: usize> {
    mem: [u8; PHYSICAL_SIZE],
    start: usize,
    mapped_width: usize,
}

impl<const PHYSICAL_SIZE: usize> Rom<PHYSICAL_SIZE> {
    pub fn with_mirrored_width(
        start: usize,
        mapped_width: usize,
        mem: [u8; PHYSICAL_SIZE],
    ) -> Self {
        Self {
            mem,
            start,
            mapped_width,
        }
    }

    fn justify_address(&self, idx: usize) -> usize {
        idx % PHYSICAL_SIZE
    }
}

impl<const WIDTH: usize> MemoryRegion for Rom<WIDTH> {
    fn region_range(&self) -> std::range::Range<usize> {
        (self.start..(self.start + self.mapped_width)).into()
    }

    fn write(&mut self, _: usize, _: u8) {}

    fn read(&mut self, rel_idx: usize) -> u8 {
        self.mem[self.justify_address(rel_idx)]
    }
}
