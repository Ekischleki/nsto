use crate::{
    memory::{DummyMem, InternalRam, Memory, MemoryRegion},
    ppu::{PPU, PPUOamDmaRegisterMap, PPURegistersMap},
};

pub struct Registers {
    a: u8,
    x: u8,
    y: u8,
    pc: u16,
    sp: u8,
    c: bool,
    z: bool,
    i: bool,
    d: bool,
    v: bool,
    n: bool,
    _b: bool,
    schedule_i: Option<bool>,
}

impl Registers {
    pub fn new() -> Self {
        Self {
            a: 0,
            x: 0,
            y: 0,
            pc: 0,
            sp: 0,
            c: false,
            z: false,
            v: false,
            n: false,
            _b: false,
            d: false,
            i: true,
            schedule_i: None,
        }
    }
}

pub struct CPU {
    mem: Memory<0x10000>,
    reg: Registers,
    cycle_count: usize,
}

impl CPU {
    pub fn cycle_count(&self) -> usize {
        self.cycle_count
    }
    pub fn new() -> Self {
        let reg = Registers::new();
        let mem = Memory::construct(vec![
            Box::new(InternalRam::default()),
            Box::new(DummyMem::new((0x2000..0x10000).into())),
        ])
        .unwrap();
        Self {
            mem,
            reg,
            cycle_count: 0,
        }
    }

    pub fn start_with_memory(mem: Memory<0x10000>) -> Self {
        let mut res = Self {
            mem,
            reg: Registers::new(),
            cycle_count: 0,
        };
        res.reset();
        res
    }

    //Creates the default cpu memory with the cartridge memory represented at 0x4020-0x10000
    pub fn with_cartridge_memory(
        mut mem: Vec<Box<dyn MemoryRegion>>,
        ppu: &PPU,
    ) -> Memory<0x10000> {
        let mut default_mem: Vec<Box<dyn MemoryRegion>> = vec![
            Box::new(InternalRam::default()),
            Box::new(PPURegistersMap::new(ppu.reg())),
            Box::new(DummyMem::new((0x4000..0x4014).into())),
            Box::new(PPUOamDmaRegisterMap::new(ppu.reg())),
            Box::new(DummyMem::new((0x4015..0x4020).into())),
        ];
        default_mem.append(&mut mem);
        Memory::construct(default_mem).unwrap()
    }

    pub fn reset(&mut self) {
        self.reg.sp = self.reg.sp.wrapping_sub(3);
        self.reg.pc = self.deref_word_at(0xFFFC);
        self.reg.i = true;
    }

    fn read_addr_zp_x(&mut self, arg: u8, cycle: bool) -> u8 {
        let addr = arg.wrapping_add(self.reg.x) as usize;
        let res = self.mem.read(addr);
        if cycle {
            self.cycle_count += 4;
        }
        res
    }

    fn write_addr_zp_x(&mut self, arg: u8, data: u8, cycle: bool) {
        let addr = arg.wrapping_add(self.reg.x) as usize;
        self.mem.write(addr, data);
        if cycle {
            self.cycle_count += 4;
        }
    }

    fn read_addr_zp_y(&mut self, arg: u8, cycle: bool) -> u8 {
        let addr = arg.wrapping_add(self.reg.y) as usize;
        let res = self.mem.read(addr);
        if cycle {
            self.cycle_count += 4;
        }
        res
    }

    fn write_addr_zp_y(&mut self, arg: u8, data: u8, cycle: bool) {
        let addr = arg.wrapping_add(self.reg.y) as usize;
        self.mem.write(addr, data);
        if cycle {
            self.cycle_count += 4;
        }
    }

    fn read_addr_abs_x(&mut self, arg: u16, cycle: bool) -> u8 {
        let addr = arg.wrapping_add(self.reg.x as u16);
        let res = self.mem.read(addr as usize);
        if cycle {
            self.cycle_count += 4;
            //Oops cycle detection
            if arg & 0xFF00 != addr & 0xFF00 {
                self.cycle_count += 1;
            }
        }
        res
    }

    fn write_addr_abs_x(&mut self, arg: u16, val: u8, cycle: bool) {
        let addr = arg.wrapping_add(self.reg.x as u16);
        self.mem.write(addr as usize, val);
        if cycle {
            self.cycle_count += 5;
        }
    }

    fn write_addr_abs_y(&mut self, arg: u16, data: u8, cycle: bool) {
        let addr = arg.wrapping_add(self.reg.y as u16);
        self.mem.write(addr as usize, data);
        if cycle {
            self.cycle_count += 5;
        }
    }

    fn read_addr_abs_y(&mut self, arg: u16, cycle: bool) -> u8 {
        let addr = arg.wrapping_add(self.reg.y as u16);
        let res = self.mem.read(addr as usize);
        if cycle {
            self.cycle_count += 4;
            //Oops cycle detection
            if arg & 0xFF00 != addr & 0xFF00 {
                self.cycle_count += 1;
            }
        }
        res
    }

    fn deref_word_at_zp(&mut self, ptr: u8) -> u16 {
        self.deref_word_at(ptr as u16)
    }

    fn deref_word_at(&mut self, ptr: u16) -> u16 {
        let lo = self.mem.read(ptr as usize);
        let hi = self.mem.read(ptr.wrapping_add(1) as usize);
        let res = u16::from_le_bytes([lo, hi]);
        res
    }

    fn read_addr_indr_x(&mut self, arg: u8, cycle: bool) -> u8 {
        let ptr = arg.wrapping_add(self.reg.x);
        let addr = self.deref_word_at_zp(ptr);
        let res = self.mem.read(addr as usize);
        if cycle {
            self.cycle_count += 6;
        }
        res
    }

