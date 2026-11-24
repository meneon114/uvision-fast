//! STM32F10x system memory map (RM0008 3.3): flash with its boot alias at 0, SRAM, the
//! SRAM and peripheral bit-band regions, the APB/AHB peripherals and the Cortex-M3 private
//! peripheral bus. CPU accesses go through [`Memory::read`] / [`Memory::write`] /
//! [`Memory::fetch16`], which report bus errors and execute-never violations so the core can
//! raise precise BusFault / MemManage exceptions. The `read8`.. / `write8`.. helpers are the
//! debugger's view: they never fault and reads have no side effects.

use crate::elf::LoadedElf;
use crate::sim::peripherals::Peripherals;
use std::collections::HashSet;

pub const FLASH_BASE: u32 = 0x0800_0000;
pub const SRAM_BASE: u32 = 0x2000_0000;
pub const SRAM_BB_BASE: u32 = 0x2200_0000;
pub const PERIPH_BASE: u32 = 0x4000_0000;
pub const PERIPH_BB_BASE: u32 = 0x4200_0000;
pub const SYSMEM_BASE: u32 = 0x1FFF_F000;

/// Flash / SRAM sizes of an STM32F10x part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceMap {
    pub name: String,
    pub flash_size: u32,
    pub sram_size: u32,
}

impl DeviceMap {
    /// Derive the memory sizes from an ST part number such as `STM32F103C8` or `STM32F100RB`.
    /// Unknown names fall back to STM32F103C8 (Blue Pill).
    pub fn for_device(device: &str) -> DeviceMap {
        let d = device.trim().to_ascii_uppercase();
        let fallback = DeviceMap { name: "STM32F103C8".into(), flash_size: 64 * 1024, sram_size: 20 * 1024 };
        let Some(rest) = d.strip_prefix("STM32F10") else {
            return fallback;
        };
        let mut chars = rest.chars();
        let line = chars.next().unwrap_or('3');
        let _pins = chars.next();
        let size = chars.next().unwrap_or('8');
        let flash_k = match size {
            '4' => 16,
            '6' => 32,
            '8' => 64,
            'B' => 128,
            'C' => 256,
            'D' => 384,
            'E' => 512,
            'F' => 768,
            'G' => 1024,
            _ => return fallback,
        };
        let sram_k = match (line, size) {
            ('0', '4') | ('0', '6') => 4,
            ('0', '8') | ('0', 'B') => 8,
            ('0', 'C') => 24,
            ('0', _) => 32,
            ('1', '4') => 4,
            ('1', '6') => 6,
            ('1', '8') => 10,
            ('1', 'B') => 16,
            ('1', 'C') => 32,
            ('1', 'D') | ('1', 'E') => 48,
            ('1', _) => 80,
            ('2', '4') => 4,
            ('2', '6') => 6,
            ('2', '8') => 10,
            ('2', _) => 16,
            ('3', '4') => 6,
            ('3', '6') => 10,
            ('3', '8') | ('3', 'B') => 20,
            ('3', 'C') => 48,
            ('3', 'D') | ('3', 'E') => 64,
            ('3', _) => 96,
            ('5', _) | ('7', _) => 64,
            _ => return fallback,
        };
        DeviceMap { name: d.chars().take(11).collect(), flash_size: flash_k * 1024, sram_size: sram_k * 1024 }
    }
}

impl Default for DeviceMap {
    fn default() -> Self {
        DeviceMap::for_device("STM32F103C8")
    }
}

/// Why a CPU access failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemError {
    /// BusFault (unmapped address, write to flash, unprivileged PPB access, unaligned device access).
    Bus,
    /// MemManage: instruction fetch from an execute-never region of the default memory map.
    Xn,
}

#[derive(Debug, Clone)]
pub struct Memory {
    pub flash: Vec<u8>,
    pub sram: Vec<u8>,
    pub peripherals: Peripherals,
    pub dirty_sram: HashSet<u32>,
    pub map: DeviceMap,
}

impl Default for Memory {
    fn default() -> Self {
        Self::new(DeviceMap::default(), 8_000_000)
    }
}

impl Memory {
    pub fn new(map: DeviceMap, hse_hz: u32) -> Self {
        Self {
            flash: vec![0xFF; map.flash_size as usize],
            sram: vec![0x00; map.sram_size as usize],
            peripherals: Peripherals::new(hse_hz),
            dirty_sram: HashSet::new(),
            map,
        }
    }

    pub fn reset_and_load(&mut self, elf: &LoadedElf) {
        self.flash.fill(0xFF);
        self.sram.fill(0x00);
        let hse = self.peripherals.rcc.hse_hz;
        self.peripherals = Peripherals::new(hse);
        self.dirty_sram.clear();
        for seg in &elf.segments {
            for (i, &b) in seg.data.iter().enumerate() {
                self.load_byte(seg.paddr.wrapping_add(i as u32), b);
            }
        }
    }

