use std::{cell::RefCell, rc::Rc, thread::available_parallelism};

use crate::{
    cartridge::NametableMirroring,
    memory::{DummyMem, Memory, MemoryRegion},
    memory_regions::RwRam,
};

#[derive(Default)]
pub struct PPURegisters {
    ppuctrl: u8,
    ppumask: u8,
    ppustatus: u8,
    oamaddr: u8,
    oamdata: u8,
    wrote_oamdata: Option<u8>,
    ppuscroll: u16,
    ppuaddr: u16,
    ppudata: u8,
    wrote_ppudata: Option<u16>,
    oamdma: u8,
    write_oamdma: bool,
    t: Option<u8>,
}

impl PPURegisters {
    pub fn poll_write_oamdma(&mut self) -> Option<u8> {
        if self.write_oamdma {
            self.write_oamdma = false;
            Some(self.oamdma)
        } else {
            None
        }
    }
    pub fn cpu_access_ppudata(&mut self) {
        self.ppuaddr = self.ppuaddr.wrapping_add(self.vram_addr_incr()) & 0x3FFF
    }
    pub fn set_vblank_flag(&mut self, v: bool) {
        if self.vblank_flag() != v {
            self.ppustatus ^= 0x80;
        }
    }
    pub fn set_sprt_overflow_flag(&mut self, v: bool) {
        if self.sprite_overflow_flag() != v {
            self.ppustatus ^= 0x20;
        }
    }
    pub fn set_sprt_hit_flag(&mut self, v: bool) {
        if self.sprite_hit_flag() != v {
            self.ppustatus ^= 0x40;
        }
    }
    pub fn base_name_addr(&self) -> usize {
        (self.ppuctrl & 0x03) as usize
    }
    pub fn vram_addr_incr(&self) -> u16 {
        if self.ppuctrl & 0x04 == 0 {
            1 //Going across
        } else {
            32 //Going down
        }
    }
    pub fn sprite_pattern_addr(&self) -> usize {
        if self.ppuctrl & 0x08 == 0 {
            0x0000
        } else {
            0x1000
        }
    }
    pub fn bg_pattern_addr(&self) -> usize {
        if self.ppuctrl & 0x10 == 0 {
            0x0000
        } else {
            0x1000
        }
    }
    pub fn long_sprites(&self) -> bool {
        self.ppuctrl & 0x20 != 0
    }
    pub fn ppu_slave(&self) -> bool {
        //Apparently this should never be set because on most cartridges it would short Vcc and Gnd
        self.ppuctrl & 0x40 != 0
    }
    pub fn vblank_nmi(&self) -> bool {
        self.ppuctrl & 0x80 != 0
    }

    pub fn greyscale(&self) -> bool {
        self.ppumask & 0x01 != 0
    }

    pub fn show_bg_left(&self) -> bool {
        self.ppumask & 0x02 != 0
    }

    pub fn show_sprt_left(&self) -> bool {
        self.ppumask & 0x04 != 0
    }
    pub fn render_bg(&self) -> bool {
        self.ppumask & 0x08 != 0
    }
    pub fn render_sprt(&self) -> bool {
        self.ppumask & 0x10 != 0
    }
    pub fn emph_red(&self) -> bool {
        self.ppumask & 0x20 != 0
    }
    pub fn emph_green(&self) -> bool {
        self.ppumask & 0x40 != 0
    }
    pub fn emph_blue(&self) -> bool {
        self.ppumask & 0x80 != 0
    }
    pub fn scroll_x(&self) -> u16 {
        let bit_8 = ((self.ppuctrl & 0x01) as u16) << 8;
        (self.ppuscroll & 0xFF) | bit_8
    }
    pub fn scroll_y(&self) -> u16 {
        let bit_8 = ((self.ppuctrl & 0x02) as u16) << 7;
        ((self.ppuscroll & 0xFF00) >> 8) | bit_8
    }
    pub fn coarse_scroll_x(&self) -> u8 {
        (self.scroll_x() >> 3) as u8
    }
    pub fn coarse_scroll_y(&self) -> u8 {
        (self.scroll_y() >> 3) as u8
    }
    pub fn vblank_flag(&self) -> bool {
        self.ppustatus & 0x80 != 0
    }
    pub fn sprite_hit_flag(&self) -> bool {
        self.ppustatus & 0x40 != 0
    }
    pub fn sprite_overflow_flag(&self) -> bool {
        self.ppustatus & 0x20 != 0
    }
}