    fn write_addr_indr_x(&mut self, arg: u8, data: u8, cycle: bool) {
        let ptr = arg.wrapping_add(self.reg.x);
        let addr = self.deref_word_at_zp(ptr);
        self.mem.write(addr as usize, data);
        if cycle {
            self.cycle_count += 6;
        }
    }

    fn read_addr_indr_y(&mut self, arg: u8, cycle: bool) -> u8 {
        let deref = self.deref_word_at_zp(arg);
        let addr = deref.wrapping_add(self.reg.y as u16);
        let res = self.mem.read(addr as usize);
        if cycle {
            self.cycle_count += 5;
            if deref & 0xFF00 != addr & 0xFF00 {
                self.cycle_count += 1;
            }
        }
        res
    }

    fn write_addr_indr_y(&mut self, arg: u8, data: u8, cycle: bool) {
        let deref = self.deref_word_at_zp(arg);
        let addr = deref.wrapping_add(self.reg.y as u16);
        self.mem.write(addr as usize, data);
        if cycle {
            self.cycle_count += 6;
        }
    }

    fn fetch_pc_byte(&mut self) -> u8 {
        let res = self.mem.read(self.reg.pc as usize);
        self.reg.pc += 1;
        res
    }

    fn fetch_abs_from_pc(&mut self) -> u8 {
        let addr = self.fetch_pc_word();
        self.mem.read(addr as usize)
    }

    fn fetch_pc_word(&mut self) -> u16 {
        let lo = self.fetch_pc_byte();
        let hi = self.fetch_pc_byte();
        u16::from_le_bytes([lo, hi])
    }

    fn set_flags(&mut self, c: bool, z: bool, n: bool, val: u16) {
        if c {
            self.reg.c = val > 0xFF;
        }
        if z {
            self.reg.z = val == 0;
        }
        if n {
            self.reg.n = val & 0x80 != 0;
        }
    }

    fn adc(&mut self, mem: u8) {
        let res = self.reg.a as u16 + mem as u16 + if self.reg.c { 1 } else { 0 };
        self.set_flags(true, true, true, res);
        let res = (res & 0xFF) as u8;
        self.reg.v = (res ^ self.reg.a) & (res ^ mem) & 0x80 != 0;
        self.reg.a = res;
    }

    fn and(&mut self, mem: u8) {
        self.reg.a = self.reg.a & mem;
        self.set_flags(false, true, true, self.reg.a as u16);
    }

    fn asl(&mut self, value: u8) -> u8 {
        self.reg.c = value & 0x80 != 0;
        let res = value << 1;
        self.reg.z = res == 0;
        self.reg.n = res & 0x80 != 0;
        res
    }

    fn lsr(&mut self, value: u8) -> u8 {
        self.reg.c = value & 0x01 != 0;
        let res = value >> 1;
        self.reg.z = res == 0;
        self.reg.n = false;
        res
    }

    fn ror(&mut self, value: u8) -> u8 {
        self.reg.c = value & 0x01 != 0;
        let res = value.rotate_right(1);
        self.reg.z = res == 0;
        self.reg.n = res & 0x80 != 0;
        res
    }

    fn rol(&mut self, value: u8) -> u8 {
        self.reg.c = value & 0x80 != 0;
        let res = value.rotate_left(1);
        self.reg.z = res == 0;
        self.reg.n = res & 0x80 != 0;
        res
    }

    fn dec(&mut self, value: u8) -> u8 {
        let res = value.wrapping_sub(1);
        self.reg.z = res == 0;
        self.reg.n = res & 0x80 != 0;
        res
    }

    fn inc(&mut self, value: u8) -> u8 {
        let res = value.wrapping_add(1);
        self.reg.z = res == 0;
        self.reg.n = res & 0x80 != 0;
        res
    }

    fn bit(&mut self, mem: u8) {
        let v = self.reg.a & mem;
        self.reg.z = v == 0;
        self.reg.n = v & 0x80 != 0;
        self.reg.v = v & 0x40 != 0;
    }

    fn cmp(&mut self, mem: u8) {
        self.reg.c = self.reg.a >= mem;
        self.reg.z = self.reg.a == mem;
        self.reg.n = self.reg.a.wrapping_sub(mem) & 0x80 != 0;
    }

    fn cpx(&mut self, mem: u8) {
        self.reg.c = self.reg.x >= mem;
        self.reg.z = self.reg.x == mem;
        self.reg.n = self.reg.x.wrapping_sub(mem) & 0x80 != 0;
    }

    fn cpy(&mut self, mem: u8) {
        self.reg.c = self.reg.y >= mem;
        self.reg.z = self.reg.y == mem;
        self.reg.n = self.reg.y.wrapping_sub(mem) & 0x80 != 0;
    }

    fn eor(&mut self, mem: u8) {
        let res = self.reg.a ^ mem;
        self.reg.z = res == 0;
        self.reg.n = res & 0x80 != 0;
        self.reg.a = res;
    }

    fn lda(&mut self, mem: u8) {
        self.reg.z = mem == 0;
        self.reg.n = mem & 0x80 != 0;
        self.reg.a = mem;
    }

    fn ldx(&mut self, mem: u8) {
        self.reg.z = mem == 0;
        self.reg.n = mem & 0x80 != 0;
        self.reg.x = mem;
    }

