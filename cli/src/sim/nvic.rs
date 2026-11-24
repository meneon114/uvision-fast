//! Cortex-M3 System Control Block, NVIC and DWT cycle counter (ARMv7-M B3.2 / B3.4).
//!
//! Pending and active state are kept per exception number so the CPU can do proper
//! priority arbitration: preemption by group priority, tail-chaining on return, PRIMASK /
//! FAULTMASK / BASEPRI masking and level-sensitive interrupt lines that re-pend while the
//! peripheral keeps them asserted.

use serde::{Deserialize, Serialize};

pub const EXC_RESET: usize = 1;
pub const EXC_NMI: usize = 2;
pub const EXC_HARDFAULT: usize = 3;
pub const EXC_MEMMANAGE: usize = 4;
pub const EXC_BUSFAULT: usize = 5;
pub const EXC_USAGEFAULT: usize = 6;
pub const EXC_SVCALL: usize = 11;
pub const EXC_DEBUGMON: usize = 12;
pub const EXC_PENDSV: usize = 14;
pub const EXC_SYSTICK: usize = 15;

/// Number of external interrupt inputs implemented (STM32F10x connectivity line: 68 -> 96 slots).
pub const NUM_IRQ: usize = 96;
pub const NUM_EXC: usize = 16 + NUM_IRQ;

/// STM32F1 implements the top 4 priority bits.
pub const PRIO_MASK: u8 = 0xF0;

// CFSR bits
pub const CFSR_IACCVIOL: u32 = 1 << 0;
pub const CFSR_DACCVIOL: u32 = 1 << 1;
pub const CFSR_MUNSTKERR: u32 = 1 << 3;
pub const CFSR_MSTKERR: u32 = 1 << 4;
pub const CFSR_MMARVALID: u32 = 1 << 7;
pub const CFSR_IBUSERR: u32 = 1 << 8;
pub const CFSR_PRECISERR: u32 = 1 << 9;
pub const CFSR_UNSTKERR: u32 = 1 << 11;
pub const CFSR_STKERR: u32 = 1 << 12;
pub const CFSR_BFARVALID: u32 = 1 << 15;
pub const CFSR_UNDEFINSTR: u32 = 1 << 16;
pub const CFSR_INVSTATE: u32 = 1 << 17;
pub const CFSR_INVPC: u32 = 1 << 18;
pub const CFSR_NOCP: u32 = 1 << 19;
pub const CFSR_UNALIGNED: u32 = 1 << 24;
pub const CFSR_DIVBYZERO: u32 = 1 << 25;

pub const HFSR_VECTTBL: u32 = 1 << 1;
pub const HFSR_FORCED: u32 = 1 << 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Nvic {
    pub enabled: Vec<bool>,
    pub pending: Vec<bool>,
    pub active: Vec<bool>,
    /// Current level of each external interrupt line as driven by the peripherals.
    #[serde(skip)]
    pub line: Vec<bool>,
    /// IPR bytes for IRQ 0..NUM_IRQ.
    pub ipr: Vec<u8>,
    /// SHPR bytes for exceptions 4..15 (index = exception number - 4).
    pub shpr: [u8; 12],
    pub vtor: u32,
    pub prigroup: u8,
    pub scr: u32,
    pub ccr: u32,
    pub shcsr_ena: u32,
    pub cfsr: u32,
    pub hfsr: u32,
    pub mmfar: u32,
    pub bfar: u32,
    pub dfsr: u32,
    /// AIRCR.SYSRESETREQ written: the simulator performs a system reset.
    #[serde(skip)]
    pub reset_requested: bool,
    pub demcr: u32,
    pub dwt_ctrl: u32,
    pub dwt_cyccnt: u32,
}

impl Default for Nvic {
    fn default() -> Self {
        Self {
            enabled: vec![false; NUM_EXC],
            pending: vec![false; NUM_EXC],
            active: vec![false; NUM_EXC],
            line: vec![false; NUM_IRQ],
            ipr: vec![0; NUM_IRQ],
            shpr: [0; 12],
            vtor: 0,
            prigroup: 0,
            scr: 0,
            // PM0056: CCR reset value 0x0000_0200 (STKALIGN = 1)
            ccr: 0x0000_0200,
            shcsr_ena: 0,
            cfsr: 0,
            hfsr: 0,
            mmfar: 0,
            bfar: 0,
            dfsr: 0,
            reset_requested: false,
            demcr: 0,
            dwt_ctrl: 0x4000_0000,
            dwt_cyccnt: 0,
        }
    }
}