    /// Peripheral-only reset (AIRCR.SYSRESETREQ): memory contents are kept.
    pub fn system_reset(&mut self) {
        let hse = self.peripherals.rcc.hse_hz;
        let gpio_ext: Vec<(u16, u16)> = self.peripherals.gpios.iter().map(|g| (g.ext_pins, g.ext_mask)).collect();
        let tx: Vec<Vec<u8>> = self.peripherals.usarts.iter().map(|u| u.tx_buffer.clone()).collect();
        self.peripherals = Peripherals::new(hse);
        for (g, (p, m)) in self.peripherals.gpios.iter_mut().zip(gpio_ext) {
            g.ext_pins = p;
            g.ext_mask = m;
            g.refresh_idr();
        }
        for (u, t) in self.peripherals.usarts.iter_mut().zip(tx) {
            u.tx_buffer = t;
        }
    }

    fn flash_off(&self, addr: u32) -> Option<usize> {
        let size = self.map.flash_size;
        if (FLASH_BASE..FLASH_BASE.wrapping_add(size)).contains(&addr) {
            Some((addr - FLASH_BASE) as usize)
        } else if addr < size {
            Some(addr as usize) // boot alias (BOOT0 = 0)
        } else {
            None
        }
    }

    fn sram_off(&self, addr: u32) -> Option<usize> {
        if (SRAM_BASE..SRAM_BASE.wrapping_add(self.map.sram_size)).contains(&addr) {
            Some((addr - SRAM_BASE) as usize)
        } else {
            None
        }
    }

    fn load_byte(&mut self, addr: u32, b: u8) {
        if let Some(o) = self.flash_off(addr) {
            self.flash[o] = b;
        } else if let Some(o) = self.sram_off(addr) {
            self.sram[o] = b;
        }
    }

    fn is_mmio(addr: u32) -> bool {
        (PERIPH_BASE..0x4002_4000).contains(&addr) || (0xE000_0000..=0xE00F_FFFF).contains(&addr)
    }

    fn sysmem_read8(&self, addr: u32) -> u8 {
        let fsize_kb = (self.map.flash_size / 1024) as u16;
        match addr {
            0x1FFF_F7E0 => fsize_kb as u8,
            0x1FFF_F7E1 => (fsize_kb >> 8) as u8,
            0x1FFF_F7E8..=0x1FFF_F7F3 => (addr as u8).wrapping_mul(37) ^ 0x5A, // unique ID
            _ => 0xFF,
        }
    }

    /// Byte read from plain memory (no peripherals). `None` = not plain memory.
    fn plain_read8(&self, addr: u32) -> Option<u8> {
        if let Some(o) = self.flash_off(addr) {
            Some(self.flash[o])
        } else if let Some(o) = self.sram_off(addr) {
            Some(self.sram[o])
        } else if (SYSMEM_BASE..0x1FFF_F810).contains(&addr) {
            Some(self.sysmem_read8(addr))
        } else {
            None
        }
    }

    fn bitband_target(&self, addr: u32) -> Option<(u32, u32)> {
        let sram_bb_end = SRAM_BB_BASE + self.map.sram_size * 32;
        let per_bb_end = PERIPH_BB_BASE + 0x24000 * 32;
        let (base, target) = if (SRAM_BB_BASE..sram_bb_end).contains(&addr) {
            (SRAM_BB_BASE, SRAM_BASE)
        } else if (PERIPH_BB_BASE..per_bb_end).contains(&addr) {
            (PERIPH_BB_BASE, PERIPH_BASE)
        } else {
            return None;
        };
        let bit_index = (addr - base) / 4;
        Some((target + bit_index / 8, bit_index % 8))
    }

    fn mmio_read(&mut self, addr: u32, size: u32, side_effects: bool) -> Result<u32, MemError> {
        if addr & (size - 1) != 0 {
            return Err(MemError::Bus);
        }
        let word = self.peripherals.read(addr & !3, side_effects).ok_or(MemError::Bus)?;
        let shift = (addr & 3) * 8;
        Ok(match size {
            1 => (word >> shift) & 0xFF,
            2 => (word >> shift) & 0xFFFF,
            _ => word,
        })
    }

