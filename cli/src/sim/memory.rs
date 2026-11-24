use crate::elf::LoadedElf;
use crate::sim::peripherals::Peripherals;
use std::collections::HashSet;

pub const FLASH_BASE: u32 = 0x0800_0000;
pub const FLASH_SIZE: usize = 512 * 1024;
pub const SRAM_BASE: u32 = 0x2000_0000;
pub const SRAM_SIZE: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct Memory {
    pub flash: Vec<u8>,
    pub sram: Vec<u8>,
    pub peripherals: Peripherals,
    pub dirty_sram: HashSet<u32>,
}

impl Default for Memory {
    fn default() -> Self {
        Self {
            flash: vec![0xFF; FLASH_SIZE],
            sram: vec![0x00; SRAM_SIZE],
            peripherals: Peripherals::default(),
            dirty_sram: HashSet::new(),
        }
    }
}

impl Memory {
    pub fn reset_and_load(&mut self, elf: &LoadedElf) {
        self.flash.fill(0xFF);
        self.sram.fill(0x00);
        self.peripherals = Peripherals::default();
        self.dirty_sram.clear();

        for seg in &elf.segments {
            for (i, &b) in seg.data.iter().enumerate() {
                let addr = seg.vaddr.wrapping_add(i as u32);
                self.load_byte(addr, b);
            }
        }
    }

    fn load_byte(&mut self, addr: u32, b: u8) {
        if (FLASH_BASE..FLASH_BASE + FLASH_SIZE as u32).contains(&addr) {
            self.flash[(addr - FLASH_BASE) as usize] = b;
        } else if addr < FLASH_SIZE as u32 {
            self.flash[addr as usize] = b;
        } else if (SRAM_BASE..SRAM_BASE + SRAM_SIZE as u32).contains(&addr) {
            self.sram[(addr - SRAM_BASE) as usize] = b;
        }
    }

    pub fn read8(&mut self, addr: u32) -> u8 {
        if (FLASH_BASE..FLASH_BASE + FLASH_SIZE as u32).contains(&addr) {
            self.flash[(addr - FLASH_BASE) as usize]
        } else if addr < FLASH_SIZE as u32 {
            self.flash[addr as usize]
        } else if (SRAM_BASE..SRAM_BASE + SRAM_SIZE as u32).contains(&addr) {
            self.sram[(addr - SRAM_BASE) as usize]
        } else if is_mmio(addr) {
            let aligned = addr & !3;
            let shift = (addr & 3) * 8;
            ((self.peripherals.read32(aligned) >> shift) & 0xFF) as u8
        } else {
            0
        }
    }

    pub fn read16(&mut self, addr: u32) -> u16 {
        let b0 = self.read8(addr) as u16;
        let b1 = self.read8(addr.wrapping_add(1)) as u16;
        b0 | (b1 << 8)
    }

    pub fn read32(&mut self, addr: u32) -> u32 {
        if is_mmio(addr) {
            return self.peripherals.read32(addr & !3);
        }
        let b0 = self.read8(addr) as u32;
        let b1 = self.read8(addr.wrapping_add(1)) as u32;
        let b2 = self.read8(addr.wrapping_add(2)) as u32;
        let b3 = self.read8(addr.wrapping_add(3)) as u32;
        b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
    }

    pub fn write8(&mut self, addr: u32, val: u8) {
        if (SRAM_BASE..SRAM_BASE + SRAM_SIZE as u32).contains(&addr) {
            self.sram[(addr - SRAM_BASE) as usize] = val;
            self.dirty_sram.insert(addr);
        } else if is_mmio(addr) {
            let aligned = addr & !3;
            let shift = (addr & 3) * 8;
            let cur = self.peripherals.read32(aligned);
            let next = (cur & !(0xFF << shift)) | ((val as u32) << shift);
            self.peripherals.write32(aligned, next);
        }
    }

    pub fn write16(&mut self, addr: u32, val: u16) {
        if is_mmio(addr) {
            let aligned = addr & !3;
            let shift = (addr & 2) * 8;
            let cur = self.peripherals.read32(aligned);
            let next = (cur & !(0xFFFF << shift)) | ((val as u32) << shift);
            self.peripherals.write32(aligned, next);
            return;
        }
        self.write8(addr, (val & 0xFF) as u8);
        self.write8(addr.wrapping_add(1), ((val >> 8) & 0xFF) as u8);
    }

    pub fn write32(&mut self, addr: u32, val: u32) {
        if is_mmio(addr) {
            self.peripherals.write32(addr & !3, val);
            return;
        }
        self.write8(addr, (val & 0xFF) as u8);
        self.write8(addr.wrapping_add(1), ((val >> 8) & 0xFF) as u8);
        self.write8(addr.wrapping_add(2), ((val >> 16) & 0xFF) as u8);
        self.write8(addr.wrapping_add(3), ((val >> 24) & 0xFF) as u8);
    }

    pub fn read_slice(&mut self, addr: u32, len: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(len);
        for i in 0..len {
            out.push(self.read8(addr.wrapping_add(i as u32)));
        }
        out
    }

    pub fn read_sram_word_raw(&self, addr: u32) -> Option<u32> {
        if (SRAM_BASE..SRAM_BASE + SRAM_SIZE as u32 - 3).contains(&addr) {
            let o = (addr - SRAM_BASE) as usize;
            Some(u32::from_le_bytes([
                self.sram[o],
                self.sram[o + 1],
                self.sram[o + 2],
                self.sram[o + 3],
            ]))
        } else {
            None
        }
    }
}

fn is_mmio(addr: u32) -> bool {
    (0x4000_0000..=0x4003_FFFF).contains(&addr) || (0xE000_0000..=0xE00F_FFFF).contains(&addr)
}