    fn ldy(&mut self, mem: u8) {
        self.reg.z = mem == 0;
        self.reg.n = mem & 0x80 != 0;
        self.reg.y = mem;
    }

    fn ora(&mut self, mem: u8) {
        self.reg.a = self.reg.a | mem;
        self.reg.z = self.reg.a == 0;
        self.reg.n = self.reg.a & 0x80 != 0;
    }

    fn sbc(&mut self, mem: u8) {
        let res = self.reg.a as i16 - mem as i16 - if self.reg.c { 0 } else { 1 };
        self.reg.c = res >= 0;
        let res = (res & 0x00FF) as u8;
        self.reg.z = res == 0;
        self.reg.v = (res ^ self.reg.a) & (res ^ !mem) & 0x80 != 0;
        self.reg.n = 0x80 & res != 0;
        self.reg.a = res;
    }

    fn push(&mut self, mem: u8) {
        let sp = self.reg.sp as usize + 0x100;
        self.mem.write(sp, mem);
        println!("Push {mem} to {}", self.reg.sp);
        self.reg.sp = self.reg.sp.wrapping_sub(1);
    }

    fn pop(&mut self) -> u8 {
        self.reg.sp = self.reg.sp.wrapping_add(1);
        let sp = self.reg.sp as usize + 0x100;
        let res = self.mem.read(sp);
        println!("Pop {res} from {}", self.reg.sp);
        res
    }

    fn branch_if_condition(&mut self, v: bool) {
        let rel = self.fetch_pc_byte().cast_signed() as i16;
        let dest = self.reg.pc.wrapping_add_signed(rel);
        self.cycle_count += 2;
        if !v {
            return;
        }
        self.cycle_count += 1;
        if self.reg.pc & 0xFF00 != dest & 0xFF00 {
            self.cycle_count += 1;
        }
        self.reg.pc = dest;
    }

    fn push_pc_to_stack(&mut self) {
        self.push((self.reg.pc >> 8) as u8);
        self.push((self.reg.pc & 0xFF) as u8);
    }

    fn pop_pc_from_stack(&mut self) {
        let lo = self.pop();
        let hi = self.pop();
        self.reg.pc = u16::from_le_bytes([lo, hi]);
    }
    pub fn dump_memory(&mut self, start: u16, len: usize) {
        for offset in (0..len).step_by(16) {
            let addr = start.wrapping_add(offset as u16);

            print!("{addr:04X}: ");

            for i in 0..16 {
                if offset + i < len {
                    let value = self.mem.read(addr.wrapping_add(i as u16) as usize);
                    print!("{value:02X} ");
                } else {
                    print!("   ");
                }
            }

            println!();
        }
    }
    fn pack_flags(&self, b5: bool, b4: bool) -> u8 {
        let mut packed = 0b00000000;
        if self.reg.n {
            packed |= 0x80;
        }
        if self.reg.v {
            packed |= 0x40;
        }
        if b5 {
            packed |= 0x20;
        }
        if b4 {
            packed |= 0x10;
        }
        if self.reg.d {
            packed |= 0x08;
        }
        if self.reg.i {
            packed |= 0x04;
        }
        if self.reg.z {
            packed |= 0x02;
        }
        if self.reg.c {
            packed |= 0x01;
        }
        packed
    }

    fn apply_packed(&mut self, packed: u8, sched_i: bool) {
        self.reg.n = packed & 0x80 != 0;
        self.reg.v = packed & 0x40 != 0;
        self.reg.d = packed & 0x08 != 0;
        if sched_i {
            self.reg.schedule_i = Some(packed & 0x04 != 0);
        } else {
            self.reg.i = packed & 0x04 != 0;
        }
        self.reg.z = packed & 0x02 != 0;
        self.reg.c = packed & 0x01 != 0;
    }

    ///Triggers a non maskable interrupt
    pub fn nmi(&mut self) {
        println!("NMI call");
        self.push_pc_to_stack();
        self.push(self.pack_flags(true, false));
        self.reg.i = true;
        let addr = self.deref_word_at(0xFFFA);
        self.reg.pc = addr;
        self.cycle_count += 7;
    }