    fn mmio_write(&mut self, addr: u32, size: u32, val: u32) -> Result<(), MemError> {
        if addr & (size - 1) != 0 {
            return Err(MemError::Bus);
        }
        let shift = (addr & 3) * 8;
        let (data, mask) = match size {
            1 => {
                let b = val & 0xFF;
                if Peripherals::is_apb(addr) {
                    (b * 0x0101_0101, 0xFFFF_FFFF)
                } else {
                    (b << shift, 0xFF << shift)
                }
            }
            2 => {
                let h = val & 0xFFFF;
                if Peripherals::is_apb(addr) {
                    (h | (h << 16), 0xFFFF_FFFF)
                } else {
                    (h << shift, 0xFFFF << shift)
                }
            }
            _ => (val, 0xFFFF_FFFF),
        };
        if self.peripherals.write(addr & !3, data, mask) {
            Ok(())
        } else {
            Err(MemError::Bus)
        }
    }

    /// CPU data read of `size` (1, 2, 4) bytes.
    pub fn read(&mut self, addr: u32, size: u32, privileged: bool) -> Result<u32, MemError> {
        if (0xE000_0000..=0xE00F_FFFF).contains(&addr) && !privileged {
            return Err(MemError::Bus);
        }
        if Self::is_mmio(addr) {
            return self.mmio_read(addr, size, true);
        }
        if let Some((target, bit)) = self.bitband_target(addr) {
            let byte = if target >= PERIPH_BASE {
                (self.mmio_read(target & !3, 4, true)? >> ((target & 3) * 8)) & 0xFF
            } else {
                self.plain_read8(target).ok_or(MemError::Bus)? as u32
            };
            return Ok((byte >> bit) & 1);
        }
        let mut v = 0u32;
        for i in 0..size {
            let b = self.plain_read8(addr.wrapping_add(i)).ok_or(MemError::Bus)?;
            v |= (b as u32) << (8 * i);
        }
        Ok(v)
    }

    /// CPU data write of `size` bytes.
    pub fn write(&mut self, addr: u32, size: u32, val: u32, privileged: bool) -> Result<(), MemError> {
        if (0xE000_0000..=0xE00F_FFFF).contains(&addr) && !privileged {
            // STIR is writable from thread mode only when CCR.USERSETMPEND is set
            let stir_ok = addr == 0xE000_EF00 && self.peripherals.nvic.ccr & 2 != 0;
            if !stir_ok {
                return Err(MemError::Bus);
            }
        }
        if Self::is_mmio(addr) {
            return self.mmio_write(addr, size, val);
        }
        if let Some((target, bit)) = self.bitband_target(addr) {
            let on = val & 1 != 0;
            if target >= PERIPH_BASE {
                let word_addr = target & !3;
                let shift = (target & 3) * 8 + bit;
                let w = self.mmio_read(word_addr, 4, true)?;
                let nw = if on { w | (1 << shift) } else { w & !(1 << shift) };
                return self.mmio_write(word_addr, 4, nw);
            }
            let o = self.sram_off(target).ok_or(MemError::Bus)?;
            if on {
                self.sram[o] |= 1 << bit;
            } else {
                self.sram[o] &= !(1 << bit);
            }
            self.dirty_sram.insert(target);
            return Ok(());
        }
        // All bytes must be writable SRAM (flash is read-only without the FPEC sequence)
        for i in 0..size {
            if self.sram_off(addr.wrapping_add(i)).is_none() {
                return Err(MemError::Bus);
            }
        }
        for i in 0..size {
            let a = addr.wrapping_add(i);
            let o = (a - SRAM_BASE) as usize;
            self.sram[o] = (val >> (8 * i)) as u8;
            self.dirty_sram.insert(a);
        }
        Ok(())
    }

    /// Instruction fetch (halfword).
    pub fn fetch16(&mut self, addr: u32) -> Result<u16, MemError> {
        if addr >= 0x4000_0000 && !(0x6000_0000..0xA000_0000).contains(&addr) {
            return Err(MemError::Xn);
        }
        let b0 = self.plain_read8(addr).ok_or(MemError::Bus)?;
        let b1 = self.plain_read8(addr.wrapping_add(1)).ok_or(MemError::Bus)?;
        Ok(b0 as u16 | ((b1 as u16) << 8))
    }

    /// Is `addr` in flash (for wait-state timing)?
    pub fn is_flash(&self, addr: u32) -> bool {
        self.flash_off(addr).is_some()
    }

    // ---- debugger view (never faults, reads have no side effects) --------------------------

    pub fn read8(&mut self, addr: u32) -> u8 {
        if Self::is_mmio(addr) {
            return self.mmio_read(addr, 1, false).unwrap_or(0) as u8;
        }
        if let Some((target, bit)) = self.bitband_target(addr) {
            let byte = if target >= PERIPH_BASE {
                self.mmio_read(target, 1, false).unwrap_or(0) as u8
            } else {
                self.plain_read8(target).unwrap_or(0)
            };
            return (byte >> bit) & 1;
        }
        self.plain_read8(addr).unwrap_or(0)
    }

