use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use crate::{
    controller::{ConrollerMemory, Controller}, memory::{DummyMem, InternalRam, MemoryBus, MemoryRegion}, ppu::{PPU, PPUOamDmaRegisterMap, PPURegistersMap},
};

pub struct Registers {
    addr: u8,
    page: u8,
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
            addr: 0,
            page: 0,
        }
    }
}

pub struct CPU {
    mem: MemoryBus<0x10000>,
    reg: Registers,
    action_queue: VecDeque<CpuAction>,
    cycle_count: usize,
    dma: Rc<RefCell<Option<Dma>>>,
    internal_bus: u8,
    //Any instruction modifying a value from the internal bus cannot directly write it to the bus and will thus save it here for the next cycle
    transformed: u8,
    word_read: u8,
    nmi: bool,
}

pub struct Dma {
    dma_page: u8,
    dma_addr: u8,
    dma_dest: usize,
    dma_buf: Option<u8>,
}

impl Dma {
    pub fn new(dma_page: u8, dma_dest: usize) -> Self {
        Self { dma_page, dma_dest, dma_buf: None, dma_addr: 0 }
    }
}

impl CPU {

    fn alu_immediate(&mut self, instr: InternalBusRead) {
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![instr], }
        );
    }

    fn alu_single(&mut self, instr: InternalBusRead) {
        self.action_queue.push_back(
            CpuAction {cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::DummyInstr,  internal_bus_put: InternalBusPut::A, internal_bus_read: vec![instr],  }
        );
    }

    fn alu_zp(&mut self, instr: InternalBusRead) {
        //First (instr) => Bus Addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusAddr], }
        );
        //Second (Bus addr) => instr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Zp, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![instr], }
        );
    }

    fn write_zp(&mut self, value: InternalBusPut) {
        //First (instr) => Bus Addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusAddr], }
        );
        //Second value => (bus addr)
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Write, cpu_bus_addr: MemBusAddr::Zp, internal_bus_put: value, internal_bus_read: vec![InternalBusRead::MemBus], }
        );
    }

    fn rmw_zp(&mut self, instr: InternalBusRead) {
        //First (instr) => Bus Addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusAddr], }
        );

        //Second (Bus addr) => transform
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Zp, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::Transform], }
        );
        //Third transform => (Bus addr), (instr => transform) dummy write
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Write, cpu_bus_addr: MemBusAddr::Zp, internal_bus_put: InternalBusPut::Transformed, internal_bus_read: vec![InternalBusRead::MemBus, instr], }
        );
        //Fourth transform => (Bus addr)
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Write, cpu_bus_addr: MemBusAddr::Zp, internal_bus_put: InternalBusPut::Transformed, internal_bus_read: vec![InternalBusRead::MemBus], }
        );
    }

    fn alu_zp_x(&mut self, instr: InternalBusRead) {
        //First (instr) => Bus Addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusAddr], }
        );
        //Second (Bus addr), x => Transform addr add => Bus addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Zp, internal_bus_put: InternalBusPut::X, internal_bus_read: vec![InternalBusRead::BusAddrAdd(self.reg.x)], }
        );
        //Third (Bus Addr) => instr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Zp, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![instr], }
        );
    }

    fn write_zp_x(&mut self, value: InternalBusPut) {
        //First (instr) => Bus Addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusAddr], }
        );
        //Second (Bus addr), x => Transform addr add => Bus addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Zp, internal_bus_put: InternalBusPut::X, internal_bus_read: vec![InternalBusRead::BusAddrAdd(self.reg.x)], }
        );
        //Third value => (bus addr)
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Write, cpu_bus_addr: MemBusAddr::Zp, internal_bus_put: value, internal_bus_read: vec![InternalBusRead::MemBus], }
        );
    }

    fn rmw_zp_x(&mut self, instr: InternalBusRead) {
        //First (instr) => Bus Addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusAddr], }
        );
        //Second (Bus addr), x => Transform addr add => Bus addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Zp, internal_bus_put: InternalBusPut::X, internal_bus_read: vec![InternalBusRead::BusAddrAdd(self.reg.x)], }
        );
        //Third (Bus addr) => transform
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Zp, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::Transform], }
        );
        //Fourth transform => (Bus addr), (instr => transform) dummy write
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Write, cpu_bus_addr: MemBusAddr::Zp, internal_bus_put: InternalBusPut::Transformed, internal_bus_read: vec![InternalBusRead::MemBus, instr], }
        );
        //Fifth transform => (Bus addr)
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Write, cpu_bus_addr: MemBusAddr::Zp, internal_bus_put: InternalBusPut::Transformed, internal_bus_read: vec![InternalBusRead::MemBus], }
        );
    }

    fn alu_zp_y(&mut self, instr: InternalBusRead) {
        //First (instr) => Bus Addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusAddr], }
        );
        //Second (Bus addr), y => Transform addr add => Bus addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Zp, internal_bus_put: InternalBusPut::Y, internal_bus_read: vec![InternalBusRead::BusAddrAdd(self.reg.y)], }
        );
        //Third (Bus Addr) => instr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Zp, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![instr], }
        );
    }

    fn write_zp_y(&mut self, value: InternalBusPut) {
        //First (instr) => Bus Addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusAddr], }
        );
        //Second (Bus addr), y => Transform addr add => Bus addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Zp, internal_bus_put: InternalBusPut::Y, internal_bus_read: vec![InternalBusRead::BusAddrAdd(self.reg.y)], }
        );
        //Third value => (Bus addr)
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Write, cpu_bus_addr: MemBusAddr::Zp, internal_bus_put: value, internal_bus_read: vec![InternalBusRead::MemBus], }
        );
    }

    fn alu_abs(&mut self, instr: InternalBusRead) {
        //First (instr) => Bus Addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusAddr], }
        );
        //Second (instr) => Bus page
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusPage], }
        );
        //Third (bus page, bus addr) => instr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Abs, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![instr], }
        );
    }

    fn write_abs(&mut self, value: InternalBusPut) {
        //First (instr) => Bus Addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusAddr], }
        );
        //Second (instr) => Bus Page
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusPage], }
        );
        //Third value => (Bus page, Bus Addr)
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Write, cpu_bus_addr: MemBusAddr::Abs, internal_bus_put: value, internal_bus_read: vec![InternalBusRead::MemBus], }
        );
    }

    fn rmw_abs(&mut self, instr: InternalBusRead) {
        //First (instr) => Bus Addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusAddr], }
        );
        //Second (instr) => bus page
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusPage], }
        );
        //Third (Bus page, Bus addr) => transform
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Abs, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::Transform], }
        );
        //Fourth transform => (Bus page, Bus addr), (instr => transform) dummy write
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Write, cpu_bus_addr: MemBusAddr::Abs, internal_bus_put: InternalBusPut::Transformed, internal_bus_read: vec![InternalBusRead::MemBus, instr], }
        );
        //Fifth transform => (Bus page, Bus addr)
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Write, cpu_bus_addr: MemBusAddr::Abs, internal_bus_put: InternalBusPut::Transformed, internal_bus_read: vec![InternalBusRead::MemBus], }
        );
    }

    fn alu_abs_x(&mut self, instr: InternalBusRead) {
        //First (instr) => Bus Addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusAddr], }
        );
        //Second (instr) => bus page, (Transform addr carry add => Bus addr)
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusPage, InternalBusRead::BusAddrAddCarry(self.reg.x, false)], }
        );
        //If carry (handled by Transform add addr carry): third (bus page, bus addr), inc bus page
        //Fourth (bus page, bus addr), internal => instr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Abs, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![instr], }
        );
    }

    fn write_abs_x(&mut self, value: InternalBusPut) {
        //First (instr) => Bus Addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusAddr], }
        );
        //Second (instr) => bus page, Addr carry add => Bus addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusPage, InternalBusRead::BusAddrAddCarry(self.reg.x, true)], }
        );
        //Third forced oops cycle. (bus page, bus addr), (on overflow inc bus page)
        //Fourth value => (Bus page, Bus Addr)
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Write, cpu_bus_addr: MemBusAddr::Abs, internal_bus_put: value, internal_bus_read: vec![InternalBusRead::MemBus], }
        );
    }

    fn rmw_abs_x(&mut self, instr: InternalBusRead) {
        //First (instr) => Bus Addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusAddr], }
        );
        //Second (instr) => bus page, Addr carry add => Bus addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusPage, InternalBusRead::BusAddrAddCarry(self.reg.x, true)], }
        );
        //Third forced oops cycle. (bus page, bus addr), (on overflow inc bus page)
        //Fourth (Bus page, Bus addr) => transform
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Abs, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::Transform], }
        );
        //Fifth transform => (Bus page, Bus addr), (instr => transform) dummy write
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Write, cpu_bus_addr: MemBusAddr::Abs, internal_bus_put: InternalBusPut::Transformed, internal_bus_read: vec![InternalBusRead::MemBus, instr], }
        );
        //Sixth transform => (Bus page, Bus addr)
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Write, cpu_bus_addr: MemBusAddr::Abs, internal_bus_put: InternalBusPut::Transformed, internal_bus_read: vec![InternalBusRead::MemBus], }
        );
    }

    fn alu_abs_y(&mut self, instr: InternalBusRead) {
        //First (instr) => Bus Addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusAddr], }
        );
        //Second (instr), y => Transform addr carry add => Bus addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusPage, InternalBusRead::BusAddrAddCarry(self.reg.y, false)], }
        );
        //If carry (handled by Transform add addr carry): third (bus page, bus addr), inc bus page
        //Fourth (bus page, bus addr), internal => instr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Abs, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![instr], }
        );
    }

    fn write_abs_y(&mut self, value: InternalBusPut) {
        //First (instr) => Bus Addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusAddr], }
        );
        //Second (instr) => bus page, Addr carry add => Bus addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusPage, InternalBusRead::BusAddrAddCarry(self.reg.y , true)], }
        );
        //Third forced oops cycle. (bus page, bus addr), (on overflow inc bus page)
        //Fourth value => (Bus page, Bus Addr)
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Write, cpu_bus_addr: MemBusAddr::Abs, internal_bus_put: value, internal_bus_read: vec![InternalBusRead::MemBus], }
        );
    }

    fn alu_indr_x(&mut self, instr: InternalBusRead) {
        //First (instr) => Bus Addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusAddr], }
        );
        //Second (bus addr), x => Word read add => Word read 
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Zp, internal_bus_put: InternalBusPut::X, internal_bus_read: vec![InternalBusRead::WordReadAdd], }
        );
        //Third (word read++) => bus addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::WordRead, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusAddr], }
        );
        //Fourth (word read++) => bus page
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::WordRead, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusPage], }
        );
        //Fifth (bus page, bus addr), internal => instr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Abs, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![instr], }
        );
    }
    fn write_indr_x(&mut self, value: InternalBusPut) {
        //First (instr) => Bus Addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusAddr], }
        );
        //Second (bus addr), x => Word read add => Word read 
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Zp, internal_bus_put: InternalBusPut::X, internal_bus_read: vec![InternalBusRead::WordReadAdd], }
        );
        //Third (word read++) => bus addr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::WordRead, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusAddr], }
        );
        //Fourth (word read++) => bus page
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::WordRead, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusPage], }
        );
        //Fifth , internal => instr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Write, cpu_bus_addr: MemBusAddr::Abs, internal_bus_put: value, internal_bus_read: vec![InternalBusRead::MemBus], }
        );
    }

    fn alu_indr_y(&mut self, instr: InternalBusRead) {
        //First (instr) => WordRead
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::WordRead], }
        );
        //Second (word read++), Bus addr 
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::WordRead, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusAddr], }
        );
        //Third (word read++) => bus page, y + bus addr => bus addr (on carry oops cycle)
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::WordRead, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusPage, InternalBusRead::BusAddrAddCarry(self.reg.y, false)], }
        );
        //If carry (handled by Transform add addr carry): inc bus page
        //Fourth/Fifth (bus page, bus addr) => instr
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Abs, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![instr], }
        );

    }

    fn write_indr_y(&mut self, value: InternalBusPut) {
        //First (instr) => WordRead
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::WordRead], }
        );
        //Second (word read++), Bus addr 
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::WordRead, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusAddr], }
        );
        //Third (word read++) => bus page, y + bus addr => bus addr (on carry oops cycle)
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::WordRead, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusPage, InternalBusRead::BusAddrAddCarry(self.reg.y, true)], }
        );
        //Fourth (bus page, bus addr), inc bus page on carry
        //Fifth value => (bus page, bus addr)
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Write, cpu_bus_addr: MemBusAddr::Abs, internal_bus_put: value, internal_bus_read: vec![InternalBusRead::MemBus], }
        );

    }

    pub fn cycle_count(&self) -> usize {
        self.cycle_count
    }

    pub fn start_with_memory(mem: MemoryBus<0x10000>, dma: Rc<RefCell<Option<Dma>>>) -> Self {
        let mut res = Self {
            mem,
            reg: Registers::new(),
            cycle_count: 0,
            dma: dma,
            action_queue: VecDeque::new(),
            internal_bus: 0,
            transformed: 0,
            word_read: 0,
            nmi: false,
        };
        res.reset();
        res
    }

    //Creates the default cpu memory with the cartridge memory represented at 0x4020-0x10000
    pub fn with_cartridge_memory(
        mut mem: Vec<Box<dyn MemoryRegion>>,
        ppu: &PPU,
        controller: Rc<RefCell<Controller>>

    ) -> Self {
        let dma: Rc<RefCell<Option<Dma>>> = Default::default();
        let mut default_mem: Vec<Box<dyn MemoryRegion>> = vec![
            Box::new(InternalRam::default()),
            Box::new(PPURegistersMap::new(ppu.reg())),
            Box::new(DummyMem::new((0x4000..0x4014).into())),
            Box::new(PPUOamDmaRegisterMap::new(ppu.reg(), dma.clone())),
            Box::new(DummyMem::new((0x4015..0x4016).into())),
            Box::new(ConrollerMemory::new(controller)),
            Box::new(DummyMem::new((0x4018..0x4020).into())),
        ];
        default_mem.append(&mut mem);
        let mem = MemoryBus::construct(default_mem).unwrap();
        Self::start_with_memory(mem, dma)
    }

    pub fn reset(&mut self) {
        self.reg.sp = self.reg.sp.wrapping_sub(3);
        self.reg.pc = self.deref_word_at(0xFFFC);
        self.reg.i = true;
    }


    fn deref_word_at(&mut self, ptr: u16) -> u16 {
        let lo = self.mem.read(ptr as usize);
        let hi = self.mem.read(ptr.wrapping_add(1) as usize);
        let res = u16::from_le_bytes([lo, hi]);
        res
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
        //println!("adc {mem}");
        let res = self.reg.a as u16 + mem as u16 + if self.reg.c { 1 } else { 0 };
        self.reg.c = res > 0xFF;
        self.reg.z = res & 0xFF == 0;
        self.reg.n = res & 0x80 != 0; 
        //self.set_flags(true, true, true, res);
        let res = (res & 0xFF) as u8;
        self.reg.v = (res ^ self.reg.a) & (res ^ mem) & 0x80 != 0;
        self.reg.a = res;
    }

    fn and(&mut self, mem: u8) {
        //println!("and {mem}");
        self.reg.a = self.reg.a & mem;
        self.set_flags(false, true, true, self.reg.a as u16);
    }

    fn asl(&mut self, value: u8) -> u8 {
        //println!("asl {value}");
        self.reg.c = value & 0x80 != 0;
        let res = value << 1;
        self.reg.z = res == 0;
        self.reg.n = res & 0x80 != 0;
        res
    }

    fn lsr(&mut self, value: u8) -> u8 {
        //println!("lsr {value}");
        self.reg.c = value & 0x01 != 0;
        let res = value >> 1;
        self.reg.z = res == 0;
        self.reg.n = false;
        res
    }

    fn ror(&mut self, value: u8) -> u8 {
        //println!("ror {value}");
        let old_c = self.reg.c;
        self.reg.c = value & 0x01 != 0;
        let res = value >> 1 | if old_c {0x80} else {0};
        self.reg.z = res == 0;
        self.reg.n = res & 0x80 != 0;
        res
    }

    fn rol(&mut self, value: u8) -> u8 {
        //println!("rol {value}");
        let old_c = self.reg.c;
        self.reg.c = value & 0x80 != 0;
        let res = value << 1 | if old_c {1} else {0};
        self.reg.z = res == 0;
        self.reg.n = res & 0x80 != 0;
        res
    }

    fn dec(&mut self, value: u8) -> u8 {
        //println!("dec {value}");
        let res = value.wrapping_sub(1);
        self.reg.z = res == 0;
        self.reg.n = res & 0x80 != 0;
        res
    }

    fn inc(&mut self, value: u8) -> u8 {
        //println!("inc {value}");
        let res = value.wrapping_add(1);
        self.reg.z = res == 0;
        self.reg.n = res & 0x80 != 0;
        res
    }

    fn bit(&mut self, mem: u8) {
        //println!("bit {mem}");
        let v = self.reg.a & mem;
        self.reg.z = v == 0;
        self.reg.n = mem & 0x80 != 0;
        self.reg.v = mem & 0x40 != 0;
    }

    fn cmp(&mut self, mem: u8) {
        //println!("cmp {mem}");
        self.reg.c = self.reg.a >= mem;
        self.reg.z = self.reg.a == mem;
        self.reg.n = self.reg.a.wrapping_sub(mem) & 0x80 != 0;
    }

    fn cpx(&mut self, mem: u8) {
        //println!("cpx {mem}");
        self.reg.c = self.reg.x >= mem;
        self.reg.z = self.reg.x == mem;
        self.reg.n = self.reg.x.wrapping_sub(mem) & 0x80 != 0;
    }

    fn cpy(&mut self, mem: u8) {
        //println!("cpy {mem}");
        self.reg.c = self.reg.y >= mem;
        self.reg.z = self.reg.y == mem;
        self.reg.n = self.reg.y.wrapping_sub(mem) & 0x80 != 0;
    }

    fn eor(&mut self, mem: u8) {
        //println!("eor {mem}");
        let res = self.reg.a ^ mem;
        self.reg.z = res == 0;
        self.reg.n = res & 0x80 != 0;
        self.reg.a = res;
    }

    fn lda(&mut self, mem: u8) {
        //println!("lda {mem}");
        self.reg.z = mem == 0;
        self.reg.n = mem & 0x80 != 0;
        self.reg.a = mem;
    }

    fn ldx(&mut self, mem: u8) {
        //println!("ldx {mem}");
        self.reg.z = mem == 0;
        self.reg.n = mem & 0x80 != 0;
        self.reg.x = mem;
    }

    fn ldy(&mut self, mem: u8) {
        //println!("ldy {mem}");
        self.reg.z = mem == 0;
        self.reg.n = (mem & 0x80) != 0;
        self.reg.y = mem;
    }

    fn ora(&mut self, mem: u8) {
        //println!("ora {mem}");
        self.reg.a = self.reg.a | mem;
        self.reg.z = self.reg.a == 0;
        self.reg.n = self.reg.a & 0x80 != 0;
    }

    fn sbc(&mut self, mem: u8) {
        //println!("sbc {mem}");
        let res = self.reg.a as i16 - mem as i16 - if self.reg.c { 0 } else { 1 };
        self.reg.c = res >= 0;
        let res = (res & 0x00FF) as u8;
        self.reg.z = res == 0;
        self.reg.v = (res ^ self.reg.a) & (res ^ !mem) & 0x80 != 0;
        self.reg.n = 0x80 & res != 0;
        self.reg.a = res;
    }

    fn branch_if_condition(&mut self, v: bool) {
        //First (instr) => Transform ; relative offset
        self.action_queue.push_back(
            CpuAction { cpu_bus_addr: MemBusAddr::Instr, cycle_type: CycleType::Read, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::Transform ],  }
        );
        if !v {
            return;
        }
        //Second (instr dummy), Tramsform => BranchRelative
        self.action_queue.push_back(
            CpuAction { cpu_bus_addr: MemBusAddr::DummyInstr, cycle_type: CycleType::Read, internal_bus_put: InternalBusPut::Transformed, internal_bus_read: vec![InternalBusRead::BranchRelative],  }
        );
        //On page crossed: (instr dummy), Relative Carry => SP
    }

    fn dummy_read_instr(&mut self) {
        self.action_queue.push_back(
            CpuAction { internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![], cpu_bus_addr: MemBusAddr::DummyInstr, cycle_type: CycleType::Read }
        );
    }
    fn dummy_read_stack(&mut self) {
        self.action_queue.push_back(
            CpuAction { internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![], cpu_bus_addr: MemBusAddr::DummyStack, cycle_type: CycleType::Read }
        );
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
        self.nmi = true;
    }

    fn get_cpu_bus_addr(&mut self, addr: MemBusAddr) -> u16 {
        match addr {
            MemBusAddr::Const(c) => c,
            MemBusAddr::Abs => {
                u16::from_le_bytes([self.reg.addr, self.reg.page])
            }
            MemBusAddr::Zp => {
                self.reg.addr as u16
            }
            MemBusAddr::Instr => {
                let res = self.reg.pc;
                self.reg.pc = self.reg.pc.wrapping_add(1);
                res
            }
            MemBusAddr::DummyInstr => {
                self.reg.pc
            }
            MemBusAddr::WordRead => {
                let res = self.word_read;
                self.word_read = self.word_read.wrapping_add(1);
                res as u16
            }
            MemBusAddr::StackPull => {
                self.reg.sp = self.reg.sp.wrapping_add(1);
                self.reg.sp as u16 + 0x100
            }
            MemBusAddr::StackPush => {
                let res = self.reg.sp as u16 + 0x100;
                self.reg.sp = self.reg.sp.wrapping_sub(1);
                res
            }
            MemBusAddr::DummyStack => self.reg.sp as u16 + 0x100,
            MemBusAddr::None => unreachable!()
        }
    }

    fn brk_routine(&mut self, b_flag: bool) {
        //Push(PcHi)
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Write, cpu_bus_addr: MemBusAddr::StackPush, internal_bus_put: InternalBusPut::PcHi, internal_bus_read: vec![InternalBusRead::MemBus],  }
        );
        //Push(PcLo)
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Write, cpu_bus_addr: MemBusAddr::StackPush, internal_bus_put: InternalBusPut::PcLo, internal_bus_read: vec![InternalBusRead::MemBus],  }
        );
        //Push(Flags), schedule iv read (reads nmi vector if an nmi is available, which would skip a brk or irq. This is nes behaviour), set interrupt disable flag
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Write, cpu_bus_addr: MemBusAddr::StackPush, internal_bus_put: InternalBusPut::Flags(true, b_flag), internal_bus_read: vec![InternalBusRead::MemBus, InternalBusRead::ReadIV, InternalBusRead::SEI],  }
        );
    }

    fn jump_vector(&mut self, pos: u16) {
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Const(pos), internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::PcLo],  }
        );
        self.action_queue.push_back(
            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Const(pos.wrapping_add(1)), internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::PcHi],  }
        );
    }

    pub fn cycle(&mut self) {
        self.cycle_count += 1;
        {
            let mut dma_container = self.dma.borrow_mut();
            if let Some(dma) = dma_container.as_mut() {
                let addr = (dma.dma_page as usize) << 8 | (dma.dma_addr as usize);
                if let Some(data) = dma.dma_buf.take() {
                    self.mem.write(dma.dma_dest, data);
                    if dma.dma_addr == 0xFF {
                        *dma_container = None;
                        return;
                    }
                    dma.dma_addr += 1;
                    return;
                }
                //We cannot read on write cycles
                if self.cycle_count % 2 == 1 {
                    return;
                }
                if dma.dma_addr == 0 {
                    //Emulate init cycle
                    self.cycle_count += 1;
                }
                dma.dma_buf = Some(self.mem.read(addr));
                return;
            }
        }
        if self.action_queue.is_empty() {
            if let Some(sched_i) = self.reg.schedule_i.take() {
                self.reg.i = sched_i;
            }
            if self.nmi {
                self.dummy_read_instr();
                self.dummy_read_instr();
                self.brk_routine(false);
            } else {
                self.action_queue.push_back(CpuAction::fetch_decode_instruction_action());
            }
        }

        let action = self.action_queue.pop_front().unwrap();
        //println!("{:#?}", action);
        if let CycleType::Read = action.cycle_type {
            let addr = self.get_cpu_bus_addr(action.cpu_bus_addr);
            self.mem.read(addr as usize);
        }

        self.internal_bus = match action.internal_bus_put {
            InternalBusPut::A => self.reg.a,
            InternalBusPut::X => self.reg.x,
            InternalBusPut::Y => self.reg.y,
            InternalBusPut::Transformed => self.transformed,
            InternalBusPut::BusAddr => self.reg.addr,
            InternalBusPut::BusPage => self.reg.page,
            InternalBusPut::MemBus => self.mem.bus(),
            InternalBusPut::Const(c) => c,
            InternalBusPut::Flags(b5, b4) => self.pack_flags(b5, b4),
            InternalBusPut::PcHi => (self.reg.pc >> 8) as u8,
            InternalBusPut::PcLo => (self.reg.pc & 0x00FF) as u8
        };

        for read in action.internal_bus_read {
            match read {
                InternalBusRead::A => self.reg.a = self.internal_bus,
                InternalBusRead::Y => self.reg.y = self.internal_bus,
                InternalBusRead::X => self.reg.x = self.internal_bus,
                InternalBusRead::PcHi => self.reg.pc = (self.reg.pc & 0x00FF) | ((self.internal_bus as u16) << 8),
                InternalBusRead::PcLo => self.reg.pc = (self.reg.pc & 0xFF00) | (self.internal_bus as u16),
                InternalBusRead::MemBusAddrToPc => {
                    self.reg.pc = self.get_cpu_bus_addr(MemBusAddr::Abs)
                }
                InternalBusRead::MemBus => {
                    assert_eq!(action.cycle_type, CycleType::Write);
                    let addr = self.get_cpu_bus_addr(action.cpu_bus_addr);
                    self.mem.write(addr as usize, self.internal_bus);
                },
                InternalBusRead::InstrDecode => {
                    self.decode_opcode(self.internal_bus);
                }
                InternalBusRead::BusAddr => {
                    self.reg.addr = self.internal_bus;
                }
                InternalBusRead::BusAddrAdd(arg) => {
                    self.reg.addr = self.reg.addr.wrapping_add(arg)
                }
                InternalBusRead::BusAddrAddCarry(arg, force_oops) => {
                    let (new_addr, overflow) = self.reg.addr.overflowing_add(arg);
                    self.reg.addr = new_addr;
                    if overflow || force_oops {
                        self.action_queue.push_front(
                            CpuAction { internal_bus_put: InternalBusPut::Const(0), internal_bus_read: vec![InternalBusRead::BusPageAdd(if overflow {1} else {0})], cpu_bus_addr: MemBusAddr::Abs, cycle_type: CycleType::Read }
                        );
                    }
                }
                InternalBusRead::BusPage => {
                    self.reg.page = self.internal_bus;
                }
                InternalBusRead::BusPageAdd(arg) => {
                    self.reg.page = self.reg.page.wrapping_add(arg)
                }
                InternalBusRead::WordRead => {
                    self.word_read = self.internal_bus
                }
                InternalBusRead::WordReadAdd => {
                    self.word_read = self.internal_bus.wrapping_add(self.reg.addr);
                }
                InternalBusRead::ReadIV => {
                    if self.nmi {
                        self.dump_memory(0x100, 0x100);
                        self.jump_vector(0xFFFA);
                    } else {
                        self.jump_vector(0xFFFE);
                    }
                    self.nmi = false;
                }
                InternalBusRead::ADC => self.adc(self.internal_bus),
                InternalBusRead::AND => self.and(self.internal_bus),
                InternalBusRead::HardwareASL => self.reg.a = self.asl(self.internal_bus),
                InternalBusRead::ASL => self.transformed = self.asl(self.internal_bus),
                InternalBusRead::Transform => self.transformed = self.internal_bus,
                InternalBusRead::BranchRelative => {
                    let relative = self.internal_bus.cast_signed();
                    let new_pc = self.reg.pc.wrapping_add_signed(relative as i16);
                    self.reg.pc = (self.reg.pc & 0xFF00) | (new_pc & 0x00FF);
                    if self.reg.pc & 0xFF00 != new_pc & 0xFF00 {
                        self.action_queue.push_front(
                            CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::DummyInstr, internal_bus_put: InternalBusPut::Const(relative.signum().cast_unsigned()), internal_bus_read: vec![InternalBusRead::CorrectPcPage],  }
                        );
                    }
                }
                InternalBusRead::CorrectPcPage => self.reg.pc = self.reg.pc.wrapping_add((self.internal_bus as u16) << 8),
                InternalBusRead::BIT => self.bit(self.internal_bus),
                InternalBusRead::CMP => self.cmp(self.internal_bus),
                InternalBusRead::CPX => self.cpx(self.internal_bus),
                InternalBusRead::CPY => self.cpy(self.internal_bus),
                InternalBusRead::DEC => self.transformed = self.dec(self.internal_bus),
                InternalBusRead::EOR => self.eor(self.internal_bus),
                InternalBusRead::INC => self.transformed = self.inc(self.internal_bus),
                InternalBusRead::LDA => self.lda(self.internal_bus),
                InternalBusRead::LDX => self.ldx(self.internal_bus),
                InternalBusRead::LDY => self.ldy(self.internal_bus),
                InternalBusRead::HardwareLSR => self.reg.a = self.lsr(self.internal_bus),
                InternalBusRead::LSR => self.transformed = self.lsr(self.internal_bus),
                InternalBusRead::ORA => self.ora(self.internal_bus),
                InternalBusRead::SEI => self.reg.i = true,
                InternalBusRead::Flags(sched_i) => self.apply_packed(self.internal_bus, sched_i),
                InternalBusRead::HardwareROL => self.reg.a = self.rol(self.internal_bus),
                InternalBusRead::ROL => self.transformed = self.rol(self.internal_bus),
                InternalBusRead::HardwareROR => self.reg.a = self.ror(self.internal_bus),
                InternalBusRead::ROR => self.transformed = self.ror(self.internal_bus),
                InternalBusRead::SBC => self.sbc(self.internal_bus),
                InternalBusRead::UpdateZN => {
                    self.reg.z = self.internal_bus == 0;
                    self.reg.n = self.internal_bus & 0x80 != 0;
                }
            }
        }
    
    }


    pub fn decode_opcode(&mut self, instr: u8) {
        
        //println!("Decoding opcode {instr} at {}", self.reg.pc);
        if self.reg.pc == 34151 {
            println!("brk");
        }
        match instr {
            //--- ADC
            // Immediate
            0x69 => self.alu_immediate(InternalBusRead::ADC),
            //ZP
            0x65 => self.alu_zp(InternalBusRead::ADC),
            //ZP, X
            0x75 => self.alu_zp_x(InternalBusRead::ADC),
            //ABS
            0x6D => self.alu_abs(InternalBusRead::ADC),
            //ABS, X
            0x7D => self.alu_abs_x(InternalBusRead::ADC),
            //ABS, Y
            0x79 => self.alu_abs_y(InternalBusRead::ADC),
            //(IND, X)
            0x61 => self.alu_indr_x(InternalBusRead::ADC),
            //(IND), Y
            0x71 => self.alu_indr_y(InternalBusRead::ADC),
            //-----AND
            //IM
            0x29 => self.alu_immediate(InternalBusRead::AND),
            //ZP
            0x25 => self.alu_zp(InternalBusRead::AND),
            //ZP, X
            0x35 => self.alu_zp_x(InternalBusRead::AND),
            //ABS
            0x2D => self.alu_abs(InternalBusRead::AND),
            //ABS, X
            0x3D => self.alu_abs_x(InternalBusRead::AND),
            //ABS, Y
            0x39 => self.alu_abs_y(InternalBusRead::AND),
            //(IND, X)
            0x21 => self.alu_indr_x(InternalBusRead::AND),
            //(IND), Y
            0x31 => self.alu_indr_y(InternalBusRead::AND),
            //-----ASL
            //A
            0x0A => self.alu_single(InternalBusRead::HardwareASL),
            //ZP
            0x06 => self.rmw_zp(InternalBusRead::ASL),
            //ZP, X
            0x16 => self.rmw_zp_x(InternalBusRead::ASL),
            //ABS
            0x0E => self.rmw_abs(InternalBusRead::ASL),
            //ABS, X
            0x1E => self.rmw_abs_x(InternalBusRead::ASL),
            //----BCC
            0x90 => self.branch_if_condition(!self.reg.c),
            //----BCS
            0xB0 => self.branch_if_condition(self.reg.c),
            
            //----BEQ
            0xF0 => self.branch_if_condition(self.reg.z),
            //----BIT
            //ZP
            0x24 => self.alu_zp(InternalBusRead::BIT),
            //ABS
            0x2C => self.alu_abs(InternalBusRead::BIT),
            //----BMI
            0x30 => self.branch_if_condition(self.reg.n),
            
            //----BNE
            0xD0 => self.branch_if_condition(!self.reg.z),
            
            //----BPL
            0x10 => self.branch_if_condition(!self.reg.n),
            
            //----BRK
            //TODO: Implement 6502 bug where simultaneous IRQ and NMI skip IRQ handler
            0x00 => {
                //(instr) => _
                self.action_queue.push_back(
                    CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![],  }
                );
                self.brk_routine(true);
            }
            //----BVC
            0x50 => self.branch_if_condition(!self.reg.v),
            
            //----BVS
            0x70 => self.branch_if_condition(self.reg.v),
            
            //----CLC
            0x18 => {
                self.dummy_read_instr();
                self.reg.c = false;
            }
            //----CLD
            0xD8 => {
                self.dummy_read_instr();
                self.reg.d = false;
            }
            //----CLI
            0x58 => {
                self.dummy_read_instr();
                self.reg.schedule_i = Some(false);
            }
            //----CLV
            0xB8 => {
                self.dummy_read_instr();
                self.reg.v = false;
            }
            //----CMP
            //Im
            0xC9 => self.alu_immediate(InternalBusRead::CMP),
            //ZP
            0xC5 => self.alu_zp(InternalBusRead::CMP),
            //ZP, X
            0xD5 => self.alu_zp_x(InternalBusRead::CMP),
            //Abs
            0xCD => self.alu_abs(InternalBusRead::CMP),
            //Abs, X
            0xDD => self.alu_abs_x(InternalBusRead::CMP),
            //Abs, Y
            0xD9 => self.alu_abs_y(InternalBusRead::CMP),
            //Ind, X
            0xC1 => self.alu_indr_x(InternalBusRead::CMP),
            //Ind, Y
            0xD1 => self.alu_indr_y(InternalBusRead::CMP),
            //----CPX
            //Im
            0xE0 => self.alu_immediate(InternalBusRead::CPX),
            //ZP
            0xE4 => self.alu_zp(InternalBusRead::CPX),
            //ABS
            0xEC => self.alu_abs(InternalBusRead::CPX),
            //----CPY
            //Im
            0xC0 => self.alu_immediate(InternalBusRead::CPY),
            //ZP
            0xC4 => self.alu_zp(InternalBusRead::CPY),
            //ABS
            0xCC => self.alu_abs(InternalBusRead::CPY),
            //----DEC
            //ZP
            0xC6 => self.rmw_zp(InternalBusRead::DEC),
            //ZP, X
            0xD6 => self.rmw_zp_x(InternalBusRead::DEC),
            //ABS
            0xCE => self.rmw_abs(InternalBusRead::DEC),
            //ABS, X
            0xDE => self.rmw_abs_x(InternalBusRead::DEC),
            //----DEX
            0xCA => {
                self.reg.x = self.reg.x.wrapping_sub(1);
                self.reg.z = self.reg.x == 0;
                self.reg.n = self.reg.x & 0x80 != 0;
                self.dummy_read_instr();
            }
            //----DEY
            0x88 => {
                self.reg.y = self.reg.y.wrapping_sub(1);
                self.reg.z = self.reg.y == 0;
                self.reg.n = self.reg.y & 0x80 != 0;
                self.dummy_read_instr();
            }
            //----EOR
            //Im
            0x49 => self.alu_immediate(InternalBusRead::EOR),
            //ZP
            0x45 => self.alu_zp(InternalBusRead::EOR),
            //ZP, X
            0x55 => self.alu_zp_x(InternalBusRead::EOR),
            //Abs
            0x4D => self.alu_abs(InternalBusRead::EOR),
            //Abs, X
            0x5D => self.alu_abs_x(InternalBusRead::EOR),
            //Abs, Y
            0x59 => self.alu_abs_y(InternalBusRead::EOR),
            //Ind, X
            0x41 => self.alu_indr_x(InternalBusRead::EOR),
            //Ind, Y
            0x51 => self.alu_indr_y(InternalBusRead::EOR),
            //----INC
            //ZP
            0xE6 => self.rmw_zp(InternalBusRead::INC),
            //ZP, X
            0xF6 => self.rmw_zp_x(InternalBusRead::INC),
            //ABS
            0xEE => self.rmw_abs(InternalBusRead::INC),
            //ABS, X
            0xFE => self.rmw_abs_x(InternalBusRead::INC),
            //----INX
            0xE8 => {
                self.reg.x = self.reg.x.wrapping_add(1);
                self.reg.z = self.reg.x == 0;
                self.reg.n = self.reg.x & 0x80 != 0;
                self.dummy_read_instr();
            }
            //----INY
            0xC8 => {
                self.reg.y = self.reg.y.wrapping_add(1);
                self.reg.z = self.reg.y == 0;
                self.reg.n = self.reg.y & 0x80 != 0;
                self.dummy_read_instr();
            }
            //----JMP
            //ABS
            0x4C => {
                self.action_queue.push_back(
                    CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusAddr],  }
                );
                self.action_queue.push_back(
                    CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusPage, InternalBusRead::MemBusAddrToPc],  }
                );
            }
            //Ind
            0x6C => {
                //(instr) => bus addr
                self.action_queue.push_back(
                    CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusAddr],  }
                );
                //(instr) => bus page
                self.action_queue.push_back(
                    CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusPage],  }
                );
                //(bus page, bus addr) => PcLo, bus addr++
                self.action_queue.push_back(
                    CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Abs, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::PcLo, InternalBusRead::BusAddrAdd(1)] }
                );
                //(bus page, bus addr) => PcHi
                self.action_queue.push_back(
                    CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Abs, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::PcHi] }
                );
            }
            //----JSR
            0x20 => {
                //(instr) => bus addr
                self.action_queue.push_back(
                    CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusAddr],  }
                );
                //(dummy stack)
                self.dummy_read_stack();
                //PcHi => (sp--)
                self.action_queue.push_back(
                    CpuAction { cycle_type: CycleType::Write, cpu_bus_addr: MemBusAddr::StackPush, internal_bus_put: InternalBusPut::PcHi, internal_bus_read: vec![InternalBusRead::MemBus],  }
                );
                //PcLo => (sp--)
                self.action_queue.push_back(
                    CpuAction { cycle_type: CycleType::Write, cpu_bus_addr: MemBusAddr::StackPush, internal_bus_put: InternalBusPut::PcLo, internal_bus_read: vec![InternalBusRead::MemBus],  }
                );
                //(instr) => Bus page; bus page, addr => PC
                self.action_queue.push_back(
                    CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::BusPage, InternalBusRead::MemBusAddrToPc],  }
                );
                    
            }
            //----LDA
            //Im
            0xA9 => self.alu_immediate(InternalBusRead::LDA),
            //ZP
            0xA5 => self.alu_zp(InternalBusRead::LDA),
            //ZP, X
            0xB5 => self.alu_zp_x(InternalBusRead::LDA),
            //Abs
            0xAD => self.alu_abs(InternalBusRead::LDA),
            //Abs, X
            0xBD => self.alu_abs_x(InternalBusRead::LDA),
            //Abs, Y
            0xB9 => self.alu_abs_y(InternalBusRead::LDA),
            //Ind, X
            0xA1 => self.alu_indr_x(InternalBusRead::LDA),
            //Ind, Y
            0xB1 => self.alu_indr_y(InternalBusRead::LDA),
            //----LDX
            //Im
            0xA2 => self.alu_immediate(InternalBusRead::LDX),
            //ZP
            0xA6 => self.alu_zp(InternalBusRead::LDX),
            //ZP, Y
            0xB6 => self.alu_zp_y(InternalBusRead::LDX),
            //Abs
            0xAE => self.alu_abs(InternalBusRead::LDX),
            //Abs, Y
            0xBE => self.alu_abs_y(InternalBusRead::LDX),
            //----LDY
            //Im
            0xA0 => self.alu_immediate(InternalBusRead::LDY),
            //ZP
            0xA4 => self.alu_zp(InternalBusRead::LDY),
            //ZP, X
            0xB4 => self.alu_zp_x(InternalBusRead::LDY),
            //Abs
            0xAC => self.alu_abs(InternalBusRead::LDY),
            //Abs, X
            0xBC => self.alu_abs_x(InternalBusRead::LDY),
            //-----LSR
            //A
            0x4A => self.alu_single(InternalBusRead::HardwareLSR),
            //ZP
            0x46 => self.rmw_zp(InternalBusRead::LSR),
            //ZP, X
            0x56 => self.rmw_zp_x(InternalBusRead::LSR),
            //ABS
            0x4E => self.rmw_abs(InternalBusRead::LSR),
            //ABS, X
            0x5E => self.rmw_abs_x(InternalBusRead::LSR),
            //----NOP
            0xEA => {
                self.dummy_read_instr();
            }
            //--- ORA
            // Immediate
            0x09 => self.alu_immediate(InternalBusRead::ORA),
            //ZP
            0x05 => self.alu_zp(InternalBusRead::ORA),
            //ZP, X
            0x15 => self.alu_zp_x(InternalBusRead::ORA),
            //ABS
            0x0D => self.alu_abs(InternalBusRead::ORA),
            //ABS, X
            0x1D => self.alu_abs_x(InternalBusRead::ORA),
            //ABS, Y
            0x19 => self.alu_abs_y(InternalBusRead::ORA),
            //(IND, X)
            0x01 => self.alu_indr_x(InternalBusRead::ORA),
            //(IND), Y
            0x11 => self.alu_indr_y(InternalBusRead::ORA),
            //----PHA
            0x48 => {
                self.dummy_read_instr();
                self.action_queue.push_back(
                    CpuAction { cycle_type: CycleType::Write, cpu_bus_addr: MemBusAddr::StackPush, internal_bus_put: InternalBusPut::A, internal_bus_read: vec![InternalBusRead::MemBus, InternalBusRead::UpdateZN],  }
                );
            }
            //---PHP
            0x08 => {
                self.dummy_read_instr();
                self.action_queue.push_back(
                    CpuAction { cycle_type: CycleType::Write, cpu_bus_addr: MemBusAddr::StackPush, internal_bus_put: InternalBusPut::Flags(true, true), internal_bus_read: vec![InternalBusRead::MemBus],  }
                );
            }
            //----PLA
            0x68 => {
                self.dummy_read_instr();
                self.dummy_read_stack();
                self.action_queue.push_back(
                    CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::StackPull, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::A, InternalBusRead::UpdateZN],  }
                );
            }
            //----PLP
            0x28 => {
                self.dummy_read_instr();
                self.dummy_read_stack();
                self.action_queue.push_back(
                    CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::StackPull, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::Flags(true)],  }
                );
            }
            //----ROL
            //A
            0x2A => self.alu_single(InternalBusRead::HardwareROL),
            //ZP
            0x26 => self.rmw_zp(InternalBusRead::ROL),
            //ZP, X
            0x36 => self.rmw_zp_x(InternalBusRead::ROL),
            //ABS
            0x2E => self.rmw_abs(InternalBusRead::ROL),
            //ABS, X
            0x3E => self.rmw_abs_x(InternalBusRead::ROL),
            //----ROR
            //A
            0x6A => self.alu_single(InternalBusRead::HardwareROR),
            //ZP
            0x66 => self.rmw_zp(InternalBusRead::ROR),
            //ZP, X
            0x76 => self.rmw_zp_x(InternalBusRead::ROR),
            //ABS
            0x6E => self.rmw_abs(InternalBusRead::ROR),
            //ABS, X
            0x7E => self.rmw_abs_x(InternalBusRead::ROR),
            //----RTI
            0x40 => {
                self.dump_memory(0x100, 0x100);
                self.dummy_read_instr();
                self.dummy_read_stack();
                //flags = Pull()
                self.action_queue.push_back(
                    CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::StackPull, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::Flags(false)],  }
                );
                //PcLo = Pull()
                self.action_queue.push_back(
                    CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::StackPull, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::PcLo],  }
                );
                //PcHi = Pull()
                self.action_queue.push_back(
                    CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::StackPull, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::PcHi],  }
                );
            }
            //----RTS
            0x60 => {
                self.dummy_read_instr();
                self.dummy_read_stack();
                //PcLo = Pull()
                self.action_queue.push_back(
                    CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::StackPull, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::PcLo],  }
                );
                //PcHi = Pull()
                self.action_queue.push_back(
                    CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::StackPull, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::PcHi],  }
                );
                //(instr++) => _
                self.action_queue.push_back(
                    CpuAction { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![],  }
                );
            }
            //--- SBC
            // Immediate
            0xE9 => self.alu_immediate(InternalBusRead::SBC),
            //ZP
            0xE5 => self.alu_zp(InternalBusRead::SBC),
            //ZP, X
            0xF5 => self.alu_zp_x(InternalBusRead::SBC),
            //ABS
            0xED => self.alu_abs(InternalBusRead::SBC),
            //ABS, X
            0xFD => self.alu_abs_x(InternalBusRead::SBC),
            //ABS, Y
            0xF9 => self.alu_abs_y(InternalBusRead::SBC),
            //(IND, X)
            0xE1 => self.alu_indr_x(InternalBusRead::SBC),
            //(IND), Y
            0xF1 => self.alu_indr_y(InternalBusRead::SBC),
            //----SEC
            0x38 => {
                //println!("sec");
                self.reg.c = true;
                self.dummy_read_instr();
            }
            //----SED
            0xF8 => {
                //println!("sed");
                self.reg.d = true;
                self.dummy_read_instr();
            }
            //----SEI
            0x78 => {
                //println!("sei");
                self.reg.schedule_i = Some(true);
                self.dummy_read_instr();
            }
            //--- STA
            // ZP
            0x85 => self.write_zp(InternalBusPut::A),
            //ZP, X
            0x95 => self.write_zp_x(InternalBusPut::A),
            //ABS
            0x8D => self.write_abs(InternalBusPut::A),
            //ABS, X
            0x9D => self.write_abs_x(InternalBusPut::A),
            //ABS, Y
            0x99 => self.write_abs_y(InternalBusPut::A),
            //(IND, X)
            0x81 => self.write_indr_x(InternalBusPut::A),
            //(IND), Y
            0x91 => self.write_indr_y(InternalBusPut::A),
            //--- STX
            // ZP
            0x86 => self.write_zp(InternalBusPut::X),
            //ZP, Y
            0x96 => self.write_zp_y(InternalBusPut::X),
            //ABS
            0x8E => self.write_abs(InternalBusPut::X),
            //--- STY
            // ZP
            0x84 => self.write_zp(InternalBusPut::Y),
            //ZP, X
            0x94 => self.write_zp_x(InternalBusPut::Y),
            //ABS
            0x8C => self.write_abs(InternalBusPut::Y),
            //----TAX
            0xAA => {
                //println!("TAX");
                self.reg.x = self.reg.a;
                self.reg.z = self.reg.x == 0;
                self.reg.n = self.reg.x & 0x80 != 0;
                self.dummy_read_instr();
            }
            //----TAY
            0xA8 => {
                //println!("TAY");
                self.reg.y = self.reg.a;
                self.reg.z = self.reg.y == 0;
                self.reg.n = self.reg.y & 0x80 != 0;
                self.dummy_read_instr();
            }
            //----TSX
            0xBA => {
                //println!("TSX");
                self.reg.x = self.reg.sp;
                self.reg.z = self.reg.x == 0;
                self.reg.n = self.reg.x & 0x80 != 0;
                self.dummy_read_instr();
            }
            //----TXA
            0x8A => {
                //println!("TXA");
                self.reg.a = self.reg.x;
                self.reg.z = self.reg.a == 0;
                self.reg.n = self.reg.a & 0x80 != 0;
                self.dummy_read_instr();
            }
            //----TXS
            0x9A => {
                //println!("TXS");
                self.reg.sp = self.reg.x;
                self.dummy_read_instr();
            }
            //----TYA
            0x98 => {
                //println!("TYA");
                self.reg.a = self.reg.y;
                self.reg.z = self.reg.a == 0;
                self.reg.n = self.reg.a & 0x80 != 0;
                self.dummy_read_instr();
            }
            _ => {
                println!("Unimplemented instruction {instr} at pc {}", self.reg.pc)
            }
        }
    }

    pub fn mem_mut(&mut self) -> &mut MemoryBus<0x10000> {
        &mut self.mem
    }

    pub fn cycle_count_mut(&mut self) -> &mut usize {
        &mut self.cycle_count
    }
}


