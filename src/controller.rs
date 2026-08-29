use std::{cell::RefCell, rc::Rc};

use sdl3::keyboard::Keycode;

use crate::memory::MemoryRegion;

#[derive(Default)]
pub struct Controller {
    a: bool,
    b: bool,
    select: bool,
    start: bool,
    up: bool,
    down: bool,
    left: bool,
    right: bool
}

impl Controller {
    fn keycode_to_field(&mut self, keycode: &Keycode) -> Option<&mut bool> {

        Some(match keycode {
            Keycode::Period => {
                &mut self.a
            }
            Keycode::Comma => {
                &mut self.b
            }
            Keycode::Space => &mut self.start,
            Keycode::Return => &mut self.select,
            Keycode::W => &mut self.up,
            Keycode::S => &mut self.down,
            Keycode::A => &mut self.left,
            Keycode::D => &mut self.right,
            _ => return None
        })
    }
    pub fn key_down(&mut self, keycode: &Keycode) {
        if let Some(field) = self.keycode_to_field(keycode) {
            *field = true;
        }
    }
    pub fn key_up(&mut self, keycode: &Keycode) {
        if let Some(field) = self.keycode_to_field(keycode) {
            *field = false;
        }
    }
    pub fn pack_neg(&self) -> u8 {
        let mut res = 0;
        if self.a {
            res |= 0x01;
        }
        if self.b {
            res |= 0x02;
        }
        if self.select {
            res |= 0x04;
        }
        if self.start {
            res |= 0x08;
        }
        if self.up {
            res |= 0x10;
        }
        if self.down {
            res |= 0x20
        }
        if self.left {
            res |= 0x40;
        }
        if self.right {
            res |= 0x80;
        }
        !res

    }
}


pub struct ConrollerMemory {
    controller: Rc<RefCell<Controller>>,
    strobe: bool,
    a: u8,
    b: u8,
}

impl ConrollerMemory {
    pub fn new(controller: Rc<RefCell<Controller>>) -> Self {
        Self { controller, strobe: true, a: 0, b: 0 }
    }
}

impl MemoryRegion for ConrollerMemory {
    fn region_range(&self) -> std::range::Range<usize> {
        (0x4016..0x4018).into()
    }

    fn write(&mut self, rel_idx: usize, data: u8) {
        match rel_idx {
            0x0000 => {
                self.strobe = data & 0x01 == 1;
                if self.strobe {
                    self.a = self.controller.borrow().pack_neg();
                    self.b = 0xFF;
                }
            },
            _ => {}
        }
    }

    fn read(&mut self, rel_idx: usize) -> u8 {
        if self.strobe {
            self.a = self.controller.borrow().pack_neg();
            //No buttons pressed
            self.b = 0xFF;
        }

        match rel_idx {
            0x0000 => {
                //Inverted
                let is_pressed = self.a & 0x01 == 0;
                self.a >>= 1;
                if is_pressed {
                    0x01
                } else {
                    0x00
                }
            }
            0x0001 => {
                //Inverted
                let is_pressed = self.b & 0x01 == 0;
                self.b >>= 1;
                if is_pressed {
                    0x01
                } else {
                    0x00
                }
            }
            _ => unreachable!()
        }
    }
}