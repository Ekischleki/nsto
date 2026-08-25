use sdl3::{
    Sdl, VideoSubsystem,
    event::Event,
    pixels::{Color, PixelFormat},
    render::{Canvas, TextureCreator},
    video::{Window, WindowContext},
};

use crate::{cartridge::Cartridge, cpu::CPU, ppu::PPU};

pub struct UIEngine {
    sdl: Sdl,
    video_subsystem: VideoSubsystem,
    canvas: Canvas<Window>,
}

impl UIEngine {
    pub fn new() -> Self {
        let sdl_context = sdl3::init().unwrap();
        let video_subsystem = sdl_context.video().unwrap();

        let window = video_subsystem
            .window("NES", 256, 240)
            .position_centered()
            .build()
            .unwrap();

        let mut canvas = window.into_canvas();

        canvas.set_draw_color(Color::RGB(0, 0, 0));
        canvas.clear();
        canvas.present();

        Self {
            sdl: sdl_context,
            video_subsystem,
            canvas,
        }
    }

    pub fn present_screen(&mut self, pixels: [u32; 256 * 240]) {
        let texture_creator = self.canvas.texture_creator();
        let mut texture = texture_creator
            .create_texture_streaming(PixelFormat::ARGB8888, 256, 240)
            .unwrap();

        texture.update(None, bytemuck::cast_slice(&pixels), 256 * 4);

        self.canvas.clear();
        self.canvas.copy(&texture, None, None);
        self.canvas.present();

        for event in self.sdl.event_pump().unwrap().poll_iter() {
            match event {
                Event::Quit { timestamp } => {
                    panic!("Quit")
                }
                _ => {}
            }
        }
    }
}
pub struct Nes {
    cpu: CPU,
    ppu: PPU,

    rendering_engine: UIEngine,
}

impl Nes {
    pub fn from_cartridge(cartridge: Cartridge) -> Self {
        let nes_devices = cartridge.create_devices();

        Self {
            cpu: nes_devices.cpu,
            ppu: nes_devices.ppu,
            rendering_engine: UIEngine::new(),
        }
    }

    pub fn run(&mut self) {
        println!("Running emulator!");
        //This is helpful to set up ppu registers and within the 2 cycle clock desync window
        self.ppu.clock();
        //Optionally add up to 2 cycles of ppu clock desync
        let mut wait_for_next_frame = true;
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

            if !self.ppu.finished_frame() {
                wait_for_next_frame = false;
            } else {
                if !wait_for_next_frame {
                    wait_for_next_frame = true;
                    self.rendering_engine.present_screen(self.ppu.monitor());
                }
            }

            if self.ppu.poll_nmi() {
                self.cpu.nmi();
            }
        }
    }
}
