/// Struct containing everything
use super::arm7tdmi::Core;
use super::cartridge::Cartridge;
use super::gpu::*;
use super::interrupt::*;
use super::iodev::*;
use super::sysbus::SysBus;

use super::SyncedIoDevice;
use crate::backend::*;

pub struct GameBoyAdvance {
    backend: Box<dyn EmulatorBackend>,
    pub cpu: Core,
    pub sysbus: Box<SysBus>,
}

impl GameBoyAdvance {
    pub fn new(
        cpu: Core,
        bios_rom: Vec<u8>,
        gamepak: Cartridge,
        backend: Box<dyn EmulatorBackend>,
    ) -> GameBoyAdvance {
        let io = IoDevices::new();
        GameBoyAdvance {
            backend: backend,
            cpu: cpu,
            sysbus: Box::new(SysBus::new(io, bios_rom, gamepak)),
        }
    }

    pub fn frame(&mut self) {
        self.update_key_state();
        while self.sysbus.io.gpu.state != GpuState::VBlank {
            self.step_new();
        }
        while self.sysbus.io.gpu.state == GpuState::VBlank {
            self.step_new();
        }
    }

    pub fn update_key_state(&mut self) {
        self.sysbus.io.keyinput = self.backend.get_key_state();
    }

    pub fn add_breakpoint(&mut self, addr: u32) -> Option<usize> {
        if !self.cpu.breakpoints.contains(&addr) {
            let new_index = self.cpu.breakpoints.len();
            self.cpu.breakpoints.push(addr);
            Some(new_index)
        } else {
            None
        }
    }

    pub fn check_breakpoint(&self) -> Option<u32> {
        let next_pc = self.cpu.get_next_pc();
        for bp in &self.cpu.breakpoints {
            if *bp == next_pc {
                return Some(next_pc);
            }
        }

        None
    }

    pub fn step_new(&mut self) {
        let mut irqs = IrqBitmask(0);
        let previous_cycles = self.cpu.cycles;

        // // I hate myself for doing this, but rust left me no choice.
        let io = unsafe {
            let ptr = &mut *self.sysbus as *mut SysBus;
            &mut (*ptr).io as &mut IoDevices
        };

        let cycles = if !io.dmac.perform_work(&mut self.sysbus, &mut irqs) {
            if io.intc.irq_pending() {
                self.cpu.irq(&mut self.sysbus);
                io.haltcnt = HaltState::Running;
            }

            if HaltState::Running == io.haltcnt {
                self.cpu.step(&mut self.sysbus).unwrap();
                self.cpu.cycles - previous_cycles
            } else {
                1
            }
        } else {
            0
        };

        io.timers.step(cycles, &mut self.sysbus, &mut irqs);
        if let Some(new_gpu_state) = io.gpu.step(cycles, &mut self.sysbus, &mut irqs) {
            match new_gpu_state {
                GpuState::VBlank => {
                    self.backend.render(io.gpu.get_framebuffer());
                    io.dmac.notify_vblank();
                }
                GpuState::HBlank => io.dmac.notify_hblank(),
                _ => {}
            }
        }

        io.intc.request_irqs(irqs);
    }
}