const DEFAULT_PALETTE: &[u8] = include_bytes!("../2C02G_U_wiki.pal");

pub fn to_palette(bytes: [u8; 0xC0]) -> [u32; 64] {
    bytes
        .chunks_exact(3)
        .into_iter()
        .map(|color_bytes| {
            u32::from_be_bytes([0xFF, color_bytes[0], color_bytes[1], color_bytes[2]])
        })
        .collect::<Vec<u32>>()
        .try_into()
        .unwrap()
}

pub struct PPU {
    line: usize,
    dot: usize,
    cycle_count: usize,
    mem: Memory<0x4000>,
    reg: Rc<RefCell<PPURegisters>>,
    nmi: bool,
    palette_mapping: [u32; 64],
    monitor: [u32; 256 * 240],
    oam: [u8; 256],
    sprite_list: Vec<usize>,
}

impl PPU {
    pub fn check_registers(&mut self) {
        let mut reg = self.reg.borrow_mut();
        if let Some(addr) = reg.wrote_ppudata.take() {
            self.mem.write(addr as usize, reg.ppudata);
        }
        if let Some(addr) = reg.wrote_oamdata.take() {
            self.oam[addr as usize] = reg.oamdata;
        }
        reg.ppudata = self.mem.read(reg.ppuaddr as usize);
        reg.oamdata = self.oam[reg.oamaddr as usize];
    }
    pub fn poll_nmi(&mut self) -> bool {
        if self.nmi {
            self.nmi = false;
            true
        } else {
            false
        }
    }
    pub fn start_with_memory(mem: Memory<0x4000>) -> Self {
        Self {
            line: 0,
            dot: 0,
            cycle_count: 0,
            mem,
            reg: Default::default(),
            nmi: false,
            palette_mapping: to_palette(DEFAULT_PALETTE[..0xC0].try_into().unwrap()),
            monitor: [0; 256 * 240],
            oam: [0; 256],
            sprite_list: Vec::with_capacity(8),
        }
    }

    pub fn with_cartridge_memory(
        mut memory: Vec<Box<dyn MemoryRegion>>,
        mirroring: NametableMirroring,
    ) -> Memory<0x4000> {
        memory.push(Box::new(NametableMemory::new(mirroring)));
        memory.push(Box::new(RwRam::<0x20>::with_mirrored_width(0x3F00, 0x100)));
        Memory::construct(memory).unwrap()
    }

    fn get_color(&mut self, color_idx: usize, palette_idx: usize, background: bool) -> u32 {
        if color_idx == 0 {
            //Transparent
            return 0;
        }
        let pallet_addr = if background { 0x3F00 } else { 0x3F10 } + palette_idx * 4 + color_idx;

        let color = self.palette_mapping[(self.mem.read(pallet_addr) & 0x3F) as usize];
        color
    }

    fn get_palette_idx(
        &mut self,
        attribute_table_addr: usize,
        tile_x: usize,
        tile_y: usize,
    ) -> usize {
        let attribute_x = tile_x / 4;
        let attribute_y = tile_y / 4;
        let attribute_idx = attribute_x + attribute_y * 8;
        let attribute = self.mem.read(attribute_table_addr + attribute_idx);
        let quadrant_x = (tile_x / 2) % 2;
        let quadrant_y = (tile_y / 2) % 2;
        let quadrant_idx = quadrant_x | (quadrant_y << 1);
        let palette_idx = (attribute >> (quadrant_idx * 2)) & 0x03;
        palette_idx as usize
    }

