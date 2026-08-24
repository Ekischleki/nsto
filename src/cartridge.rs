use std::{io::Read, iter::Map};

use crate::{
    cpu::CPU,
    memory::{DummyMem, Memory, MemoryRegion},
    memory_regions::{Rom, RwRam},
    ppu::PPU,
};

pub struct Cartridge {
    prg_rom: Vec<PrgRom>,
    chr_rom: ChrRomType,
    nametable_mirroring: NametableMirroring,
    prg_ram_persist: bool,
    prg_ram_size: usize,
    mapper: Mapper,
}

impl Cartridge {
    pub fn create_devices(self) -> NesDevices {
        match &self.mapper {
            Mapper::NROM => self.create_devices_nrom(),
        }
    }

    fn create_devices_nrom(self) -> NesDevices {
        assert!(
            self.prg_rom.len() <= 2,
            "Prg rom is bigger than mappable cpu memory region."
        );
        assert_ne!(self.prg_rom.len(), 0);
        let mut cpu_region: Vec<Box<dyn MemoryRegion>> = vec![
            Box::new(DummyMem::new((0x4020..0x6000).into())),
            Box::new(RwRam::<0x2000>::with_mirrored_width(0x6000, 0x2000)),
        ];
        if self.prg_rom.len() == 1 {
            cpu_region.push(Box::new(Rom::with_mirrored_width(
                0x8000,
                0x8000,
                self.prg_rom[0].data,
            )));
        } else {
            cpu_region.push(Box::new(Rom::with_mirrored_width(
                0x8000,
                0x4000,
                self.prg_rom[0].data,
            )));
            cpu_region.push(Box::new(Rom::with_mirrored_width(
                0xC000,
                0x4000,
                self.prg_rom[1].data,
            )));
        }

        let mut ppu_region: Vec<Box<dyn MemoryRegion>> = vec![];
        if let ChrRomType::ChrRom(r) = self.chr_rom {
            assert_eq!(r.len(), 1);
            ppu_region.push(Box::new(Rom::with_mirrored_width(0, 0x2000, r[0].data)));
        }

        let ppu_mem = PPU::with_cartridge_memory(ppu_region, self.nametable_mirroring);

        let ppu = PPU::start_with_memory(ppu_mem);

        let cpu_mem = CPU::with_cartridge_memory(cpu_region, &ppu);
        let cpu = CPU::start_with_memory(cpu_mem);

        NesDevices { cpu, ppu }
    }
}

pub struct NesDevices {
    pub cpu: CPU,
    pub ppu: PPU,
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
        let mut header = [0; 16];
        s.read_exact(&mut header).unwrap();

        if &header[0..4] != b"NES\x1A" {
            todo!()
        }

        let prg_rom_banks = header[4];

        let chr_rom_banks = header[5];

        let flag_6 = header[6];
        let flag_7 = header[7];
        let prg_ram_size = header[8];
        let flag_9 = header[9];
        let flag_10 = header[10];

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

        if header[12..16] != [0, 0, 0, 0] {
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