#[derive(Debug)]
pub struct CpuAction {
    internal_bus_put: InternalBusPut,
    internal_bus_read: Vec<InternalBusRead>,
    cpu_bus_addr: MemBusAddr,
    cycle_type: CycleType,
}

impl CpuAction {
    pub fn fetch_decode_instruction_action() -> Self {
        Self { cycle_type: CycleType::Read, cpu_bus_addr: MemBusAddr::Instr, internal_bus_put: InternalBusPut::MemBus, internal_bus_read: vec![InternalBusRead::InstrDecode], }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CycleType {
    Read,
    Write,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MemBusAddr {
    Const(u16),
    Instr,
    DummyInstr,
    Abs,
    Zp,
    WordRead,
    StackPull,
    DummyStack,
    StackPush,
    None,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InternalBusPut {
    A,
    X,
    Y,
    Flags(bool, bool),
    PcLo,
    PcHi,
    BusAddr,
    BusPage,
    MemBus,
    Transformed,
    Const(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InternalBusRead {
    UpdateZN,

    InstrDecode,

    BusAddr,
    BusPage,

    A,
    X,
    Y,
    SEI,
    PcLo,
    PcHi,
    MemBusAddrToPc,
    ///Writes to the CPU bus at BusPage BusAddr 
    MemBus,
    WordRead,
    //bus addr + internal bus => word read
    WordReadAdd,
    //bus page + $1 => bus page
    BusPageAdd(u8),
    ///bus addr + $1 => bus addr
    BusAddrAdd(u8),
    /// bus addr + $1 => bus addr
    /// On overflow adds cpu action: (bus page, bus addr), 1 => BusPageAdd
    BusAddrAddCarry(u8, bool),
    Flags(bool),
    //internal bus as i8 + sp => sp (extra cycle on page crossed)
    BranchRelative,
    CorrectPcPage,
    Transform,
    ReadIV,
    //ALU Instructions  
    ADC,
    AND,
    HardwareASL,
    ASL,
    BIT,
    CMP,
    CPX,
    CPY,
    DEC,
    EOR,
    INC,
    LDA,
    LDX,
    LDY,
    HardwareLSR,
    LSR,
    ORA,
    HardwareROL,
    ROL,
    HardwareROR,
    ROR,
    SBC,
}