    pub fn execute_instruction(&mut self) {
        if let Some(sched_i) = self.reg.schedule_i.take() {
            self.reg.i = sched_i;
        }

        let instr = self.fetch_pc_byte();
        match instr {
            //--- ADC
            // Immediate
            0x69 => {
                let immediate = self.fetch_pc_byte();
                self.adc(immediate);
                self.cycle_count += 2;
            }
            //ZP
            0x65 => {
                let addr = self.fetch_pc_byte();
                let mem = self.mem.read(addr as usize);
                self.adc(mem);
                self.cycle_count += 3;
            }
            //ZP, X
            0x75 => {
                let arg = self.fetch_pc_byte();
                let mem = self.read_addr_zp_x(arg, true);
                self.adc(mem);
            }
            //ABS
            0x6D => {
                let mem = self.fetch_abs_from_pc();
                self.adc(mem);
                self.cycle_count += 4;
            }
            //ABS, X
            0x7D => {
                let arg = self.fetch_pc_word();
                let mem = self.read_addr_abs_x(arg, true);
                self.adc(mem);
            }
            //ABS, Y
            0x79 => {
                let arg = self.fetch_pc_word();
                let mem = self.read_addr_abs_y(arg, true);
                self.adc(mem);
            }
            //(IND, X)
            0x61 => {
                let arg = self.fetch_pc_byte();
                let mem = self.read_addr_indr_x(arg, true);
                self.adc(mem);
            }
            //(IND), Y
            0x71 => {
                let arg = self.fetch_pc_byte();
                let mem = self.read_addr_indr_y(arg, true);
                self.adc(mem);
            }
            //-----AND
            //IM
            0x29 => {
                let mem = self.fetch_pc_byte();
                self.and(mem);
                self.cycle_count += 2;
            }

            //ZP
            0x25 => {
                let addr = self.fetch_pc_byte();
                let mem = self.mem.read(addr as usize);
                self.and(mem);
                self.cycle_count += 3;
            }
            //ZP, X
            0x35 => {
                let arg = self.fetch_pc_byte();
                let mem = self.read_addr_zp_x(arg, true);
                self.and(mem);
            }
            //ABS
            0x2D => {
                let mem = self.fetch_abs_from_pc();
                self.and(mem);
                self.cycle_count += 4;
            }
            //ABS, X
            0x3D => {
                let arg = self.fetch_pc_word();
                let mem = self.read_addr_abs_x(arg, true);
                self.and(mem);
            }
            //ABS, Y
            0x39 => {
                let arg = self.fetch_pc_word();
                let mem = self.read_addr_abs_y(arg, true);
                self.and(mem);
            }
            //(IND, X)
            0x21 => {
                let arg = self.fetch_pc_byte();
                let mem = self.read_addr_indr_x(arg, true);
                self.and(mem);
            }
            //(IND), Y
            0x31 => {
                let arg = self.fetch_pc_byte();
                let mem = self.read_addr_indr_y(arg, true);
                self.and(mem);
            }
            //-----ASL
            //A
            0x0A => {
                self.reg.a = self.asl(self.reg.a);
                self.cycle_count += 2;
            }
            //ZP
            0x06 => {
                let addr = self.fetch_pc_byte();
                let value = self.mem.read(addr as usize);
                let res = self.asl(value);
                self.mem.write(addr as usize, res);
                self.cycle_count += 5;
            }
            //ZP, X
            0x16 => {
                let arg = self.fetch_pc_byte();
                let value = self.read_addr_zp_x(arg, false);
                let res = self.asl(value);
                self.write_addr_zp_x(arg, res, false);
                self.cycle_count += 6;
            }
            //ABS
            0x0E => {
                let addr = self.fetch_pc_word();
                let value = self.mem.read(addr as usize);
                let res = self.asl(value);
                self.mem.write(addr as usize, res);
                self.cycle_count += 6;
            }
            //ABS, X
            0x1E => {
                let arg = self.fetch_pc_word();
                let value = self.read_addr_abs_x(arg, false);
                let res = self.asl(value);
                self.write_addr_abs_x(arg, res, false);
                self.cycle_count += 7;
            }
            //----BCC
            0x90 => {
                self.branch_if_condition(!self.reg.c);
            }
            //----BCS
            0xB0 => {
                self.branch_if_condition(self.reg.c);
            }
            //----BEQ
            0xF0 => {
                self.branch_if_condition(self.reg.z);
            }
            //----BIT
            //ZP
            0x24 => {
                let addr = self.fetch_pc_byte();
                let mem = self.mem.read(addr as usize);
                self.bit(mem);
                self.cycle_count += 3;
            }
            //ABS
            0x2C => {
                let addr = self.fetch_pc_word();
                let mem = self.mem.read(addr as usize);
                self.bit(mem);
                self.cycle_count += 4;
            }
            //----BMI
            0x30 => {
                self.branch_if_condition(self.reg.n);
            }
            //----BNE
            0xD0 => {
                self.branch_if_condition(!self.reg.z);
            }
            //----BPL
            0x10 => {
                self.branch_if_condition(!self.reg.n);
            }
            //----BRK
            //TODO: Implement 6502 bug where simultaneous IRQ and NMI skip IRQ handler
            0x00 => {
                //This byte goes unused
                _ = self.fetch_pc_byte();
                self.push_pc_to_stack();
                let packed = self.pack_flags(true, true);
                self.push(packed);
                self.reg.i = true;
                let jmp = self.deref_word_at(0xFFFE);
                self.reg.pc = jmp;
                self.cycle_count += 7;
            }
            //----BVC
            0x50 => {
                self.branch_if_condition(!self.reg.v);
            }
            //----BVS
            0x70 => {
                self.branch_if_condition(self.reg.v);
            }
            //----CLC
            0x18 => {
                self.reg.c = false;
                self.cycle_count += 2;
            }
            //----CLD
            0xD8 => {
                self.reg.d = false;
                self.cycle_count += 2;
            }
            //----CLI
            0x58 => {
                self.reg.schedule_i = Some(false);
                self.cycle_count += 2;
            }
            //----CLV
            0xB8 => {
                self.reg.v = false;
                self.cycle_count += 2;
            }
            //----CMP
            //Im
            0xC9 => {
                let mem = self.fetch_pc_byte();
                self.cmp(mem);
                self.cycle_count += 2;
            }
            //ZP
            0xC5 => {
                let addr = self.fetch_pc_byte() as usize;
                let mem = self.mem.read(addr);
                self.cmp(mem);
                self.cycle_count += 3;
            }
            //ZP, X
            0xD5 => {
                let arg = self.fetch_abs_from_pc();
                let mem = self.read_addr_zp_x(arg, true);
                self.cmp(mem);
            }
            //Abs
            0xCD => {
                let addr = self.fetch_pc_word() as usize;
                let mem = self.mem.read(addr);
                self.cmp(mem);
                self.cycle_count += 4;
            }
            //Abs, X
            0xDD => {
                let arg = self.fetch_pc_word();
                let mem = self.read_addr_abs_x(arg, true);
                self.cmp(mem);
            }
            //Abs, Y
            0xD9 => {
                let arg = self.fetch_pc_word();
                let mem = self.read_addr_abs_y(arg, true);
                self.cmp(mem);
            }
            //Ind, X
            0xC1 => {
                let arg = self.fetch_pc_byte();
                let mem = self.read_addr_indr_x(arg, true);
                self.cmp(mem);
            }
            //Ind, Y
            0xD1 => {
                let arg = self.fetch_pc_byte();
                let mem = self.read_addr_indr_y(arg, true);
                self.cmp(mem);
            }
            //----CPX
            //Im
            0xE0 => {
                let mem = self.fetch_pc_byte();
                self.cpx(mem);
                self.cycle_count += 2;
            }
            //ZP
            0xE4 => {
                let addr = self.fetch_pc_byte() as usize;
                let mem = self.mem.read(addr);
                self.cpx(mem);
                self.cycle_count += 3;
            }
            //ABS
            0xEC => {
                let addr = self.fetch_pc_word() as usize;
                let mem = self.mem.read(addr);
                self.cpx(mem);
                self.cycle_count += 4;
            }
            //----CPY
            //Im
            0xC0 => {
                let mem = self.fetch_pc_byte();
                self.cpy(mem);
                self.cycle_count += 2;
            }
            //ZP
            0xC4 => {
                let addr = self.fetch_pc_byte() as usize;
                let mem = self.mem.read(addr);
                self.cpy(mem);
                self.cycle_count += 3;
            }
            //ABS
            0xCC => {
                let addr = self.fetch_pc_word() as usize;
                let mem = self.mem.read(addr);
                self.cpy(mem);
                self.cycle_count += 4;
            }
            //----DEC
            //ZP
            0xC6 => {
                let addr = self.fetch_pc_byte() as usize;
                let mem = self.mem.read(addr);
                let res = self.dec(mem);
                self.mem.write(addr, res);
                self.cycle_count += 5;
            }
            //ZP, X
            0xD6 => {
                let arg = self.fetch_pc_byte();
                let mem = self.read_addr_zp_x(arg, false);
                let res = self.dec(mem);
                self.write_addr_zp_x(arg, res, false);
                self.cycle_count += 6;
            }
            //ABS
            0xCE => {
                let addr = self.fetch_pc_word() as usize;
                let mem = self.mem.read(addr);
                let res = self.dec(mem);
                self.mem.write(addr, res);
                self.cycle_count += 6;
            }
            //ABS, X
            0xDE => {
                let arg = self.fetch_pc_word();
                let mem = self.read_addr_abs_x(arg, false);
                let res = self.dec(mem);
                self.write_addr_abs_x(arg, res, false);
                self.cycle_count += 7;
            }
            //----DEX
            0xCA => {
                self.reg.x = self.reg.x.wrapping_sub(1);
                self.reg.z = self.reg.x == 0;
                self.reg.n = self.reg.x & 0x80 != 0;
                self.cycle_count += 2;
            }
            //----DEY
            0x88 => {
                self.reg.y = self.reg.y.wrapping_sub(1);
                self.reg.z = self.reg.y == 0;
                self.reg.n = self.reg.y & 0x80 != 0;
                self.cycle_count += 2;
            }
            //----EOR
            //Im
            0x49 => {
                let mem = self.fetch_pc_byte();
                self.eor(mem);
                self.cycle_count += 2;
            }
            //ZP
            0x45 => {
                let addr = self.fetch_pc_byte() as usize;
                let mem = self.mem.read(addr);
                self.eor(mem);
                self.cycle_count += 3;
            }
            //ZP, X
            0x55 => {
                let arg = self.fetch_abs_from_pc();
                let mem = self.read_addr_zp_x(arg, true);
                self.eor(mem);
            }
            //Abs
            0x4D => {
                let addr = self.fetch_pc_word() as usize;
                let mem = self.mem.read(addr);
                self.eor(mem);
                self.cycle_count += 4;
            }
            //Abs, X
            0x5D => {
                let arg = self.fetch_pc_word();
                let mem = self.read_addr_abs_x(arg, true);
                self.eor(mem);
            }
            //Abs, Y
            0x59 => {
                let arg = self.fetch_pc_word();
                let mem = self.read_addr_abs_y(arg, true);
                self.eor(mem);
            }
            //Ind, X
            0x41 => {
                let arg = self.fetch_pc_byte();
                let mem = self.read_addr_indr_x(arg, true);
                self.eor(mem);
            }
            //Ind, Y
            0x51 => {
                let arg = self.fetch_pc_byte();
                let mem = self.read_addr_indr_y(arg, true);
                self.eor(mem);
            }
            //----INC
            //ZP
            0xE6 => {
                let addr = self.fetch_pc_byte() as usize;
                let mem = self.mem.read(addr);
                let res = self.inc(mem);
                self.mem.write(addr, res);
                self.cycle_count += 5;
            }
            //ZP, X
            0xF6 => {
                let arg = self.fetch_pc_byte();
                let mem = self.read_addr_zp_x(arg, false);
                let res = self.inc(mem);
                self.write_addr_zp_x(arg, res, false);
                self.cycle_count += 6;
            }
            //ABS
            0xEE => {
                let addr = self.fetch_pc_word() as usize;
                let mem = self.mem.read(addr);
                let res = self.inc(mem);
                self.mem.write(addr, res);
                self.cycle_count += 6;
            }
            //ABS, X
            0xFE => {
                let arg = self.fetch_pc_word();
                let mem = self.read_addr_abs_x(arg, false);
                let res = self.inc(mem);
                self.write_addr_abs_x(arg, res, false);
                self.cycle_count += 7;
            }
            //----INX
            0xE8 => {
                self.reg.x = self.reg.x.wrapping_add(1);
                self.reg.z = self.reg.x == 0;
                self.reg.n = self.reg.x & 0x80 != 0;
                self.cycle_count += 2;
            }
            //----INY
            0xC8 => {
                self.reg.y = self.reg.y.wrapping_add(1);
                self.reg.z = self.reg.y == 0;
                self.reg.n = self.reg.y & 0x80 != 0;
                self.cycle_count += 2;
            }
            //----JMP
            //ABS
            0x4C => {
                let addr = self.fetch_pc_word();
                self.reg.pc = addr;
                self.cycle_count += 3;
            }
            //Ind
            0x6C => {
                self.dump_memory(0, 0xFF);
                let ptr = self.fetch_pc_word();
                //This op is bugged and does not wrap the hi byte of the ptr when dereferencing across a page boundary
                let lo = self.mem.read(ptr as usize);
                let updated_ptr = ptr.wrapping_add(1);
                let updated_ptr = (0xFF00 & ptr) | (0xFF & updated_ptr);
                let hi = self.mem.read(updated_ptr as usize);
                let addr = u16::from_le_bytes([lo, hi]);
                println!("Jmp (${ptr}) = Jmp ${addr}");
                self.reg.pc = addr;
                self.cycle_count += 5;
            }
            //----JSR
            0x20 => {
                let addr = self.fetch_pc_word();
                println!("JSR {addr}");
                self.reg.pc = self.reg.pc.wrapping_sub(1);
                self.push_pc_to_stack();
                self.reg.pc = addr;
                self.cycle_count += 6;
            }
            //----LDA
            //Im
            0xA9 => {
                let mem = self.fetch_pc_byte();
                self.lda(mem);
                self.cycle_count += 2;
            }
            //ZP
            0xA5 => {
                let addr = self.fetch_pc_byte() as usize;
                let mem = self.mem.read(addr);
                self.lda(mem);
                self.cycle_count += 3;
            }
            //ZP, X
            0xB5 => {
                let arg = self.fetch_abs_from_pc();
                let mem = self.read_addr_zp_x(arg, true);
                self.lda(mem);
            }
            //Abs
            0xAD => {
                let addr = self.fetch_pc_word() as usize;
                let mem = self.mem.read(addr);
                self.lda(mem);
                self.cycle_count += 4;
            }
            //Abs, X
            0xBD => {
                let arg = self.fetch_pc_word();
                let mem = self.read_addr_abs_x(arg, true);
                self.lda(mem);
            }
            //Abs, Y
            0xB9 => {
                let arg = self.fetch_pc_word();
                let mem = self.read_addr_abs_y(arg, true);
                self.lda(mem);
            }
            //Ind, X
            0xA1 => {
                let arg = self.fetch_pc_byte();
                let mem = self.read_addr_indr_x(arg, true);
                self.lda(mem);
            }
            //Ind, Y
            0xB1 => {
                let arg = self.fetch_pc_byte();
                let mem = self.read_addr_indr_y(arg, true);
                self.lda(mem);
            }
            //----LDX
            //Im
            0xA2 => {
                let mem = self.fetch_pc_byte();
                self.ldx(mem);
                self.cycle_count += 2;
            }
            //ZP
            0xA6 => {
                let addr = self.fetch_pc_byte() as usize;
                let mem = self.mem.read(addr);
                self.ldx(mem);
                self.cycle_count += 3;
            }
            //ZP, Y
            0xB6 => {
                let arg = self.fetch_abs_from_pc();
                let mem = self.read_addr_zp_y(arg, true);
                self.ldx(mem);
            }
            //Abs
            0xAE => {
                let addr = self.fetch_pc_word() as usize;
                let mem = self.mem.read(addr);
                self.ldx(mem);
                self.cycle_count += 4;
            }
            //Abs, Y
            0xBE => {
                let arg = self.fetch_pc_word();
                let mem = self.read_addr_abs_y(arg, true);
                self.ldx(mem);
            }
            //----LDY
            //Im
            0xA0 => {
                let mem = self.fetch_pc_byte();
                self.ldy(mem);
                self.cycle_count += 2;
            }
            //ZP
            0xA4 => {
                let addr = self.fetch_pc_byte() as usize;
                let mem = self.mem.read(addr);
                self.ldy(mem);
                self.cycle_count += 3;
            }
            //ZP, X
            0xB4 => {
                let arg = self.fetch_abs_from_pc();
                let mem = self.read_addr_zp_x(arg, true);
                self.ldy(mem);
            }
            //Abs
            0xAC => {
                let addr = self.fetch_pc_word() as usize;
                let mem = self.mem.read(addr);
                self.ldy(mem);
                self.cycle_count += 4;
            }
            //Abs, X
            0xBC => {
                let arg = self.fetch_pc_word();
                let mem = self.read_addr_abs_x(arg, true);
                self.ldx(mem);
            }
            //-----LSR
            //A
            0x4A => {
                self.reg.a = self.lsr(self.reg.a);
                self.cycle_count += 2;
            }
            //ZP
            0x46 => {
                let addr = self.fetch_pc_byte();
                let value = self.mem.read(addr as usize);
                let res = self.lsr(value);
                self.mem.write(addr as usize, res);
                self.cycle_count += 5;
            }
            //ZP, X
            0x56 => {
                let arg = self.fetch_pc_byte();
                let value = self.read_addr_zp_x(arg, false);
                let res = self.lsr(value);
                self.write_addr_zp_x(arg, res, false);
                self.cycle_count += 6;
            }
            //ABS
            0x4E => {
                let addr = self.fetch_pc_word();
                let value = self.mem.read(addr as usize);
                let res = self.lsr(value);
                self.mem.write(addr as usize, res);
                self.cycle_count += 6;
            }
            //ABS, X
            0x5E => {
                let arg = self.fetch_pc_word();
                let value = self.read_addr_abs_x(arg, false);
                let res = self.lsr(value);
                self.write_addr_abs_x(arg, res, false);
                self.cycle_count += 7;
            }
            //----NOP
            0xEA => {
                self.cycle_count += 2;
            }
            //--- ORA
            // Immediate
            0x09 => {
                let immediate = self.fetch_pc_byte();
                self.ora(immediate);
                self.cycle_count += 2;
            }
            //ZP
            0x05 => {
                let addr = self.fetch_pc_byte();
                let mem = self.mem.read(addr as usize);
                self.ora(mem);
                self.cycle_count += 3;
            }
            //ZP, X
            0x15 => {
                let arg = self.fetch_pc_byte();
                let mem = self.read_addr_zp_x(arg, true);
                self.ora(mem);
            }
            //ABS
            0x0D => {
                let mem = self.fetch_abs_from_pc();
                self.ora(mem);
                self.cycle_count += 4;
            }
            //ABS, X
            0x1D => {
                let arg = self.fetch_pc_word();
                let mem = self.read_addr_abs_x(arg, true);
                self.ora(mem);
            }
            //ABS, Y
            0x19 => {
                let arg = self.fetch_pc_word();
                let mem = self.read_addr_abs_y(arg, true);
                self.ora(mem);
            }
            //(IND, X)
            0x01 => {
                let arg = self.fetch_pc_byte();
                let mem = self.read_addr_indr_x(arg, true);
                self.ora(mem);
            }
            //(IND), Y
            0x11 => {
                let arg = self.fetch_pc_byte();
                let mem = self.read_addr_indr_y(arg, true);
                self.ora(mem);
            }
            //----PHA
            0x48 => {
                println!("PHA");
                self.push(self.reg.a);
                self.cycle_count += 3;
            }
            //---PHP
            0x08 => {
                println!("PHP");
                let packed = self.pack_flags(true, true);
                self.push(packed);
                self.cycle_count += 3;
            }
            //----PLA
            0x68 => {
                println!("PLA");
                self.reg.a = self.pop();
                self.cycle_count += 3;
            }
            //----PLP
            0x28 => {
                println!("PLP");
                let packed = self.pop();
                self.apply_packed(packed, true);
                self.cycle_count += 4;
            }
            //----ROL
            //A
            0x2A => {
                self.reg.a = self.rol(self.reg.a);
                self.cycle_count += 2;
            }
            //ZP
            0x26 => {
                let addr = self.fetch_pc_byte();
                let value = self.mem.read(addr as usize);
                let res = self.rol(value);
                self.mem.write(addr as usize, res);
                self.cycle_count += 5;
            }
            //ZP, X
            0x36 => {
                let arg = self.fetch_pc_byte();
                let value = self.read_addr_zp_x(arg, false);
                let res = self.rol(value);
                self.write_addr_zp_x(arg, res, false);
                self.cycle_count += 6;
            }
            //ABS
            0x2E => {
                let addr = self.fetch_pc_word();
                let value = self.mem.read(addr as usize);
                let res = self.rol(value);
                self.mem.write(addr as usize, res);
                self.cycle_count += 6;
            }
            //ABS, X
            0x3E => {
                let arg = self.fetch_pc_word();
                let value = self.read_addr_abs_x(arg, false);
                let res = self.rol(value);
                self.write_addr_abs_x(arg, res, false);
                self.cycle_count += 7;
            }
            //----ROR
            //A
            0x6A => {
                self.reg.a = self.ror(self.reg.a);
                self.cycle_count += 2;
            }
            //ZP
            0x66 => {
                let addr = self.fetch_pc_byte();
                let value = self.mem.read(addr as usize);
                let res = self.ror(value);
                self.mem.write(addr as usize, res);
                self.cycle_count += 5;
            }
            //ZP, X
            0x76 => {
                let arg = self.fetch_pc_byte();
                let value = self.read_addr_zp_x(arg, false);
                let res = self.ror(value);
                self.write_addr_zp_x(arg, res, false);
                self.cycle_count += 6;
            }
            //ABS
            0x6E => {
                let addr = self.fetch_pc_word();
                let value = self.mem.read(addr as usize);
                let res = self.ror(value);
                self.mem.write(addr as usize, res);
                self.cycle_count += 6;
            }
            //ABS, X
            0x7E => {
                let arg = self.fetch_pc_word();
                let value = self.read_addr_abs_x(arg, false);
                let res = self.ror(value);
                self.write_addr_abs_x(arg, res, false);
                self.cycle_count += 7;
            }
            //----RTI
            0x40 => {
                println!("RTI call");
                let packed = self.pop();
                self.apply_packed(packed, false);
                self.pop_pc_from_stack();
                self.cycle_count += 6;
            }
            //----RTS
            0x60 => {
                println!("RTS call");
                self.pop_pc_from_stack();
                self.reg.pc += 1;
                self.cycle_count += 6;
            }
            //--- SBC
            // Immediate
            0xE9 => {
                let immediate = self.fetch_pc_byte();
                self.sbc(immediate);
                self.cycle_count += 2;
            }
            //ZP
            0xE5 => {
                let addr = self.fetch_pc_byte();
                let mem = self.mem.read(addr as usize);
                self.sbc(mem);
                self.cycle_count += 3;
            }
            //ZP, X
            0xF5 => {
                let arg = self.fetch_pc_byte();
                let mem = self.read_addr_zp_x(arg, true);
                self.sbc(mem);
            }
            //ABS
            0xED => {
                let mem = self.fetch_abs_from_pc();
                self.sbc(mem);
                self.cycle_count += 4;
            }
            //ABS, X
            0xFD => {
                let arg = self.fetch_pc_word();
                let mem = self.read_addr_abs_x(arg, true);
                self.sbc(mem);
            }
            //ABS, Y
            0xF9 => {
                let arg = self.fetch_pc_word();
                let mem = self.read_addr_abs_y(arg, true);
                self.sbc(mem);
            }
            //(IND, X)
            0xE1 => {
                let arg = self.fetch_pc_byte();
                let mem = self.read_addr_indr_x(arg, true);
                self.sbc(mem);
            }
            //(IND), Y
            0xF1 => {
                let arg = self.fetch_pc_byte();
                let mem = self.read_addr_indr_y(arg, true);
                self.sbc(mem);
            }
            //----SEC
            0x38 => {
                self.reg.c = true;
                self.cycle_count += 2;
            }
            //----SED
            0xF8 => {
                self.reg.d = true;
                self.cycle_count += 2;
            }
            //----SEI
            0x78 => {
                self.reg.schedule_i = Some(true);
                self.cycle_count += 2;
            }
            //--- STA
            // ZP
            0x85 => {
                let addr = self.fetch_pc_byte();
                self.mem.write(addr as usize, self.reg.a);
                self.cycle_count += 3;
            }
            //ZP, X
            0x95 => {
                let arg = self.fetch_pc_byte();
                self.write_addr_zp_x(arg, self.reg.a, true);
            }
            //ABS
            0x8D => {
                let addr = self.fetch_pc_word();
                self.mem.write(addr as usize, self.reg.a);
                self.cycle_count += 4;
            }
            //ABS, X
            0x9D => {
                let arg = self.fetch_pc_word();
                self.write_addr_abs_x(arg, self.reg.a, true);
            }
            //ABS, Y
            0x99 => {
                let arg = self.fetch_pc_word();
                self.write_addr_abs_x(arg, self.reg.a, true);
            }
            //(IND, X)
            0x81 => {
                let arg = self.fetch_pc_byte();
                self.write_addr_indr_x(arg, self.reg.a, true);
            }
            //(IND), Y
            0x91 => {
                let arg = self.fetch_pc_byte();
                self.write_addr_indr_y(arg, self.reg.a, true);
            }
            //--- STX
            // ZP
            0x86 => {
                let addr = self.fetch_pc_byte();
                self.mem.write(addr as usize, self.reg.x);
                self.cycle_count += 3;
            }
            //ZP, Y
            0x96 => {
                let arg = self.fetch_pc_byte();
                self.write_addr_zp_y(arg, self.reg.x, true);
            }
            //ABS
            0x8E => {
                let addr = self.fetch_pc_word();
                self.mem.write(addr as usize, self.reg.x);
                self.cycle_count += 4;
            }
            //--- STY
            // ZP
            0x84 => {
                let addr = self.fetch_pc_byte();
                self.mem.write(addr as usize, self.reg.y);
                self.cycle_count += 3;
            }
            //ZP, X
            0x94 => {
                let arg = self.fetch_pc_byte();
                self.write_addr_zp_x(arg, self.reg.y, true);
            }
            //ABS
            0x8C => {
                let addr = self.fetch_pc_word();
                self.mem.write(addr as usize, self.reg.y);
                self.cycle_count += 4;
            }
            //----TAX
            0xAA => {
                self.reg.x = self.reg.a;
                self.reg.z = self.reg.x == 0;
                self.reg.n = self.reg.x & 0x80 != 0;
                self.cycle_count += 2;
            }
            //----TAY
            0xA8 => {
                self.reg.y = self.reg.a;
                self.reg.z = self.reg.y == 0;
                self.reg.n = self.reg.y & 0x80 != 0;
                self.cycle_count += 2;
            }
            //----TSX
            0xBA => {
                self.reg.x = self.reg.sp;
                self.reg.z = self.reg.x == 0;
                self.reg.n = self.reg.x & 0x80 != 0;
                self.cycle_count += 2;
            }
            //----TXA
            0x8A => {
                self.reg.a = self.reg.x;
                self.reg.z = self.reg.a == 0;
                self.reg.n = self.reg.a & 0x80 != 0;
                self.cycle_count += 2;
            }
            //----TXS
            0x9A => {
                self.reg.sp = self.reg.x;
                self.cycle_count += 2;
            }
            //----TYA
            0x98 => {
                self.reg.a = self.reg.y;
                self.reg.z = self.reg.a == 0;
                self.reg.n = self.reg.a & 0x80 != 0;
                self.cycle_count += 2;
            }
            _ => {
                println!("Unimplemented instruction {instr} at pc {}", self.reg.pc)
            }
        }
    }

    pub fn mem_mut(&mut self) -> &mut Memory<0x10000> {
        &mut self.mem
    }

    pub fn cycle_count_mut(&mut self) -> &mut usize {
        &mut self.cycle_count
    }
}