    pub fn read16(&mut self, addr: u32) -> u16 {
        if Self::is_mmio(addr) && addr & 1 == 0 {
            return self.mmio_read(addr, 2, false).unwrap_or(0) as u16;
        }
        self.read8(addr) as u16 | ((self.read8(addr.wrapping_add(1)) as u16) << 8)
    }

    pub fn read32(&mut self, addr: u32) -> u32 {
        if Self::is_mmio(addr) && addr & 3 == 0 {
            return self.mmio_read(addr, 4, false).unwrap_or(0);
        }
        (0..4).fold(0u32, |v, i| v | ((self.read8(addr.wrapping_add(i)) as u32) << (8 * i)))
    }

    pub fn write8(&mut self, addr: u32, val: u8) {
        if let Some(o) = self.flash_off(addr) {
            self.flash[o] = val;
            return;
        }
        let _ = self.write(addr, 1, val as u32, true);
    }

    pub fn write16(&mut self, addr: u32, val: u16) {
        if Self::is_mmio(addr) && addr & 1 == 0 {
            let _ = self.mmio_write(addr, 2, val as u32);
            return;
        }
        self.write8(addr, val as u8);
        self.write8(addr.wrapping_add(1), (val >> 8) as u8);
    }

    pub fn write32(&mut self, addr: u32, val: u32) {
        if Self::is_mmio(addr) && addr & 3 == 0 {
            let _ = self.mmio_write(addr, 4, val);
            return;
        }
        for i in 0..4 {
            self.write8(addr.wrapping_add(i), (val >> (8 * i)) as u8);
        }
    }

    pub fn read_slice(&mut self, addr: u32, len: usize) -> Vec<u8> {
        (0..len).map(|i| self.read8(addr.wrapping_add(i as u32))).collect()
    }

    pub fn read_sram_word_raw(&self, addr: u32) -> Option<u32> {
        let o = self.sram_off(addr)?;
        if o + 4 > self.sram.len() {
            return None;
        }
        Some(u32::from_le_bytes([self.sram[o], self.sram[o + 1], self.sram[o + 2], self.sram[o + 3]]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_sizes() {
        let m = DeviceMap::for_device("STM32F103C8");
        assert_eq!((m.flash_size, m.sram_size), (64 * 1024, 20 * 1024));
        let m = DeviceMap::for_device("STM32F100RB");
        assert_eq!((m.flash_size, m.sram_size), (128 * 1024, 8 * 1024));
        let m = DeviceMap::for_device("STM32F103RE");
        assert_eq!((m.flash_size, m.sram_size), (512 * 1024, 64 * 1024));
    }

    #[test]
    fn apb_byte_writes_replicate_across_lanes() {
        let mut m = Memory::default();
        m.write32(0x4002_1018, 1 << 2); // IOPAEN
        // STRB to GPIOA_ODR+1 on APB2 is seen as a 32-bit write of 0xABABABAB
        m.write(0x4001_080D, 1, 0xAB, true).unwrap();
        assert_eq!(m.read32(0x4001_080C), 0xABAB);
    }

    #[test]
    fn ppb_byte_writes_are_lane_masked() {
        let mut m = Memory::default();
        m.write(0xE000_E401, 1, 0x50, true).unwrap(); // IPR byte for IRQ1
        assert_eq!(m.read32(0xE000_E400), 0x5000);
        assert_eq!(m.peripherals.nvic.ipr[1], 0x50);
        assert_eq!(m.peripherals.nvic.ipr[0], 0);
    }

    #[test]
    fn bitband_sram_and_peripheral() {
        let mut m = Memory::default();
        m.write(0x2200_0000 + 4 * 8 + 4 * 3, 4, 1, true).unwrap(); // SRAM byte 1, bit 3
        assert_eq!(m.sram[1], 0x08);
        assert_eq!(m.read(0x2200_0000 + 4 * 11, 4, true).unwrap(), 1);
        // RCC_APB2ENR bit 4 (IOPCEN) through the peripheral alias
        let alias = PERIPH_BB_BASE + (0x2_1018 * 32) + 4 * 4;
        m.write(alias, 4, 1, true).unwrap();
        assert_eq!(m.read32(0x4002_1018), 1 << 4);
    }

    #[test]
    fn faults() {
        let mut m = Memory::default();
        assert_eq!(m.write(0x0800_0000, 4, 0, true), Err(MemError::Bus));
        assert_eq!(m.read(0x3000_0000, 4, true), Err(MemError::Bus));
        assert_eq!(m.read(0x2000_5000, 4, true), Err(MemError::Bus)); // beyond 20K
        assert_eq!(m.fetch16(0x4001_0800), Err(MemError::Xn));
        assert_eq!(m.read(0xE000_ED00, 4, false), Err(MemError::Bus));
        assert_eq!(m.read(0xE000_ED00, 4, true), Ok(0x411F_C231));
    }
}
