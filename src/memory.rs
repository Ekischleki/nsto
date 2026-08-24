use std::range::Range;

pub trait MemoryRegion {
    fn region_range(&self) -> Range<usize>;
    fn write(&mut self, rel_idx: usize, data: u8);
    fn read(&mut self, rel_idx: usize) -> u8;
}

pub struct InternalRam {
    data: [u8; 0x800],
}

impl Default for InternalRam {
    fn default() -> Self {
        Self { data: [0; 0x800] }
    }
}

impl InternalRam {
    pub fn justify_idx(idx: usize) -> usize {
        //Equal to idx % 0x800. This is because RAM gets mirrored
        idx & 0x7FF
    }
}

impl MemoryRegion for InternalRam {
    fn region_range(&self) -> Range<usize> {
        (0..0x2000).into()
    }

    fn write(&mut self, idx: usize, data: u8) {
        self.data[Self::justify_idx(idx)] = data;
    }

    fn read(&mut self, idx: usize) -> u8 {
        self.data[Self::justify_idx(idx)]
    }
}

pub struct DummyMem {
    range: Range<usize>,
}

impl DummyMem {
    pub fn new(range: Range<usize>) -> Self {
        Self { range }
    }
}

impl MemoryRegion for DummyMem {
    fn region_range(&self) -> Range<usize> {
        self.range
    }

    fn write(&mut self, _rel_idx: usize, _data: u8) {}

    fn read(&mut self, _rel_idx: usize) -> u8 {
        0
    }
}

pub struct Memory<const MEMORY_SIZE: usize> {
    regions: Box<[Box<dyn MemoryRegion>]>,
    map: [usize; MEMORY_SIZE],
}

impl<const MEMORY_SIZE: usize> Memory<MEMORY_SIZE> {
    pub fn construct(mut regions: Vec<Box<dyn MemoryRegion>>) -> Result<Self, MemoryError> {
        let mut map = [0_usize; MEMORY_SIZE];
        regions.sort_by_key(|region| region.region_range().start);
        let mut current_i = 0;
        for (region_idx, region) in regions.iter().enumerate() {
            let range = region.region_range();
            for i in range {
                map[i] = region_idx;
            }
            if range.start != current_i {
                return Err(MemoryError::InvalidRegionCoverage);
            }
            current_i = range.end;
        }
        if current_i != MEMORY_SIZE {
            return Err(MemoryError::InvalidRegionCoverage);
        }

        Ok(Self {
            map,
            regions: regions.into_boxed_slice(),
        })
    }

    fn find_region_idx_of<'a>(&self, idx: usize) -> usize {
        self.map[idx] as usize
    }

    pub fn read(&mut self, addr: usize) -> u8 {
        let region_idx = self.find_region_idx_of(addr);
        let region = &mut self.regions[region_idx];
        let range = region.region_range();
        region.read(addr - range.start)
    }

    pub fn write(&mut self, addr: usize, data: u8) {
        let region_idx = self.find_region_idx_of(addr);
        let region = &mut self.regions[region_idx];
        let range = region.region_range();
        region.write(addr - range.start, data);
    }
}

#[derive(Debug)]
pub enum MemoryError {
    InvalidRegionCoverage,
}