    fn get_bg_pixel(&mut self) -> u32 {
        //There's a total of 512 pixels for two nametables in the horizontal position (256 per nametable)
        let position_x = (self.dot + self.reg.borrow().scroll_x() as usize) % 512;

        let position_y = (self.line + self.reg.borrow().scroll_y() as usize) % 480;
        let nametable_addr: usize = 0x2000
            + if position_x >= 256 { 0x400 } else { 0 }
            + if position_y >= 240 { 0x800 } else { 0 };
        //32 tiles per nametable
        let tile_x = (position_x >> 3) % 32;
        let tile_y = (position_y >> 3) % 30;
        let addr = tile_x + tile_y * 32;
        let pattern_idx = self.mem.read(nametable_addr + addr);

        let pattern_addr = self.reg.borrow().bg_pattern_addr() + (pattern_idx as usize) * 16;

        let row = position_y % 8;
        let column = 7 - (position_x % 8);

        let pattern_lo = self.mem.read(pattern_addr + row);
        let pattern_hi = self.mem.read(pattern_addr + row + 8);

        let color_idx = (pattern_lo >> column) & 0x01 | (((pattern_hi >> column) & 0x01) << 1);
        let attribute_table_addr = nametable_addr + 0x3C0;

        let palette_idx = self.get_palette_idx(attribute_table_addr, tile_x, tile_y);

        self.get_color(color_idx as usize, palette_idx, true)
    }

    ///Returns the sprites' color (transparent otherwise) and whether the sprite was sprite 0
    fn get_sprite_pixel(&mut self, behind_background: bool) -> (u32, bool) {
        let x_min = self.dot;
        let x_max = self.dot + 8;
        let mut sprite_0 = false;
        let mut render_color = 0;
        for i in (0..self.sprite_list.len()).rev() {
            let sprite_idx = self.sprite_list[i];
            let sprite_y = self.oam[sprite_idx * 4] as usize;
            let sprite_tile = self.oam[sprite_idx * 4 + 1];
            let sprite_attr = self.oam[sprite_idx * 4 + 2];
            let sprite_x = self.oam[sprite_idx * 4 + 3] as usize;

            if !(x_min..x_max).contains(&sprite_x) {
                continue;
            }
            let sprite_behind_background = sprite_attr & 0x20 != 0;
            if sprite_behind_background != behind_background {
                continue;
            }
            let tile_row = sprite_y + 1 - self.line;
            let tile_col = sprite_x - self.dot;
            let pattern_addr =
                self.reg.borrow().sprite_pattern_addr() + (sprite_tile as usize) * 16;

            let pattern_lo = self.mem.read(pattern_addr + tile_row);
            let pattern_hi = self.mem.read(pattern_addr + tile_row + 8);

            let color_idx =
                (pattern_lo >> tile_col) & 0x01 | (((pattern_hi >> tile_col) & 0x01) << 1);
            let palette_idx = sprite_attr & 0x03;

            let color = self.get_color(color_idx as usize, palette_idx as usize, false);

            if color & 0xFF000000 == 0 {
                continue;
            }

            render_color = color;
            //Sprite 0 should always be last (since the rev) and thus not be overwritten
            sprite_0 = sprite_idx == 0;
        }

        return (render_color, sprite_0);
    }

    fn get_base_background_color(&mut self) -> u32 {
        let color_index = (self.mem.read(0x3F00) & 0x3) as usize;
        self.palette_mapping[color_index]
    }