impl Nvic {
    /// Raw (un-grouped) priority of an exception; fixed priorities are negative.
    pub fn priority(&self, exc: usize) -> i32 {
        match exc {
            EXC_RESET => -3,
            EXC_NMI => -2,
            EXC_HARDFAULT => -1,
            4..=15 => self.shpr[exc - 4] as i32,
            _ if (16..NUM_EXC).contains(&exc) => self.ipr[exc - 16] as i32,
            _ => 256,
        }
    }

    /// Group priority (subpriority bits masked off per AIRCR.PRIGROUP).
    pub fn group_priority(&self, exc: usize) -> i32 {
        let p = self.priority(exc);
        if p < 0 {
            return p;
        }
        let sub_bits = self.prigroup as u32 + 1;
        let mask = if sub_bits >= 8 { 0 } else { 0xFFu32 << sub_bits };
        (p as u32 & mask) as i32
    }

    /// Priority boost from active exceptions only.
    pub fn active_priority(&self) -> i32 {
        let mut best = 256;
        for (exc, &a) in self.active.iter().enumerate() {
            if a {
                best = best.min(self.group_priority(exc));
            }
        }
        best
    }

    pub fn active_count(&self) -> usize {
        self.active.iter().filter(|&&a| a).count()
    }

    pub fn is_enabled_exc(&self, exc: usize) -> bool {
        match exc {
            EXC_MEMMANAGE => self.shcsr_ena & (1 << 16) != 0,
            EXC_BUSFAULT => self.shcsr_ena & (1 << 17) != 0,
            EXC_USAGEFAULT => self.shcsr_ena & (1 << 18) != 0,
            0..=15 => true,
            _ => self.enabled.get(exc).copied().unwrap_or(false),
        }
    }

    /// Highest-priority pending exception that is enabled: (exception number, group priority).
    /// Ties are broken by subpriority, then by the lower exception number.
    pub fn highest_pending(&self) -> Option<(usize, i32)> {
        let mut best: Option<(usize, i32, i32)> = None;
        for exc in 1..NUM_EXC {
            if !self.pending[exc] || !self.is_enabled_exc(exc) {
                continue;
            }
            let g = self.group_priority(exc);
            let p = self.priority(exc);
            match best {
                Some((_, bg, bp)) if (g, p) >= (bg, bp) => {}
                _ => best = Some((exc, g, p)),
            }
        }
        best.map(|(e, g, _)| (e, g))
    }

    pub fn irq_pending(&self, irq: usize) -> bool {
        self.pending.get(16 + irq).copied().unwrap_or(false)
    }

    pub fn set_pending(&mut self, exc: usize) {
        if exc < NUM_EXC {
            self.pending[exc] = true;
        }
    }

    /// Update the level of an external interrupt line. A line that is asserted while the
    /// interrupt is neither pending nor active makes it pending; a rising edge always does.
    pub fn set_line(&mut self, irq: usize, level: bool) {
        if irq >= NUM_IRQ {
            return;
        }
        let exc = 16 + irq;
        let rising = level && !self.line[irq];
        self.line[irq] = level;
        if level && (rising || !self.active[exc]) {
            self.pending[exc] = true;
        }
    }

    /// Re-sample all asserted lines (called after an exception returns: a level-sensitive
    /// interrupt that is still asserted becomes pending again).
    pub fn resample_lines(&mut self) {
        for irq in 0..NUM_IRQ {
            if self.line[irq] && !self.active[16 + irq] {
                self.pending[16 + irq] = true;
            }
        }
    }

    pub fn any_irq_enabled(&self) -> bool {
        self.enabled[16..].iter().any(|&e| e)
    }

    fn vectactive(&self, ipsr: u32) -> u32 {
        ipsr & 0x1FF
    }

    fn shcsr(&self) -> u32 {
        let mut v = self.shcsr_ena;
        let bitmap: [(usize, u32, Option<u32>); 7] = [
            (EXC_MEMMANAGE, 0, Some(13)),
            (EXC_BUSFAULT, 1, Some(14)),
            (EXC_USAGEFAULT, 3, Some(12)),
            (EXC_SVCALL, 7, Some(15)),
            (EXC_DEBUGMON, 8, None),
            (EXC_PENDSV, 10, None),
            (EXC_SYSTICK, 11, None),
        ];
        for (exc, act_bit, pend_bit) in bitmap {
            if self.active[exc] {
                v |= 1 << act_bit;
            }
            if let Some(pb) = pend_bit {
                if self.pending[exc] {
                    v |= 1 << pb;
                }
            }
        }
        v
    }

