use crate::{cartridge::Cartridge, cpu::CPU, ppu::PPU};

pub struct Nes {
    cpu: CPU,
    ppu: PPU,
    irq: bool,
    nmi: bool,
}

impl Nes {
    pub fn from_cartridge(cartridge: Cartridge) -> Self {
        let nes_devices = cartridge.create_devices();

        Self {
            cpu: nes_devices.cpu,
            ppu: nes_devices.ppu,
            irq: false,
            nmi: false,
        }
    }

    pub fn run(&mut self) {
        //Optionally add up to 2 cycles of ppu clock desync
        loop {
            self.cpu.execute_instruction();
            let oamdma_poll = self.ppu.reg().borrow_mut().poll_write_oamdma();
            if let Some(oamdma) = oamdma_poll {
                let cpu_mem = self.cpu.mem_mut();
                let oam = self.ppu.oam_mut();
                let mut cpu_addr = (oamdma as usize) << 8;
                for oam_addr in 0..256 {
                    oam[oam_addr] = cpu_mem.read(cpu_addr);
                    cpu_addr += 1;
                }
                *self.cpu.cycle_count_mut() += 513;
            }
            while self.ppu.cycle_count() < self.cpu.cycle_count() * 3 {
                self.ppu.clock();
            }
            if self.ppu.poll_nmi() {
                self.cpu.nmi();
            }
        }
    }
}