    fn get_pixel(&mut self) -> u32 {
        let mut pixel = self.get_base_background_color();
        let mut background_opaque = false;
        if self.reg.borrow().render_bg() {
            let background_pixel = self.get_bg_pixel();
            if (background_pixel & 0xFF000000) != 0 {
                pixel = background_pixel;
                background_opaque = true;
            }
        }
        let render_sprt = self.reg.borrow().render_sprt();
        if render_sprt {
            let (low_priority_pixel, sprite_0) = self.get_sprite_pixel(true);
            if low_priority_pixel & 0xFF000000 != 0 {
                //Bug with nes means that sprite 0 collision is not detected on dot 255 of rendering
                if sprite_0 && background_opaque && self.dot < 255 {
                    self.reg.borrow_mut().set_sprt_hit_flag(true);
                }
                if !background_opaque {
                    pixel = low_priority_pixel;
                }
            }
            let (high_priority_pixel, sprite_0) = self.get_sprite_pixel(false);
            if high_priority_pixel & 0xFF000000 != 0 {
                if sprite_0 && background_opaque && self.dot < 255 {
                    self.reg.borrow_mut().set_sprt_hit_flag(true);
                }
                pixel = high_priority_pixel
            }
        }

        pixel
    }

    fn draw_pixel(&mut self) {
        self.monitor[self.dot + self.line * 256] = self.get_pixel();
    }

    fn advance_monitor(&mut self) {
        self.dot += 1;
        if self.dot >= 341 {
            self.dot = 0;
            self.line += 1;
            if self.line >= 262 {
                self.line = 0;
            }
        }
    }

    fn vblank_start(&mut self) {
        let mut reg = self.reg.borrow_mut();
        if reg.vblank_nmi() {
            self.nmi = true;
        }
        reg.set_vblank_flag(true);
    }

    pub fn drawing(&self) -> bool {
        self.line < 240 && self.dot < 256
    }

    pub fn finished_frame(&self) -> bool {
        self.line > 240
    }

    pub fn clock(&mut self) {
        self.check_registers();

        if self.line < 240 && self.dot == 0 {
            self.gen_sprite_list();
        }
        if self.drawing() {
            self.draw_pixel();
        }

        if self.line == 241 && self.dot == 1 {
            self.vblank_start();
        }

        self.advance_monitor();

        self.cycle_count += 1;
    }

    pub fn cycle_count(&self) -> usize {
        self.cycle_count
    }

    pub fn reg(&self) -> Rc<RefCell<PPURegisters>> {
        self.reg.clone()
    }

    pub fn oam_mut(&mut self) -> &mut [u8; 256] {
        &mut self.oam
    }

    fn gen_sprite_list(&mut self) {
        //This is intentionally bugged
        let sprite_height = if self.reg.borrow().long_sprites() {
            todo!()
        } else {
            8
        };
        self.sprite_list.clear();
        let min_sprite_y = self.line;
        let max_sprite_y = min_sprite_y + sprite_height;
        let mut overflow_m = 0;
        for i in 0..64 {
            let idx = i * 4;
            let sprite_y = self.oam[idx + overflow_m] as usize;
            if (min_sprite_y..max_sprite_y).contains(&(sprite_y + 1)) {
                if self.sprite_list.len() == 8 {
                    self.reg.borrow_mut().set_sprt_overflow_flag(true);
                    continue;
                }
                self.sprite_list.push(idx);
            }
            if self.sprite_list.len() == 8 {
                overflow_m = (overflow_m + 1) % 4;
            }
        }
    }

    pub fn monitor(&self) -> [u32; 256 * 240] {
        self.monitor
    }
}

pub struct NametableMemory {
    left_nametable: [u8; 0x400],
    right_nametable: [u8; 0x400],
    mirroring: NametableMirroring,
}

impl NametableMemory {
    pub fn new(mirroring: NametableMirroring) -> Self {
        Self {
            left_nametable: [0; 0x400],
            right_nametable: [0; 0x400],
            mirroring,
        }
    }

    //idx is relative to start of nametable region
    pub fn index(&mut self, idx: usize) -> &mut u8 {
        let idx = idx & 0x0FFF;
        let justified_idx = idx & 0x3FF;
        let table = idx >> 10;
        match self.mirroring {
            NametableMirroring::Horizontal => {
                if table < 2 {
                    &mut self.left_nametable[justified_idx]
                } else {
                    &mut self.right_nametable[justified_idx]
                }
            }
            NametableMirroring::Vertical => {
                if table & 1 == 0 {
                    &mut self.left_nametable[justified_idx]
                } else {
                    &mut self.right_nametable[justified_idx]
                }
            }
        }
    }
}

