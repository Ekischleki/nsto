use std::fs::File;

use crate::{cartridge::Cartridge, cpu::CPU, nes::Nes, ppu::PPU};

pub mod cartridge;
pub mod cpu;
pub mod memory;
pub mod memory_regions;
pub mod nes;
pub mod ppu;
pub mod controller;

fn main() {
    let cartridge = Cartridge::from_ines(&mut File::open("./Tetris.nes").unwrap());

    let mut nes = Nes::from_cartridge(cartridge);

    nes.run();
}
