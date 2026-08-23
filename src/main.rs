use std::fs::File;

use crate::cartridge::Cartridge;

pub mod cartridge;
pub mod cpu;
pub mod memory;
pub mod ppu;
pub mod memory_regions;

fn main() {
    let cartridge = Cartridge::from_ines(&mut File::open("./SMB.nes").unwrap());
}