impl MemoryRegion for NametableMemory {
    fn region_range(&self) -> std::range::Range<usize> {
        (0x2000..0x3F00).into()
    }

    fn write(&mut self, rel_idx: usize, data: u8) {
        *self.index(rel_idx) = data;
    }

    fn read(&mut self, rel_idx: usize) -> u8 {
        *self.index(rel_idx)
    }
}

pub struct PPURegistersMap {
    reg: Rc<RefCell<PPURegisters>>,
    ppu_data_buffer: u8,
}

impl PPURegistersMap {
    pub fn new(reg: Rc<RefCell<PPURegisters>>) -> Self {
        Self {
            reg,
            ppu_data_buffer: 0,
        }
    }
    fn justify_idx(idx: usize) -> usize {
        idx % 8
    }
}

impl MemoryRegion for PPURegistersMap {
    fn region_range(&self) -> std::range::Range<usize> {
        (0x2000..0x4000).into()
    }

    fn write(&mut self, rel_idx: usize, data: u8) {
        match Self::justify_idx(rel_idx) {
            0x0000 => {
                self.reg.borrow_mut().ppuctrl = data;
            }
            0x0001 => {
                self.reg.borrow_mut().ppumask = data;
            }
            0x0003 => {
                self.reg.borrow_mut().oamaddr = data;
            }
            0x0004 => {
                let mut reg = self.reg.borrow_mut();
                reg.wrote_oamdata = Some(reg.oamaddr);
                reg.oamdata = data;
                reg.oamaddr = reg.oamaddr.wrapping_add(1);
            }
            0x0007 => {
                let mut reg = self.reg.borrow_mut();
                reg.wrote_ppudata = Some(reg.ppuaddr);
                reg.ppudata = data;
                reg.cpu_access_ppudata();
            }
            0x0005 => {
                let mut reg = self.reg.borrow_mut();
                if let Some(hi) = reg.t.take() {
                    reg.ppuscroll = ((hi as u16) << 8) | (data as u16);
                } else {
                    reg.t = Some(data);
                }
            }
            0x0006 => {
                let mut reg = self.reg.borrow_mut();
                if let Some(hi) = reg.t.take() {
                    reg.ppuaddr = (((hi as u16) << 8) | (data as u16)) & 0x3FFF;
                } else {
                    reg.t = Some(data);
                }
            }
            _ => {}
        }
    }

    fn read(&mut self, rel_idx: usize) -> u8 {
        match Self::justify_idx(rel_idx) {
            0x0002 => {
                let mut reg: std::cell::RefMut<'_, PPURegisters> = self.reg.borrow_mut();
                let res = reg.ppustatus;
                reg.t = None;
                reg.set_vblank_flag(false);
                res
            }
            0x0004 => {
                let mut reg = self.reg.borrow_mut();

                reg.oamaddr = reg.oamaddr.wrapping_add(1);
                reg.oamdata
            }
            0x0007 => {
                let mut reg = self.reg.borrow_mut();
                reg.cpu_access_ppudata();
                let old_buffer = self.ppu_data_buffer;
                self.ppu_data_buffer = reg.ppudata;
                old_buffer
            }
            _ => 0,
        }
    }
}

pub struct PPUOamDmaRegisterMap {
    reg: Rc<RefCell<PPURegisters>>,
}

impl PPUOamDmaRegisterMap {
    pub fn new(reg: Rc<RefCell<PPURegisters>>) -> Self {
        Self { reg }
    }
}
impl MemoryRegion for PPUOamDmaRegisterMap {
    fn region_range(&self) -> std::range::Range<usize> {
        (0x4014..0x4015).into()
    }

    fn write(&mut self, _rel_idx: usize, data: u8) {
        let mut reg = self.reg.borrow_mut();
        reg.write_oamdma = true;
        reg.oamdma = data;
    }

    fn read(&mut self, _rel_idx: usize) -> u8 {
        0
    }
}
