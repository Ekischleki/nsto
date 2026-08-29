use std::{cell::RefCell, rc::Rc, time::Instant};

use sdl3::{
    Sdl, VideoSubsystem, event::Event, pixels::{Color, PixelFormat}, render::{Canvas, FRect, TextureCreator}, video::{Window, WindowContext},
};

use crate::{cartridge::Cartridge, controller::Controller, cpu::CPU, ppu::PPU};

pub struct UIEngine {
    sdl: Sdl,
    video_subsystem: VideoSubsystem,
    canvas: Canvas<Window>,
    controller: Rc<RefCell<Controller>>
}

impl UIEngine {
    pub fn new(controller: Rc<RefCell<Controller>>) -> Self {
        let sdl_context = sdl3::init().unwrap();
        let video_subsystem = sdl_context.video().unwrap();

        let window = video_subsystem
            .window("NES", 256 * 2, 240 * 2)
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
            controller
        }
    }

    pub fn present_screen(&mut self, pixels: [u32; 256 * 240]) {
        let texture_creator = self.canvas.texture_creator();
        let mut texture = texture_creator
            .create_texture_streaming(PixelFormat::ARGB8888, 256 * 2, 240 * 2)
            .unwrap();

        texture.update(None, bytemuck::cast_slice(&pixels), 256 * 4).unwrap();

        self.canvas.clear();
        self.canvas.copy(&texture, Some(sdl3::render::FRect { x: 0.0, y: 0.0, w: 256.0, h: 256.0 }), FRect {x: 0.0, y: 0.0, w: 512.0, h: 480.0}).unwrap();
        self.canvas.present();

        for event in self.sdl.event_pump().unwrap().poll_iter() {
            match event {
                Event::KeyDown { keycode: Some(keycode), .. } => {
                    self.controller.borrow_mut().key_down(&keycode);
                }
                Event::KeyUp { keycode: Some(keycode), .. } => {
                    self.controller.borrow_mut().key_up(&keycode);
                }
                Event::Quit { .. } => {
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
            rendering_engine: UIEngine::new(nes_devices.controller),
        }
    }

    pub fn run(&mut self) {
        let start_time = Instant::now();
        println!("Running emulator!");
        let mut instruction_check = 1000;
        //This is helpful to set up ppu registers and within the 2 cycle clock desync window
        self.ppu.clock();
        //Optionally add up to 2 cycles of ppu clock desync
        let mut wait_for_next_frame = true;
        loop {
            if instruction_check == 0 {
                let supposed_cpu_cycle = (start_time.elapsed().as_secs_f64() * 1_790_000.0) as usize;
                if supposed_cpu_cycle < self.cpu.cycle_count() {
                    continue;
                }
                if supposed_cpu_cycle > self.cpu.cycle_count() + 10000 {
                    println!("Emulator is running slow! Supposed: {supposed_cpu_cycle}, is {} ", self.cpu.cycle_count())
                }
                instruction_check = 1000;
            }
            instruction_check-=1;
            
            self.cpu.execute_instruction();
            let oamdma_poll = self.ppu.reg().borrow_mut().poll_write_oamdma();
            if let Some(oamdma) = oamdma_poll {
                let oam = self.ppu.oam_mut();
                let mut cpu_addr = (oamdma as usize) << 8;
                //self.cpu.dump_memory(cpu_addr as u16, 0x100);
                let cpu_mem = self.cpu.mem_mut();
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
