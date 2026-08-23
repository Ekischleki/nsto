use std::{io::Read, iter::Map};

use crate::memory::{Memory, MemoryRegion};

pub struct Cartridge {
    prg_rom: Vec<PrgRom>,
    chr_rom: ChrRomType,
    nametable_mirroring: NametableMirroring,
    prg_ram_persist: bool,
    prg_ram_size: usize,
    mapper: Mapper,
}

impl Cartridge {
    pub fn create_mem() {}
}

pub struct RomMem {
    cpu: Memory<0x10000>,
    ppu: Memory<0x4000>,
}

#[repr(u8)]
pub enum Mapper {
    NROM = 0,
    //Theres about a billion different mappers and im _not_ going to implement them all
}

impl TryFrom<u8> for Mapper {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::NROM),
            _ => Err(()),
        }
    }
}

pub enum NametableMirroring {
    Horizontal,
    Vertical,
}

pub enum ChrRomType {
    ChrRom(Vec<ChrRom>),
    ChrRam,
}

impl Cartridge {
    pub fn from_ines<S: Read>(s: &mut S) -> Self {
        let mut magic_number = [0; 4];
        s.read_exact(&mut magic_number).unwrap();
        if &magic_number != b"NES\x1A" {
            todo!()
        }
        let mut prg_rom_size = [0; 1];
        s.read_exact(&mut prg_rom_size).unwrap();
        let prg_rom_banks = prg_rom_size[0];

        let mut chr_rom_size = [0; 1];
        s.read_exact(&mut chr_rom_size).unwrap();
        let chr_rom_banks = chr_rom_size[0];

        let mut flags = [0; 5];
        s.read_exact(&mut flags).unwrap();

        let flag_6 = flags[0];
        let flag_7 = flags[1];
        let prg_ram_size = flags[2];
        let flag_9 = flags[3];
        let flag_10 = flags[4];

        let mapper = (flag_6 >> 4) | (flag_7 & 0xF0);
        let nametable_mirroring = if flag_6 & 0x01 == 0 {
            NametableMirroring::Horizontal
        } else {
            NametableMirroring::Vertical
        };
        let prg_ram_persist = flag_6 & 0x02 != 0;
        let trainer = flag_6 & 0x04 != 0;
        let alt_nametable_layout = flag_6 & 0x08 != 0;
        assert!(!trainer, "Trainer are not supported");
        assert!(
            !alt_nametable_layout,
            "Alternative nametable layouts are not supported"
        );

        let ines_ver = (flag_7 & 0x0C) >> 2;

        if ines_ver == 2 {
            println!("Warn: NES 2.0 is not supported");
        }
        //We ignore Vs Unisystem and PlayChoice-10 for now.
        let prg_ram_size = prg_ram_size as usize * 0x2000;

        let mut padding = [0; 4];

        s.read_exact(&mut padding).unwrap();

        if padding != [0, 0, 0, 0] {
            println!("Warn: Padding is not zeroed.");
        }

        let mut prg_rom = Vec::with_capacity(prg_rom_banks as usize);

        for _ in 0..prg_rom_banks {
            let mut prg_rom_buf = [0; 0x4000];
            s.read_exact(&mut prg_rom_buf).unwrap();
            prg_rom.push(PrgRom { data: prg_rom_buf });
        }

        let chr_rom = if chr_rom_banks == 0 {
            ChrRomType::ChrRam
        } else {
            let mut chr_rom = Vec::with_capacity(chr_rom_banks as usize);

            for _ in 0..chr_rom_banks {
                let mut chr_rom_buf = [0; 0x2000];
                s.read_exact(&mut chr_rom_buf).unwrap();
                chr_rom.push(ChrRom { data: chr_rom_buf });
            }
            ChrRomType::ChrRom(chr_rom)
        };

        Self {
            prg_rom,
            chr_rom,
            nametable_mirroring,
            prg_ram_persist,
            prg_ram_size,
            mapper: mapper.try_into().expect("Mapper isn't implemented"),
        }
    }
}

pub struct PrgRom {
    data: [u8; 0x4000],
}

pub struct ChrRom {
    data: [u8; 0x2000],
}