    fn write_shcsr(&mut self, v: u32) {
        self.shcsr_ena = v & (0x7 << 16);
        let bitmap: [(usize, u32, Option<u32>); 7] = [
            (EXC_MEMMANAGE, 0, Some(13)),
            (EXC_BUSFAULT, 1, Some(14)),
            (EXC_USAGEFAULT, 3, Some(12)),
            (EXC_SVCALL, 7, Some(15)),
            (EXC_DEBUGMON, 8, None),
            (EXC_PENDSV, 10, None),
            (EXC_SYSTICK, 11, None),
        ];
        for (exc, act_bit, pend_bit) in bitmap {
            self.active[exc] = v & (1 << act_bit) != 0;
            if let Some(pb) = pend_bit {
                self.pending[exc] = v & (1 << pb) != 0;
            }
        }
    }

    fn icsr(&self, ipsr: u32) -> u32 {
        let mut v = self.vectactive(ipsr);
        // RETTOBASE: no active exception other than the current one
        let others = self.active.iter().enumerate().filter(|(e, &a)| a && *e as u32 != ipsr).count();
        if others == 0 && ipsr != 0 {
            v |= 1 << 11;
        }
        if let Some((exc, _)) = self.highest_pending() {
            v |= (exc as u32 & 0x1FF) << 12;
        }
        if self.pending[16..].iter().any(|&p| p) {
            v |= 1 << 22;
        }
        if self.pending[EXC_SYSTICK] {
            v |= 1 << 26;
        }
        if self.pending[EXC_PENDSV] {
            v |= 1 << 28;
        }
        if self.pending[EXC_NMI] {
            v |= 1 << 31;
        }
        v
    }

    /// Read a 32-bit SCS register at `off` (offset from 0xE000_E000). `ipsr` is the current
    /// exception number (for ICSR.VECTACTIVE).
    pub fn read(&self, off: u32, ipsr: u32) -> u32 {
        let word_bits = |v: &[bool], base: usize| -> u32 {
            let mut w = 0u32;
            for i in 0..32 {
                if v.get(base + i).copied().unwrap_or(false) {
                    w |= 1 << i;
                }
            }
            w
        };
        match off {
            0x004 => (NUM_IRQ as u32).div_ceil(32).saturating_sub(1), // ICTR
            0x100..=0x17F => {
                let n = ((off - 0x100) / 4) as usize;
                word_bits(&self.enabled, 16 + n * 32)
            }
            0x180..=0x1FF => {
                let n = ((off - 0x180) / 4) as usize;
                word_bits(&self.enabled, 16 + n * 32)
            }
            0x200..=0x27F => {
                let n = ((off - 0x200) / 4) as usize;
                word_bits(&self.pending, 16 + n * 32)
            }
            0x280..=0x2FF => {
                let n = ((off - 0x280) / 4) as usize;
                word_bits(&self.pending, 16 + n * 32)
            }
            0x300..=0x37F => {
                let n = ((off - 0x300) / 4) as usize;
                word_bits(&self.active, 16 + n * 32)
            }
            0x400..=0x5EF => {
                let base = (off - 0x400) as usize;
                let mut w = 0u32;
                for i in 0..4 {
                    w |= (self.ipr.get(base + i).copied().unwrap_or(0) as u32) << (8 * i);
                }
                w
            }
            0xD00 => 0x411F_C231,
            0xD04 => self.icsr(ipsr),
            0xD08 => self.vtor,
            0xD0C => 0xFA05_0000 | ((self.prigroup as u32) << 8),
            0xD10 => self.scr,
            0xD14 => self.ccr,
            0xD18..=0xD23 => {
                let base = (off - 0xD18) as usize;
                let mut w = 0u32;
                for i in 0..4 {
                    w |= (self.shpr.get(base + i).copied().unwrap_or(0) as u32) << (8 * i);
                }
                w
            }
            0xD24 => self.shcsr(),
            0xD28 => self.cfsr,
            0xD2C => self.hfsr,
            0xD30 => self.dfsr,
            0xD34 => self.mmfar,
            0xD38 => self.bfar,
            0xDFC => self.demcr,
            _ => 0,
        }
    }

    /// Write an SCS register. `mask` selects the byte lanes actually written so that byte
    /// and halfword stores to set/clear registers only touch the addressed bits.
    pub fn write(&mut self, off: u32, val: u32, mask: u32, ipsr: u32) {
        let v = val & mask;
        let set_bits = |vec: &mut Vec<bool>, base: usize, bits: u32, on: bool| {
            for i in 0..32 {
                if bits & (1 << i) != 0 {
                    if let Some(slot) = vec.get_mut(base + i) {
                        *slot = on;
                    }
                }
            }
        };
        match off {
            0x100..=0x17F => set_bits(&mut self.enabled, 16 + ((off - 0x100) / 4) as usize * 32, v, true),
            0x180..=0x1FF => set_bits(&mut self.enabled, 16 + ((off - 0x180) / 4) as usize * 32, v, false),
            0x200..=0x27F => set_bits(&mut self.pending, 16 + ((off - 0x200) / 4) as usize * 32, v, true),
            0x280..=0x2FF => {
                let base = 16 + ((off - 0x280) / 4) as usize * 32;
                set_bits(&mut self.pending, base, v, false);
            }
            0x400..=0x5EF => {
                let base = (off - 0x400) as usize;
                for i in 0..4 {
                    if mask & (0xFF << (8 * i)) != 0 {
                        if let Some(slot) = self.ipr.get_mut(base + i) {
                            *slot = ((val >> (8 * i)) as u8) & PRIO_MASK;
                        }
                    }
                }
            }
            0xD04 => {
                if v & (1 << 31) != 0 {
                    self.pending[EXC_NMI] = true;
                }
                if v & (1 << 28) != 0 {
                    self.pending[EXC_PENDSV] = true;
                } else if v & (1 << 27) != 0 {
                    self.pending[EXC_PENDSV] = false;
                }
                if v & (1 << 26) != 0 {
                    self.pending[EXC_SYSTICK] = true;
                } else if v & (1 << 25) != 0 {
                    self.pending[EXC_SYSTICK] = false;
                }
            }
            0xD08 => {
                let full = (self.vtor & !mask) | v;
                self.vtor = full & 0x3FFF_FF80;
            }
            0xD0C => {
                if mask == 0xFFFF_FFFF && (val >> 16) == 0x05FA {
                    self.prigroup = ((val >> 8) & 7) as u8;
                    if val & (1 << 2) != 0 || val & 1 != 0 {
                        self.reset_requested = true;
                    }
                    if val & (1 << 1) != 0 && ipsr == 0 {
                        // VECTCLRACTIVE (debug use only)
                        self.active.iter_mut().for_each(|a| *a = false);
                    }
                }
            }
            0xD10 => self.scr = ((self.scr & !mask) | v) & 0x16,
            0xD14 => self.ccr = ((self.ccr & !mask) | v) & 0x31B,
            0xD18..=0xD23 => {
                let base = (off - 0xD18) as usize;
                for i in 0..4 {
                    if mask & (0xFF << (8 * i)) != 0 {
                        if let Some(slot) = self.shpr.get_mut(base + i) {
                            // SHPR1 byte 3 (exc 7), SHPR2 bytes 0-2 (exc 8-10), SHPR3 byte 1 (exc 13) are reserved
                            let exc = base + i + 4;
                            *slot = if matches!(exc, 7..=10 | 13) { 0 } else { ((val >> (8 * i)) as u8) & PRIO_MASK };
                        }
                    }
                }
            }
            0xD24 => {
                let full = (self.shcsr() & !mask) | v;
                self.write_shcsr(full);
            }
            0xD28 => self.cfsr &= !v,
            0xD2C => self.hfsr &= !v,
            0xD30 => self.dfsr &= !v,
            0xD34 => self.mmfar = (self.mmfar & !mask) | v,
            0xD38 => self.bfar = (self.bfar & !mask) | v,
            0xDFC => self.demcr = ((self.demcr & !mask) | v) & 0x010F_07F1,
            0xF00 => {
                let irq = (val & 0x1FF) as usize;
                if irq < NUM_IRQ {
                    self.pending[16 + irq] = true;
                }
            }
            _ => {}
        }
    }
}
