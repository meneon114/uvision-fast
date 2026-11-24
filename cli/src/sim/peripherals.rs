//! STM32F10x peripheral models: RCC (clock tree + clock gating), FLASH ACR, AFIO, EXTI,
//! GPIO, TIM1-TIM4, USART1-3, SysTick, NVIC/SCB and the DWT cycle counter.
//!
//! References: RM0008 (STM32F10x reference manual) and PM0056 (Cortex-M3 programming manual).
//! Everything here is driven in HCLK cycles by [`Peripherals::tick`]; bus clocks are derived
//! from the RCC configuration exactly as on silicon (APB prescalers, the x2 timer clock
//! multiplier, SysTick's HCLK/8 reference).

use crate::sim::nvic::{Nvic, EXC_SYSTICK};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub const HSI_HZ: u32 = 8_000_000;

// IRQ numbers (RM0008 table 63)
pub const IRQ_EXTI0: usize = 6;
pub const IRQ_EXTI9_5: usize = 23;
pub const IRQ_TIM1_BRK: usize = 24;
pub const IRQ_TIM1_UP: usize = 25;
pub const IRQ_TIM1_CC: usize = 27;
pub const IRQ_TIM2: usize = 28;
pub const IRQ_TIM3: usize = 29;
pub const IRQ_TIM4: usize = 30;
pub const IRQ_USART1: usize = 37;
pub const IRQ_USART2: usize = 38;
pub const IRQ_USART3: usize = 39;
pub const IRQ_EXTI15_10: usize = 40;

// ---------------------------------------------------------------------------------------------
// RCC
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RccState {
    pub cr: u32,
    pub cfgr: u32,
    pub cir: u32,
    pub apb2rstr: u32,
    pub apb1rstr: u32,
    pub ahbenr: u32,
    pub apb2enr: u32,
    pub apb1enr: u32,
    pub bdcr: u32,
    pub csr: u32,
    /// External crystal frequency (HSE).
    #[serde(default)]
    pub hse_hz: u32,
    /// Derived clocks (read-only, refreshed on every RCC write).
    #[serde(default)]
    pub sysclk_hz: u32,
    #[serde(default)]
    pub hclk_hz: u32,
    #[serde(default)]
    pub pclk1_hz: u32,
    #[serde(default)]
    pub pclk2_hz: u32,
}

impl RccState {
    pub fn new(hse_hz: u32) -> Self {
        let mut r = Self {
            cr: 0x0000_0083, // HSITRIM=16, HSIRDY, HSION
            cfgr: 0,
            cir: 0,
            apb2rstr: 0,
            apb1rstr: 0,
            ahbenr: 0x0000_0014, // SRAMEN | FLITFEN
            apb2enr: 0,
            apb1enr: 0,
            bdcr: 0,
            csr: 0x0C00_0000,
            hse_hz,
            sysclk_hz: HSI_HZ,
            hclk_hz: HSI_HZ,
            pclk1_hz: HSI_HZ,
            pclk2_hz: HSI_HZ,
        };
        r.recompute();
        r
    }

    fn pll_in_hz(&self) -> u64 {
        if self.cfgr & (1 << 16) == 0 {
            (HSI_HZ / 2) as u64
        } else if self.cfgr & (1 << 17) != 0 {
            (self.hse_hz / 2) as u64
        } else {
            self.hse_hz as u64
        }
    }

    fn pll_mul(&self) -> u64 {
        let m = (self.cfgr >> 18) & 0xF;
        if m >= 14 {
            16
        } else {
            (m + 2) as u64
        }
    }

    pub fn hpre_div(&self) -> u32 {
        match (self.cfgr >> 4) & 0xF {
            0..=7 => 1,
            8 => 2,
            9 => 4,
            10 => 8,
            11 => 16,
            12 => 64,
            13 => 128,
            14 => 256,
            _ => 512,
        }
    }

    fn ppre_div(bits: u32) -> u32 {
        match bits & 7 {
            0..=3 => 1,
            4 => 2,
            5 => 4,
            6 => 8,
            _ => 16,
        }
    }

    pub fn ppre1_div(&self) -> u32 {
        Self::ppre_div(self.cfgr >> 8)
    }

    pub fn ppre2_div(&self) -> u32 {
        Self::ppre_div(self.cfgr >> 11)
    }

    fn source_ready(&self, sw: u32) -> bool {
        match sw {
            0 => self.cr & (1 << 1) != 0,
            1 => self.cr & (1 << 17) != 0,
            2 => self.cr & (1 << 25) != 0,
            _ => false,
        }
    }

    /// Update ready flags / clock switch status and the derived frequencies.
    pub fn recompute(&mut self) {
        // Oscillators and the PLL lock immediately in the model.
        if self.cr & 1 != 0 {
            self.cr |= 1 << 1;
        } else {
            self.cr &= !(1 << 1);
        }
        if self.cr & (1 << 16) != 0 {
            self.cr |= 1 << 17;
        } else {
            self.cr &= !(1 << 17);
        }
        let pll_src_ready =
            if self.cfgr & (1 << 16) != 0 { self.cr & (1 << 17) != 0 } else { self.cr & (1 << 1) != 0 };
        if self.cr & (1 << 24) != 0 && pll_src_ready {
            self.cr |= 1 << 25;
        } else {
            self.cr &= !(1 << 25);
        }
        let sw = self.cfgr & 3;
        if sw != 3 && self.source_ready(sw) {
            self.cfgr = (self.cfgr & !0xC) | (sw << 2);
        }
        let sws = (self.cfgr >> 2) & 3;
        if !self.source_ready(sws) {
            // Current source stopped: the hardware falls back to HSI
            self.cr |= 0b11;
            self.cfgr &= !0xF;
        }
        let sws = (self.cfgr >> 2) & 3;
        let sys = match sws {
            1 => self.hse_hz as u64,
            2 => self.pll_in_hz() * self.pll_mul(),
            _ => HSI_HZ as u64,
        };
        self.sysclk_hz = sys.min(u32::MAX as u64) as u32;
        self.hclk_hz = (self.sysclk_hz / self.hpre_div()).max(1);
        self.pclk1_hz = (self.hclk_hz / self.ppre1_div()).max(1);
        self.pclk2_hz = (self.hclk_hz / self.ppre2_div()).max(1);
    }

    fn write(&mut self, off: u32, val: u32) {
        match off {
            0x00 => {
                let mut v = val & 0x010D_00F9;
                let sws = (self.cfgr >> 2) & 3;
                // A clock that currently drives SYSCLK (directly or through the PLL) cannot be stopped
                if sws == 0 || (sws == 2 && self.cfgr & (1 << 16) == 0) {
                    v |= 1;
                }
                if sws == 1 {
                    v |= 1 << 16;
                }
                if sws == 2 {
                    v |= 1 << 24;
                    if self.cfgr & (1 << 16) != 0 {
                        v |= 1 << 16;
                    }
                }
                self.cr = (self.cr & 0x0202_0002) | v;
            }
            0x04 => {
                // PLLSRC / PLLXTPRE / PLLMUL can only be written while the PLL is off
                let pll_bits = 0x003F_0000;
                let mut v = val & 0x077F_FFF3;
                if self.cr & (1 << 24) != 0 {
                    v = (v & !pll_bits) | (self.cfgr & pll_bits);
                }
                self.cfgr = (self.cfgr & 0xC) | v;
            }
            0x08 => self.cir = val & 0x0000_1F00,
            0x0C => self.apb2rstr = val,
            0x10 => self.apb1rstr = val,
            0x14 => self.ahbenr = val & 0x0000_0557,
            0x18 => self.apb2enr = val & 0x0000_FFFD,
            0x1C => self.apb1enr = val & 0x3AFE_C83F,
            0x20 => self.bdcr = val & 0x0001_8305,
            0x24 => self.csr = (self.csr & !0x01) | (val & 0x01),
            _ => {}
        }
        self.recompute();
    }

    fn read(&self, off: u32) -> u32 {
        match off {
            0x00 => self.cr,
            0x04 => self.cfgr,
            0x08 => self.cir,
            0x0C => self.apb2rstr,
            0x10 => self.apb1rstr,
            0x14 => self.ahbenr,
            0x18 => self.apb2enr,
            0x1C => self.apb1enr,
            0x20 => self.bdcr,
            0x24 => self.csr,
            _ => 0,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// GPIO
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpioPort {
    pub name: String,
    pub crl: u32,
    pub crh: u32,
    pub idr: u32,
    pub odr: u32,
    pub lckr: u32,
    /// Externally applied pin levels (buttons, keypad, signal generator).
    pub ext_pins: u16,
    /// Pins that have an external driver attached.
    pub ext_mask: u16,
    /// Alternate-function output levels driven by on-chip peripherals.
    #[serde(default)]
    pub af_val: u16,
    #[serde(default)]
    pub af_mask: u16,
    /// Physical pin levels.
    #[serde(default)]
    pub pins: u16,
}

impl GpioPort {
    pub fn new(name: &str) -> Self {
        let mut g = Self {
            name: name.to_string(),
            crl: 0x4444_4444,
            crh: 0x4444_4444,
            idr: 0,
            odr: 0,
            lckr: 0,
            ext_pins: 0,
            ext_mask: 0,
            af_val: 0,
            af_mask: 0,
            pins: 0,
        };
        g.refresh_idr();
        g
    }

    /// 4-bit CNF:MODE field of a pin.
    pub fn pin_cfg(&self, pin: u32) -> u32 {
        if pin < 8 {
            (self.crl >> (pin * 4)) & 0xF
        } else {
            (self.crh >> ((pin - 8) * 4)) & 0xF
        }
    }

    pub fn is_output(&self, pin: u32) -> bool {
        self.pin_cfg(pin) & 3 != 0
    }

    /// Recompute physical pin levels and IDR from configuration, ODR, AF outputs and
    /// external drivers.
    pub fn refresh_idr(&mut self) {
        let mut pins = 0u16;
        let mut idr = 0u16;
        for pin in 0..16u32 {
            let b = 1u16 << pin;
            let cfg = self.pin_cfg(pin);
            let mode = cfg & 3;
            let cnf = cfg >> 2;
            let ext = if self.ext_mask & b != 0 { Some(self.ext_pins & b != 0) } else { None };
            let odr = self.odr as u16 & b != 0;
            let level = if mode == 0 {
                match cnf {
                    // input with pull-up / pull-down selected by ODR
                    2 => ext.unwrap_or(odr),
                    _ => ext.unwrap_or(false),
                }
            } else {
                let drive = if cnf & 2 != 0 { self.af_mask & b != 0 && self.af_val & b != 0 } else { odr };
                if cnf & 1 != 0 {
                    // open drain: '1' releases the line
                    if drive {
                        ext.unwrap_or(true)
                    } else {
                        false
                    }
                } else {
                    drive
                }
            };
            if level {
                pins |= b;
                // analog inputs read as 0
                if !(mode == 0 && cnf == 0) {
                    idr |= b;
                }
            }
        }
        self.pins = pins;
        self.idr = idr as u32;
    }

    pub fn set_ext_pin(&mut self, pin: u8, high: bool) {
        if pin >= 16 {
            return;
        }
        let bit = 1u16 << pin;
        self.ext_mask |= bit;
        if high {
            self.ext_pins |= bit;
        } else {
            self.ext_pins &= !bit;
        }
        self.refresh_idr();
    }

    /// Disconnect the external driver from a pin.
    pub fn release_ext_pin(&mut self, pin: u8) {
        if pin < 16 {
            self.ext_mask &= !(1 << pin);
            self.ext_pins &= !(1 << pin);
            self.refresh_idr();
        }
    }

    pub fn read(&mut self, offset: u32) -> u32 {
        self.refresh_idr();
        match offset {
            0x00 => self.crl,
            0x04 => self.crh,
            0x08 => self.idr,
            0x0C => self.odr,
            0x18 => self.lckr,
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: u32, val: u32) {
        let locked = self.lckr & (1 << 16) != 0;
        let lckr = self.lckr;
        let lock_mask = |cr: u32, base: u32, new: u32| -> u32 {
            if !locked {
                return new;
            }
            let mut out = new;
            for p in 0..8 {
                if lckr & (1 << (p + base)) != 0 {
                    out = (out & !(0xF << (p * 4))) | (cr & (0xF << (p * 4)));
                }
            }
            out
        };
        match offset {
            0x00 => self.crl = lock_mask(self.crl, 0, val),
            0x04 => self.crh = lock_mask(self.crh, 8, val),
            0x0C => self.odr = val & 0xFFFF,
            0x10 => {
                let set_bits = val & 0xFFFF;
                let reset_bits = (val >> 16) & 0xFFFF;
                self.odr = (self.odr & !reset_bits) | set_bits;
            }
            0x14 => self.odr &= !(val & 0xFFFF),
            0x18
                if !locked => {
                    self.lckr = val & 0x1_FFFF;
                }
            _ => {}
        }
        self.refresh_idr();
    }
}

// ---------------------------------------------------------------------------------------------
// General-purpose / advanced timers (edge-aligned up/down counting, output compare, PWM)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimerState {
    pub name: String,
    pub irq_num: u8,
    pub cr1: u32,
    pub cr2: u32,
    pub smcr: u32,
    pub dier: u32,
    pub sr: u32,
    pub ccmr1: u32,
    pub ccmr2: u32,
    pub ccer: u32,
    pub cnt: u32,
    pub psc: u32,
    pub arr: u32,
    pub rcr: u32,
    pub ccr: [u32; 4],
    /// Prescaler counter (timer-clock ticks into the current prescaler period).
    pub psc_counter: u32,
    #[serde(default)]
    pub bdtr: u32,
    /// Shadow (active) registers, loaded at update events.
    #[serde(default)]
    pub psc_active: u32,
    #[serde(default)]
    pub arr_active: u32,
    #[serde(default)]
    pub ccr_active: [u32; 4],
    /// OCxREF levels.
    #[serde(default)]
    pub oc_ref: [bool; 4],
    #[serde(default)]
    pub advanced: bool,
    /// HCLK cycles carried towards the next timer clock when TIMxCLK < HCLK.
    #[serde(default)]
    pub hclk_acc: u32,
}

impl TimerState {
    pub fn new(name: &str, irq_num: u8) -> Self {
        Self {
            name: name.to_string(),
            irq_num,
            cr1: 0,
            cr2: 0,
            smcr: 0,
            dier: 0,
            sr: 0,
            ccmr1: 0,
            ccmr2: 0,
            ccer: 0,
            cnt: 0,
            psc: 0,
            arr: 0xFFFF,
            rcr: 0,
            ccr: [0; 4],
            psc_counter: 0,
            bdtr: 0,
            psc_active: 0,
            arr_active: 0xFFFF,
            ccr_active: [0; 4],
            oc_ref: [false; 4],
            advanced: name == "TIM1",
            hclk_acc: 0,
        }
    }

    fn ccmr_field(&self, ch: usize) -> u32 {
        let reg = if ch < 2 { self.ccmr1 } else { self.ccmr2 };
        (reg >> ((ch % 2) * 8)) & 0xFF
    }

    fn oc_mode(&self, ch: usize) -> u32 {
        (self.ccmr_field(ch) >> 4) & 7
    }

    fn is_output_channel(&self, ch: usize) -> bool {
        self.ccmr_field(ch) & 3 == 0
    }

    fn oc_preload(&self, ch: usize) -> bool {
        self.ccmr_field(ch) & (1 << 3) != 0
    }

    fn down(&self) -> bool {
        self.cr1 & (1 << 4) != 0 && (self.cr1 >> 5) & 3 == 0
    }

    pub fn read(&self, offset: u32) -> u32 {
        match offset {
            0x00 => self.cr1,
            0x04 => self.cr2,
            0x08 => self.smcr,
            0x0C => self.dier,
            0x10 => self.sr,
            0x18 => self.ccmr1,
            0x1C => self.ccmr2,
            0x20 => self.ccer,
            0x24 => self.cnt,
            0x28 => self.psc,
            0x2C => self.arr,
            0x30 if self.advanced => self.rcr,
            0x34 => self.ccr[0],
            0x38 => self.ccr[1],
            0x3C => self.ccr[2],
            0x40 => self.ccr[3],
            0x44 if self.advanced => self.bdtr,
            _ => 0,
        }
    }

    fn update_shadows(&mut self) {
        self.psc_active = self.psc;
        self.arr_active = self.arr;
        self.ccr_active = self.ccr;
    }

    /// Recompute PWM / forced OCxREF levels from the current counter.
    fn refresh_ref(&mut self) {
        let up = !self.down();
        for ch in 0..4 {
            if !self.is_output_channel(ch) {
                continue;
            }
            let ccr = self.ccr_active[ch];
            match self.oc_mode(ch) {
                4 => self.oc_ref[ch] = false,
                5 => self.oc_ref[ch] = true,
                6 => self.oc_ref[ch] = if up { self.cnt < ccr } else { self.cnt <= ccr && ccr != 0 },
                7 => self.oc_ref[ch] = if up { self.cnt >= ccr } else { !(self.cnt <= ccr && ccr != 0) },
                _ => {}
            }
        }
    }

    pub fn write(&mut self, offset: u32, val: u32) {
        match offset {
            0x00 => {
                self.cr1 = val & 0x3FF;
                if self.cr1 & (1 << 7) == 0 {
                    self.arr_active = self.arr;
                }
            }
            0x04 => self.cr2 = val & 0xFFF8,
            0x08 => self.smcr = val & 0xFFF7,
            0x0C => self.dier = val & 0x7FDF,
            0x10 => self.sr &= val | !0x1EFF, // rc_w0
            0x14 => {
                if val & 1 != 0 {
                    // UG: re-initialise the counter and load the shadow registers
                    self.cnt = if self.down() { self.arr } else { 0 };
                    self.psc_counter = 0;
                    self.update_shadows();
                    if self.cr1 & (1 << 2) == 0 {
                        self.sr |= 1;
                    }
                }
                for ch in 0..4 {
                    if val & (1 << (ch + 1)) != 0 {
                        self.sr |= 1 << (ch + 1);
                    }
                }
                if val & (1 << 6) != 0 {
                    self.sr |= 1 << 6;
                }
            }
            0x18 => self.ccmr1 = val & 0xFFFF,
            0x1C => self.ccmr2 = val & 0xFFFF,
            0x20 => self.ccer = val & 0x3FFF,
            0x24 => self.cnt = val & 0xFFFF,
            0x28 => self.psc = val & 0xFFFF,
            0x2C => {
                self.arr = val & 0xFFFF;
                if self.cr1 & (1 << 7) == 0 {
                    self.arr_active = self.arr;
                }
            }
            0x30 if self.advanced => self.rcr = val & 0xFF,
            0x34 | 0x38 | 0x3C | 0x40 => {
                let ch = ((offset - 0x34) / 4) as usize;
                self.ccr[ch] = val & 0xFFFF;
                if !self.oc_preload(ch) {
                    self.ccr_active[ch] = self.ccr[ch];
                }
            }
            0x44 if self.advanced => self.bdtr = val & 0xFFFF,
            _ => {}
        }
        self.refresh_ref();
    }

    fn update_event(&mut self) {
        if self.cr1 & (1 << 1) != 0 {
            return; // UDIS
        }
        self.update_shadows();
        self.sr |= 1;
        if self.cr1 & (1 << 3) != 0 {
            self.cr1 &= !1; // one-pulse mode: the counter stops at the update event
        }
    }

    fn compare_events(&mut self) {
        for ch in 0..4 {
            if self.cnt != self.ccr_active[ch] {
                continue;
            }
            self.sr |= 1 << (ch + 1);
            if !self.is_output_channel(ch) {
                continue;
            }
            match self.oc_mode(ch) {
                1 => self.oc_ref[ch] = true,
                2 => self.oc_ref[ch] = false,
                3 => self.oc_ref[ch] = !self.oc_ref[ch],
                _ => {}
            }
        }
    }

    fn count_one(&mut self) {
        if self.down() {
            if self.cnt == 0 {
                self.cnt = self.arr_active;
                self.update_event();
            } else {
                self.cnt -= 1;
            }
        } else if self.cnt == self.arr_active {
            self.cnt = 0;
            self.update_event();
        } else {
            self.cnt = (self.cnt + 1) & 0xFFFF;
        }
        self.compare_events();
        self.refresh_ref();
    }

    /// Counter steps that can be skipped without reaching an update or compare event.
    fn quiet_counts(&self) -> u64 {
        let down = self.down();
        let mut d: u64 = if down {
            self.cnt as u64
        } else if self.cnt <= self.arr_active {
            (self.arr_active - self.cnt) as u64
        } else {
            (0xFFFF - self.cnt) as u64
        };
        for ch in 0..4 {
            let c = self.ccr_active[ch];
            let dist = if down {
                if c < self.cnt {
                    (self.cnt - c) as u64
                } else {
                    continue;
                }
            } else if c > self.cnt {
                (c - self.cnt) as u64
            } else {
                continue;
            };
            d = d.min(dist);
        }
        d.saturating_sub(1)
    }

    /// Advance by `tclk` timer-clock cycles.
    pub fn advance(&mut self, mut tclk: u64) {
        while tclk > 0 && self.cr1 & 1 != 0 {
            let div = self.psc_active as u64 + 1;
            let to_inc = div - self.psc_counter as u64;
            if tclk < to_inc {
                self.psc_counter += tclk as u32;
                return;
            }
            tclk -= to_inc;
            self.psc_counter = 0;
            if self.arr_active == 0 {
                continue; // the counter is blocked while the auto-reload value is 0
            }
            self.count_one();
            let skip = (tclk / div).min(self.quiet_counts());
            if skip > 0 && self.cr1 & 1 != 0 {
                if self.down() {
                    self.cnt -= skip as u32;
                } else {
                    self.cnt += skip as u32;
                }
                tclk -= skip * div;
                self.refresh_ref();
            }
        }
    }

    /// Timer clocks until the next event that changes flags or outputs.
    pub fn clocks_to_event(&self) -> Option<u64> {
        if self.cr1 & 1 == 0 || self.arr_active == 0 {
            return None;
        }
        let div = self.psc_active as u64 + 1;
        Some((self.quiet_counts() + 1) * div - self.psc_counter as u64)
    }

    /// Output level of channel `ch` at the pin (None = output disabled).
    pub fn channel_output(&self, ch: usize) -> Option<bool> {
        if !self.is_output_channel(ch) || self.ccer & (1 << (ch * 4)) == 0 {
            return None;
        }
        if self.advanced && self.bdtr & (1 << 15) == 0 {
            return None; // MOE = 0
        }
        let pol = self.ccer & (1 << (ch * 4 + 1)) != 0;
        Some(self.oc_ref[ch] ^ pol)
    }

    fn irq_level(&self) -> bool {
        self.sr & self.dier & 0x5F != 0
    }
}

// ---------------------------------------------------------------------------------------------
// USART
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsartState {
    pub name: String,
    pub irq_num: u8,
    pub sr: u32,
    pub brr: u32,
    pub cr1: u32,
    pub cr2: u32,
    pub cr3: u32,
    pub gtpr: u32,
    pub tx_buffer: Vec<u8>,
    pub rx_queue: VecDeque<u8>,
    #[serde(default)]
    pub rdr: u8,
    /// Byte waiting in TDR (TXE = 0).
    #[serde(default)]
    pub tdr: Option<u8>,
    /// Byte in the transmit shift register and HCLK cycles until its stop bit ends.
    #[serde(default)]
    pub tx_shift: Option<(u8, u64)>,
    /// HCLK cycles until the byte at the head of `rx_queue` has been received.
    #[serde(default)]
    pub rx_remaining: Option<u64>,
}

impl UsartState {
    pub fn new(name: &str, irq_num: u8) -> Self {
        Self {
            name: name.to_string(),
            irq_num,
            sr: 0x00C0, // TXE | TC
            brr: 0,
            cr1: 0,
            cr2: 0,
            cr3: 0,
            gtpr: 0,
            tx_buffer: Vec::new(),
            rx_queue: VecDeque::new(),
            rdr: 0,
            tdr: None,
            tx_shift: None,
            rx_remaining: None,
        }
    }

    pub fn push_rx_bytes(&mut self, bytes: &[u8]) {
        self.rx_queue.extend(bytes.iter().copied());
    }

    fn enabled(&self) -> bool {
        self.cr1 & (1 << 13) != 0
    }

    /// HCLK cycles per frame (start + data + stop bits) at the programmed baud rate
    /// (baud = PCLK / BRR).
    fn frame_cycles(&self, apb_div: u32) -> Option<u64> {
        if self.brr < 16 {
            return None;
        }
        let data_bits = if self.cr1 & (1 << 12) != 0 { 9 } else { 8 };
        let stop_halves = match (self.cr2 >> 12) & 3 {
            0 => 2,
            1 => 1,
            2 => 4,
            _ => 3,
        };
        let halves = 2 * (1 + data_bits) + stop_halves;
        Some(self.brr as u64 * halves as u64 * apb_div as u64 / 2)
    }

    pub fn read(&mut self, offset: u32, side_effects: bool) -> u32 {
        match offset {
            0x00 => self.sr,
            0x04 => {
                if side_effects {
                    self.sr &= !((1 << 5) | 0xF); // RXNE, ORE, NE, FE, PE
                }
                self.rdr as u32
            }
            0x08 => self.brr,
            0x0C => self.cr1,
            0x10 => self.cr2,
            0x14 => self.cr3,
            0x18 => self.gtpr,
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: u32, val: u32, apb_div: u32) {
        match offset {
            // rc_w0 for RXNE, TC, LBD, CTS
            0x00 => self.sr &= val | !0x0360,
            0x04 => {
                if !self.enabled() {
                    return;
                }
                let ch = (val & 0xFF) as u8;
                self.sr &= !(1 << 6); // TC
                if self.tx_shift.is_none() && self.cr1 & (1 << 3) != 0 {
                    if let Some(fc) = self.frame_cycles(apb_div) {
                        self.tx_shift = Some((ch, fc));
                        self.sr |= 1 << 7;
                        return;
                    }
                }
                self.tdr = Some(ch);
                self.sr &= !(1 << 7);
            }
            0x08 => self.brr = val & 0xFFFF,
            0x0C => {
                self.cr1 = val & 0x3FFF;
                if !self.enabled() {
                    self.tx_shift = None;
                    self.tdr = None;
                    self.rx_remaining = None;
                    self.sr = (self.sr & !(1 << 5)) | 0xC0;
                }
            }
            0x10 => self.cr2 = val & 0x7F7F,
            0x14 => self.cr3 = val & 0x07FF,
            0x18 => self.gtpr = val & 0xFFFF,
            _ => {}
        }
    }

    pub fn advance(&mut self, mut cycles: u64, apb_div: u32) {
        if !self.enabled() {
            return;
        }
        if self.tx_shift.is_none() && self.tdr.is_some() && self.cr1 & (1 << 3) != 0 {
            if let Some(fc) = self.frame_cycles(apb_div) {
                self.tx_shift = Some((self.tdr.take().unwrap_or(0), fc));
                self.sr |= 1 << 7;
            }
        }
        let mut c = cycles;
        while let Some((ch, rem)) = self.tx_shift {
            if c < rem {
                self.tx_shift = Some((ch, rem - c));
                break;
            }
            c -= rem;
            self.tx_buffer.push(ch);
            self.tx_shift = None;
            match (self.tdr, self.frame_cycles(apb_div)) {
                (Some(next), Some(fc)) => {
                    self.tdr = None;
                    self.tx_shift = Some((next, fc));
                    self.sr |= 1 << 7;
                }
                _ => self.sr |= 1 << 6, // TC
            }
        }
        if self.cr1 & (1 << 2) == 0 {
            return;
        }
        while !self.rx_queue.is_empty() {
            let Some(fc) = self.frame_cycles(apb_div) else { return };
            let rem = self.rx_remaining.unwrap_or(fc);
            if cycles < rem {
                self.rx_remaining = Some(rem - cycles);
                return;
            }
            cycles -= rem;
            self.rx_remaining = None;
            let b = self.rx_queue.pop_front().unwrap_or(0);
            if self.sr & (1 << 5) != 0 {
                self.sr |= 1 << 3; // ORE: the new byte is lost
            } else {
                self.rdr = b;
                self.sr |= 1 << 5;
            }
        }
    }

    fn cycles_to_event(&self, apb_div: u32) -> Option<u64> {
        if !self.enabled() {
            return None;
        }
        let tx = self.tx_shift.map(|(_, r)| r);
        let rx = if self.cr1 & (1 << 2) != 0 && !self.rx_queue.is_empty() {
            self.rx_remaining.or_else(|| self.frame_cycles(apb_div))
        } else {
            None
        };
        match (tx, rx) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    fn irq_level(&self) -> bool {
        let sr = self.sr;
        let cr1 = self.cr1;
        (sr & (1 << 7) != 0 && cr1 & (1 << 7) != 0)
            || (sr & (1 << 6) != 0 && cr1 & (1 << 6) != 0)
            || (sr & ((1 << 5) | (1 << 3)) != 0 && cr1 & (1 << 5) != 0)
            || (sr & (1 << 4) != 0 && cr1 & (1 << 4) != 0)
    }
}

// ---------------------------------------------------------------------------------------------
// SysTick
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SysTickState {
    pub ctrl: u32,
    pub load: u32,
    pub val: u32,
    pub calib: u32,
    /// HCLK cycles accumulated towards the next HCLK/8 reference tick.
    #[serde(default)]
    pub div_acc: u32,
}

impl Default for SysTickState {
    fn default() -> Self {
        Self {
            ctrl: 0,
            load: 0,
            val: 0,
            // STM32F1: TENMS = 9000 (1 ms at the 9 MHz HCLK/8 reference)
            calib: 0x0000_2328,
            div_acc: 0,
        }
    }
}

impl SysTickState {
    pub fn read(&mut self, offset: u32, side_effects: bool) -> u32 {
        match offset {
            0x00 => {
                let v = self.ctrl;
                if side_effects {
                    self.ctrl &= !(1 << 16); // COUNTFLAG clears on read
                }
                v
            }
            0x04 => self.load,
            0x08 => self.val,
            0x0C => self.calib,
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: u32, val: u32) {
        match offset {
            0x00 => self.ctrl = (self.ctrl & (1 << 16)) | (val & 0x7),
            0x04 => self.load = val & 0x00FF_FFFF,
            0x08 => {
                self.val = 0;
                self.ctrl &= !(1 << 16);
            }
            _ => {}
        }
    }

    /// Advance by `cycles` HCLK cycles. The counter reloads from LOAD on the clock after it
    /// reaches 0, so the period is LOAD+1 clocks. Returns true if it reached 0.
    pub fn advance(&mut self, cycles: u64) -> bool {
        if self.ctrl & 1 == 0 {
            return false;
        }
        let mut clocks = if self.ctrl & 4 != 0 {
            cycles
        } else {
            let total = self.div_acc as u64 + cycles;
            self.div_acc = (total % 8) as u32;
            total / 8
        };
        let mut fired = false;
        while clocks > 0 {
            if self.val == 0 {
                if self.load == 0 {
                    break;
                }
                self.val = self.load;
                clocks -= 1;
                let period = self.load as u64 + 1;
                if clocks > period * 2 {
                    // fast-forward whole periods (WFI sleep)
                    clocks = clocks % period + period;
                    fired = true;
                    self.ctrl |= 1 << 16;
                }
                continue;
            }
            if clocks >= self.val as u64 {
                clocks -= self.val as u64;
                self.val = 0;
                self.ctrl |= 1 << 16;
                fired = true;
            } else {
                self.val -= clocks as u32;
                clocks = 0;
            }
        }
        fired
    }

    fn cycles_to_event(&self) -> Option<u64> {
        if self.ctrl & 1 == 0 || (self.load == 0 && self.val == 0) {
            return None;
        }
        let clocks = if self.val == 0 { self.load as u64 + 1 } else { self.val as u64 };
        Some(if self.ctrl & 4 != 0 { clocks } else { (clocks * 8).saturating_sub(self.div_acc as u64) })
    }
}

// ---------------------------------------------------------------------------------------------
// EXTI + AFIO
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtiState {
    pub imr: u32,
    pub emr: u32,
    pub rtsr: u32,
    pub ftsr: u32,
    pub swier: u32,
    pub pr: u32,
    /// Last sampled level of EXTI lines 0..15.
    #[serde(default)]
    pub lines: u16,
}

impl ExtiState {
    fn read(&self, off: u32) -> u32 {
        match off {
            0x00 => self.imr,
            0x04 => self.emr,
            0x08 => self.rtsr,
            0x0C => self.ftsr,
            0x10 => self.swier,
            0x14 => self.pr,
            _ => 0,
        }
    }

    fn write(&mut self, off: u32, val: u32) {
        let m = 0x000F_FFFF;
        match off {
            0x00 => self.imr = val & m,
            0x04 => self.emr = val & m,
            0x08 => self.rtsr = val & m,
            0x0C => self.ftsr = val & m,
            0x10 => {
                let rising = val & !self.swier & m;
                self.swier |= val & m;
                self.pr |= rising & self.imr;
            }
            0x14 => {
                self.pr &= !(val & m);
                self.swier &= !(val & m);
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AfioState {
    pub evcr: u32,
    pub mapr: u32,
    pub exticr: [u32; 4],
}

// ---------------------------------------------------------------------------------------------
// Logic analyzer
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogicSample {
    pub cycle: u64,
    pub value: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogicChannel {
    pub signal: String,
    pub samples: Vec<LogicSample>,
    pub last_value: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct LogicAnalyzer {
    pub channels: Vec<LogicChannel>,
}

impl LogicAnalyzer {
    pub fn add_signal(&mut self, signal: &str) {
        if !self.channels.iter().any(|c| c.signal.eq_ignore_ascii_case(signal)) {
            self.channels.push(LogicChannel { signal: signal.to_string(), samples: Vec::new(), last_value: None });
        }
    }

    pub fn remove_signal(&mut self, signal: &str) {
        self.channels.retain(|c| !c.signal.eq_ignore_ascii_case(signal));
    }

    pub fn clear_samples(&mut self) {
        for ch in &mut self.channels {
            ch.samples.clear();
            ch.last_value = None;
        }
    }

    pub fn record_point(&mut self, idx: usize, cycle: u64, value: u64) {
        let Some(ch) = self.channels.get_mut(idx) else {
            return;
        };
        if ch.last_value != Some(value) {
            if ch.samples.len() >= 50_000 {
                ch.samples.drain(0..10_000);
            }
            ch.samples.push(LogicSample { cycle, value });
            ch.last_value = Some(value);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Snapshot + container
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeripheralSnapshot {
    pub rcc: RccState,
    pub gpios: Vec<GpioPort>,
    pub timers: Vec<TimerState>,
    pub usarts: Vec<UsartState>,
    pub systick: SysTickState,
    pub vtor: u32,
    pub uart1_text: String,
    #[serde(default)]
    pub exti: ExtiState,
    #[serde(default)]
    pub afio: AfioState,
    #[serde(default)]
    pub flash_acr: u32,
    #[serde(default)]
    pub nvic_enabled: Vec<u32>,
    #[serde(default)]
    pub nvic_pending: Vec<u32>,
    #[serde(default)]
    pub nvic_active: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct Peripherals {
    pub rcc: RccState,
    pub gpios: [GpioPort; 7],
    pub timers: [TimerState; 4], // TIM1, TIM2, TIM3, TIM4
    pub usarts: [UsartState; 3], // USART1, USART2, USART3
    pub systick: SysTickState,
    pub nvic: Nvic,
    pub exti: ExtiState,
    pub afio: AfioState,
    pub flash_acr: u32,
    /// Set when the flash wait states are too low for SYSCLK (RM0008 3.3.3): on silicon the
    /// core then fetches corrupted instructions, so the simulator stops with this message.
    pub clock_error: Option<String>,
    /// Current exception number (mirrors IPSR; needed for ICSR reads).
    pub ipsr: u32,
}

impl Default for Peripherals {
    fn default() -> Self {
        Self::new(8_000_000)
    }
}

const GPIO_BASE: u32 = 0x4001_0800;

impl Peripherals {
    pub fn new(hse_hz: u32) -> Self {
        Self {
            rcc: RccState::new(hse_hz),
            gpios: [
                GpioPort::new("GPIOA"),
                GpioPort::new("GPIOB"),
                GpioPort::new("GPIOC"),
                GpioPort::new("GPIOD"),
                GpioPort::new("GPIOE"),
                GpioPort::new("GPIOF"),
                GpioPort::new("GPIOG"),
            ],
            timers: [
                TimerState::new("TIM1", IRQ_TIM1_UP as u8),
                TimerState::new("TIM2", IRQ_TIM2 as u8),
                TimerState::new("TIM3", IRQ_TIM3 as u8),
                TimerState::new("TIM4", IRQ_TIM4 as u8),
            ],
            usarts: [
                UsartState::new("USART1", IRQ_USART1 as u8),
                UsartState::new("USART2", IRQ_USART2 as u8),
                UsartState::new("USART3", IRQ_USART3 as u8),
            ],
            systick: SysTickState::default(),
            nvic: Nvic::default(),
            exti: ExtiState::default(),
            afio: AfioState::default(),
            flash_acr: 0x0000_0030,
            clock_error: None,
            ipsr: 0,
        }
    }

    pub fn vtor(&self) -> u32 {
        self.nvic.vtor
    }

    pub fn hclk_hz(&self) -> u32 {
        self.rcc.hclk_hz
    }

    pub fn flash_latency(&self) -> u32 {
        self.flash_acr & 7
    }

    fn check_clocks(&mut self) {
        let sys = self.rcc.sysclk_hz;
        let lat = self.flash_latency();
        let need = if sys > 48_000_000 {
            2
        } else if sys > 24_000_000 {
            1
        } else {
            0
        };
        self.clock_error = if lat < need {
            Some(format!(
                "SYSCLK is {} MHz but FLASH_ACR.LATENCY = {} (needs >= {}); set the flash wait states before switching to the faster clock",
                sys / 1_000_000,
                lat,
                need
            ))
        } else {
            None
        };
    }

    // ---- clock gating ----------------------------------------------------------------------

    fn gpio_clock(&self, idx: usize) -> bool {
        self.rcc.apb2enr & (1 << (2 + idx)) != 0
    }

    fn timer_clock(&self, idx: usize) -> bool {
        match idx {
            0 => self.rcc.apb2enr & (1 << 11) != 0,
            n => self.rcc.apb1enr & (1 << (n - 1)) != 0,
        }
    }

    fn usart_clock(&self, idx: usize) -> bool {
        match idx {
            0 => self.rcc.apb2enr & (1 << 14) != 0,
            n => self.rcc.apb1enr & (1 << (16 + n)) != 0,
        }
    }

    fn afio_clock(&self) -> bool {
        self.rcc.apb2enr & 1 != 0
    }

    /// HCLK cycles per timer clock: TIMxCLK = PCLKx when the APB prescaler is 1, else 2 x PCLKx.
    fn timer_hclk_div(&self, idx: usize) -> u32 {
        let ppre = if idx == 0 { self.rcc.ppre2_div() } else { self.rcc.ppre1_div() };
        if ppre == 1 {
            1
        } else {
            ppre / 2
        }
    }

    fn usart_apb_div(&self, idx: usize) -> u32 {
        if idx == 0 {
            self.rcc.ppre2_div()
        } else {
            self.rcc.ppre1_div()
        }
    }

    // ---- register access -------------------------------------------------------------------

    /// APB peripherals see byte/halfword writes replicated onto all byte lanes (RM0008 3.1).
    pub fn is_apb(addr: u32) -> bool {
        (0x4000_0000..0x4001_8000).contains(&addr)
    }

    /// Read a 32-bit register. `None` = bus error. `side_effects = false` for debugger reads.
    // Arms are ordered from specific to general; the catch-all ranges overlap on purpose.
    #[allow(clippy::match_overlapping_arm)]
    pub fn read(&mut self, addr: u32, side_effects: bool) -> Option<u32> {
        let addr = addr & !3;
        let v = match addr {
            0x4002_1000..=0x4002_13FF => self.rcc.read(addr - 0x4002_1000),
            0x4002_2000 => (self.flash_acr & 0x1F) | if self.flash_acr & 0x10 != 0 { 0x20 } else { 0 },
            0x4001_0000..=0x4001_03FF => {
                if !self.afio_clock() {
                    return Some(0);
                }
                match addr - 0x4001_0000 {
                    0x00 => self.afio.evcr,
                    0x04 => self.afio.mapr & !(7 << 24),
                    o @ 0x08..=0x14 => self.afio.exticr[((o - 8) / 4) as usize],
                    _ => 0,
                }
            }
            0x4001_0400..=0x4001_07FF => self.exti.read(addr - 0x4001_0400),
            GPIO_BASE..=0x4001_23FF => {
                let idx = ((addr - GPIO_BASE) / 0x400) as usize;
                if !self.gpio_clock(idx) {
                    return Some(0);
                }
                self.gpios[idx].read((addr - GPIO_BASE) % 0x400)
            }
            0x4001_2C00..=0x4001_2FFF => self.timer_read(0, addr - 0x4001_2C00),
            0x4000_0000..=0x4000_0BFF => {
                let idx = 1 + ((addr - 0x4000_0000) / 0x400) as usize;
                self.timer_read(idx, (addr - 0x4000_0000) % 0x400)
            }
            0x4001_3800..=0x4001_3BFF => self.usart_read(0, addr - 0x4001_3800, side_effects),
            0x4000_4400..=0x4000_47FF => self.usart_read(1, addr - 0x4000_4400, side_effects),
            0x4000_4800..=0x4000_4BFF => self.usart_read(2, addr - 0x4000_4800, side_effects),
            // Remaining STM32F1 peripheral space: present but not modelled (reads as zero)
            0x4000_0000..=0x4002_3FFF => 0,
            0xE000_E010..=0xE000_E01F => self.systick.read(addr - 0xE000_E010, side_effects),
            0xE000_E000..=0xE000_EFFF => self.nvic.read(addr - 0xE000_E000, self.ipsr),
            0xE000_1000 => self.nvic.dwt_ctrl,
            0xE000_1004 => self.nvic.dwt_cyccnt,
            // Other PPB space (ITM, DWT, FPB, TPIU, ROM table): RAZ
            0xE000_0000..=0xE00F_FFFF => 0,
            _ => return None,
        };
        Some(v)
    }

    fn timer_read(&self, idx: usize, off: u32) -> u32 {
        if self.timer_clock(idx) {
            self.timers[idx].read(off)
        } else {
            0
        }
    }

    fn usart_read(&mut self, idx: usize, off: u32, side: bool) -> u32 {
        if self.usart_clock(idx) {
            let v = self.usarts[idx].read(off, side);
            if side && off == 4 {
                self.update_irq_lines();
            }
            v
        } else {
            0
        }
    }

    /// Write a register. `mask` marks the byte lanes being written. Returns false on a bus error.
    // Arms are ordered from specific to general; the catch-all ranges overlap on purpose.
    #[allow(clippy::match_overlapping_arm)]
    pub fn write(&mut self, addr: u32, val: u32, mask: u32) -> bool {
        let addr = addr & !3;
        let merge = |old: u32| (old & !mask) | (val & mask);
        match addr {
            0x4002_1000..=0x4002_13FF => {
                let off = addr - 0x4002_1000;
                let old = self.rcc.read(off);
                let prev2 = self.rcc.apb2rstr;
                let prev1 = self.rcc.apb1rstr;
                self.rcc.write(off, merge(old));
                self.apply_resets(prev2, prev1);
                self.check_clocks();
            }
            0x4002_2000 => {
                self.flash_acr = merge(self.flash_acr) & 0x1F;
                self.check_clocks();
            }
            0x4001_0000..=0x4001_03FF => {
                if self.afio_clock() {
                    match addr - 0x4001_0000 {
                        0x00 => self.afio.evcr = merge(self.afio.evcr) & 0xFF,
                        0x04 => self.afio.mapr = merge(self.afio.mapr) & 0x071F_FFFF,
                        o @ 0x08..=0x14 => {
                            let i = ((o - 8) / 4) as usize;
                            self.afio.exticr[i] = merge(self.afio.exticr[i]) & 0xFFFF;
                        }
                        _ => {}
                    }
                }
            }
            0x4001_0400..=0x4001_07FF => {
                let off = addr - 0x4001_0400;
                let v = if off == 0x14 || off == 0x10 { val & mask } else { merge(self.exti.read(off)) };
                self.exti.write(off, v);
            }
            GPIO_BASE..=0x4001_23FF => {
                let idx = ((addr - GPIO_BASE) / 0x400) as usize;
                if self.gpio_clock(idx) {
                    let off = (addr - GPIO_BASE) % 0x400;
                    let v = if off == 0x10 || off == 0x14 { val & mask } else { merge(self.gpios[idx].read(off)) };
                    self.gpios[idx].write(off, v);
                }
            }
            0x4001_2C00..=0x4001_2FFF => self.timer_write(0, addr - 0x4001_2C00, val, mask),
            0x4000_0000..=0x4000_0BFF => {
                let idx = 1 + ((addr - 0x4000_0000) / 0x400) as usize;
                self.timer_write(idx, (addr - 0x4000_0000) % 0x400, val, mask);
            }
            0x4001_3800..=0x4001_3BFF => self.usart_write(0, addr - 0x4001_3800, val, mask),
            0x4000_4400..=0x4000_47FF => self.usart_write(1, addr - 0x4000_4400, val, mask),
            0x4000_4800..=0x4000_4BFF => self.usart_write(2, addr - 0x4000_4800, val, mask),
            0x4000_0000..=0x4002_3FFF => {}
            0xE000_E010..=0xE000_E01F => {
                let off = addr - 0xE000_E010;
                let old = self.systick.read(off, false);
                self.systick.write(off, merge(old));
            }
            0xE000_E000..=0xE000_EFFF => self.nvic.write(addr - 0xE000_E000, val, mask, self.ipsr),
            0xE000_1000 => self.nvic.dwt_ctrl = (self.nvic.dwt_ctrl & !1) | (val & mask & 1),
            0xE000_1004 => self.nvic.dwt_cyccnt = merge(self.nvic.dwt_cyccnt),
            0xE000_0000..=0xE00F_FFFF => {}
            _ => return false,
        }
        if addr < 0xE000_0000 {
            self.update_pins();
            self.update_irq_lines();
        }
        true
    }

    fn timer_write(&mut self, idx: usize, off: u32, val: u32, mask: u32) {
        if !self.timer_clock(idx) {
            return;
        }
        let t = &mut self.timers[idx];
        let v = match off {
            0x10 => val | !mask, // SR is rc_w0: unwritten lanes must not clear flags
            0x14 => val & mask,
            _ => (t.read(off) & !mask) | (val & mask),
        };
        t.write(off, v);
    }

    fn usart_write(&mut self, idx: usize, off: u32, val: u32, mask: u32) {
        if !self.usart_clock(idx) {
            return;
        }
        let apb = self.usart_apb_div(idx);
        let u = &mut self.usarts[idx];
        let v = match off {
            0x00 => val | !mask,
            0x04 => val & mask,
            _ => (u.read(off, false) & !mask) | (val & mask),
        };
        u.write(off, v, apb);
    }

    /// Peripheral reset through RCC_APBxRSTR.
    fn apply_resets(&mut self, prev2: u32, prev1: u32) {
        let new2 = self.rcc.apb2rstr & !prev2;
        let new1 = self.rcc.apb1rstr & !prev1;
        if new2 & 1 != 0 {
            self.afio = AfioState::default();
        }
        for i in 0..7 {
            if new2 & (1 << (2 + i)) != 0 {
                let (pins, mask) = (self.gpios[i].ext_pins, self.gpios[i].ext_mask);
                let name = self.gpios[i].name.clone();
                self.gpios[i] = GpioPort::new(&name);
                self.gpios[i].ext_pins = pins;
                self.gpios[i].ext_mask = mask;
            }
        }
        if new2 & (1 << 11) != 0 {
            self.timers[0] = TimerState::new("TIM1", IRQ_TIM1_UP as u8);
        }
        if new2 & (1 << 14) != 0 {
            let tx = std::mem::take(&mut self.usarts[0].tx_buffer);
            self.usarts[0] = UsartState::new("USART1", IRQ_USART1 as u8);
            self.usarts[0].tx_buffer = tx;
        }
        for (i, name) in [(1usize, "TIM2"), (2, "TIM3"), (3, "TIM4")] {
            if new1 & (1 << (i - 1)) != 0 {
                self.timers[i] = TimerState::new(name, (IRQ_TIM2 + i - 1) as u8);
            }
        }
        for (i, name) in [(1usize, "USART2"), (2, "USART3")] {
            if new1 & (1 << (16 + i)) != 0 {
                let tx = std::mem::take(&mut self.usarts[i].tx_buffer);
                self.usarts[i] = UsartState::new(name, (IRQ_USART1 + i) as u8);
                self.usarts[i].tx_buffer = tx;
            }
        }
    }

    /// Convenience 32-bit accessors (tests, legacy callers).
    pub fn read32(&mut self, addr: u32) -> u32 {
        self.read(addr, true).unwrap_or(0)
    }

    pub fn write32(&mut self, addr: u32, val: u32) {
        self.write(addr, val, 0xFFFF_FFFF);
    }

    // ---- pins ------------------------------------------------------------------------------

    /// Timer channel -> (port index, pin), honouring the AFIO_MAPR remaps.
    fn timer_pin(&self, tim: usize, ch: usize) -> (usize, u32) {
        let mapr = self.afio.mapr;
        match tim {
            0 => match (mapr >> 6) & 3 {
                3 => (4, [9, 11, 13, 14][ch]),
                _ => (0, [8, 9, 10, 11][ch]),
            },
            1 => {
                let r = (mapr >> 8) & 3;
                match ch {
                    0 if r & 1 != 0 => (0, 15),
                    0 => (0, 0),
                    1 if r & 1 != 0 => (1, 3),
                    1 => (0, 1),
                    2 if r & 2 != 0 => (1, 10),
                    2 => (0, 2),
                    _ if r & 2 != 0 => (1, 11),
                    _ => (0, 3),
                }
            }
            2 => match (mapr >> 10) & 3 {
                2 => [(1, 4), (1, 5), (1, 0), (1, 1)][ch],
                3 => (2, 6 + ch as u32),
                _ => [(0, 6), (0, 7), (1, 0), (1, 1)][ch],
            },
            _ => {
                if mapr & (1 << 12) != 0 {
                    (3, 12 + ch as u32)
                } else {
                    (1, 6 + ch as u32)
                }
            }
        }
    }

    /// Recompute AF outputs and pin levels, then run EXTI edge detection.
    pub fn update_pins(&mut self) {
        for g in &mut self.gpios {
            g.af_mask = 0;
            g.af_val = 0;
        }
        for tim in 0..4 {
            if !self.timer_clock(tim) {
                continue;
            }
            for ch in 0..4 {
                if let Some(level) = self.timers[tim].channel_output(ch) {
                    let (port, pin) = self.timer_pin(tim, ch);
                    let g = &mut self.gpios[port];
                    g.af_mask |= 1 << pin;
                    if level {
                        g.af_val |= 1 << pin;
                    }
                }
            }
        }
        // USART TX lines idle high while the transmitter is enabled
        for (u, port, pin) in [(0usize, 0usize, 9u32), (1, 0, 2), (2, 1, 10)] {
            let cr1 = self.usarts[u].cr1;
            if self.usart_clock(u) && cr1 & (1 << 13) != 0 && cr1 & (1 << 3) != 0 {
                let g = &mut self.gpios[port];
                g.af_mask |= 1 << pin;
                g.af_val |= 1 << pin;
            }
        }
        for g in &mut self.gpios {
            g.refresh_idr();
        }
        // EXTI line x follows the pin selected by AFIO_EXTICR
        let mut lines = 0u16;
        for line in 0..16usize {
            let port = ((self.afio.exticr[line / 4] >> ((line % 4) * 4)) & 0xF) as usize;
            if port < 7 && self.gpios[port].pins & (1 << line) != 0 {
                lines |= 1 << line;
            }
        }
        let prev = self.exti.lines;
        let rising = (lines & !prev) as u32 & self.exti.rtsr;
        let falling = (!lines & prev) as u32 & self.exti.ftsr;
        self.exti.pr |= (rising | falling) & self.exti.imr;
        self.exti.lines = lines;
    }

    /// Drive an input pin from outside the chip (button, keypad, test harness).
    pub fn set_input_pin(&mut self, port: usize, pin: u8, high: bool) {
        if let Some(g) = self.gpios.get_mut(port) {
            g.set_ext_pin(pin, high);
        }
        self.update_pins();
        self.update_irq_lines();
    }

    pub fn update_irq_lines(&mut self) {
        let pr = self.exti.pr & self.exti.imr;
        for i in 0..5 {
            self.nvic.set_line(IRQ_EXTI0 + i, pr & (1 << i) != 0);
        }
        self.nvic.set_line(IRQ_EXTI9_5, pr & 0x3E0 != 0);
        self.nvic.set_line(IRQ_EXTI15_10, pr & 0xFC00 != 0);
        let t1 = &self.timers[0];
        let (up, cc, brk) = (t1.sr & t1.dier & 1 != 0, t1.sr & t1.dier & 0x1E != 0, t1.sr & t1.dier & (1 << 7) != 0);
        self.nvic.set_line(IRQ_TIM1_UP, up);
        self.nvic.set_line(IRQ_TIM1_CC, cc);
        self.nvic.set_line(IRQ_TIM1_BRK, brk);
        for i in 1..4 {
            let lvl = self.timers[i].irq_level();
            self.nvic.set_line(IRQ_TIM2 + i - 1, lvl);
        }
        for i in 0..3 {
            let lvl = self.usarts[i].irq_level();
            self.nvic.set_line(IRQ_USART1 + i, lvl);
        }
    }

    // ---- time ------------------------------------------------------------------------------

    /// Advance all peripherals by `cycles` HCLK cycles.
    pub fn tick(&mut self, cycles: u64) {
        if cycles == 0 {
            return;
        }
        if self.nvic.dwt_ctrl & 1 != 0 && self.nvic.demcr & (1 << 24) != 0 {
            self.nvic.dwt_cyccnt = self.nvic.dwt_cyccnt.wrapping_add(cycles as u32);
        }
        if self.systick.advance(cycles) && self.systick.ctrl & 2 != 0 {
            self.nvic.set_pending(EXC_SYSTICK);
        }
        let mut pins_dirty = false;
        for i in 0..4 {
            if !self.timer_clock(i) || self.timers[i].cr1 & 1 == 0 {
                continue;
            }
            let div = self.timer_hclk_div(i) as u64;
            let t = &mut self.timers[i];
            let total = t.hclk_acc as u64 + cycles;
            t.hclk_acc = (total % div) as u32;
            let before = t.oc_ref;
            t.advance(total / div);
            if t.oc_ref != before {
                pins_dirty = true;
            }
        }
        for i in 0..3 {
            if self.usart_clock(i) && self.usarts[i].cr1 & (1 << 13) != 0 {
                let apb = self.usart_apb_div(i);
                self.usarts[i].advance(cycles, apb);
            }
        }
        if pins_dirty {
            self.update_pins();
        }
        self.update_irq_lines();
    }

    /// HCLK cycles until the next peripheral event (used to fast-forward WFI sleep).
    pub fn cycles_to_next_event(&self) -> Option<u64> {
        let mut best: Option<u64> = self.systick.cycles_to_event();
        let mut consider = |c: u64| best = Some(best.map_or(c, |b: u64| b.min(c)));
        for i in 0..4 {
            if self.timer_clock(i) {
                if let Some(c) = self.timers[i].clocks_to_event() {
                    consider(c * self.timer_hclk_div(i) as u64);
                }
            }
        }
        for i in 0..3 {
            if self.usart_clock(i) {
                if let Some(c) = self.usarts[i].cycles_to_event(self.usart_apb_div(i)) {
                    consider(c);
                }
            }
        }
        best
    }

    /// Could an interrupt still arrive (decides whether a `B .` loop is final)?
    pub fn interrupt_armed(&self) -> bool {
        (self.systick.ctrl & 3) == 3 || self.nvic.any_irq_enabled() || self.nvic.pending.iter().any(|&p| p)
    }

    // ---- debugger helpers ------------------------------------------------------------------

    pub fn eval_signal_field(&self, signal: &str) -> Option<u64> {
        let s = signal.trim().to_ascii_uppercase();
        let (port_name, field) = s.split_once('.')?;
        if let Some(gpio) = self.gpios.iter().find(|g| g.name == port_name) {
            return match field {
                "ODR" => Some(gpio.odr as u64),
                "IDR" => Some(gpio.idr as u64),
                "CRL" => Some(gpio.crl as u64),
                "CRH" => Some(gpio.crh as u64),
                "PINS" => Some(gpio.pins as u64),
                _ => match field.parse::<u8>() {
                    Ok(pin) if pin < 16 => Some(((gpio.pins >> pin) & 1) as u64),
                    _ => None,
                },
            };
        }
        if let Some(tim) = self.timers.iter().find(|t| t.name == port_name) {
            return match field {
                "CNT" => Some(tim.cnt as u64),
                "ARR" => Some(tim.arr as u64),
                "PSC" => Some(tim.psc as u64),
                "SR" => Some(tim.sr as u64),
                "CR1" => Some(tim.cr1 as u64),
                "CCR1" => Some(tim.ccr[0] as u64),
                "CCR2" => Some(tim.ccr[1] as u64),
                "CCR3" => Some(tim.ccr[2] as u64),
                "CCR4" => Some(tim.ccr[3] as u64),
                _ => None,
            };
        }
        match (port_name, field) {
            ("EXTI", "PR") => Some(self.exti.pr as u64),
            ("EXTI", "IMR") => Some(self.exti.imr as u64),
            ("SYSTICK", "VAL") => Some(self.systick.val as u64),
            ("SYSTICK", "LOAD") => Some(self.systick.load as u64),
            ("SYSTICK", "CTRL") => Some(self.systick.ctrl as u64),
            _ => None,
        }
    }

    pub fn snapshot(&self) -> PeripheralSnapshot {
        let mut gpios = self.gpios.to_vec();
        for g in &mut gpios {
            g.refresh_idr();
        }
        let words = |v: &[bool]| -> Vec<u32> {
            v[16..]
                .chunks(32)
                .map(|c| c.iter().enumerate().fold(0u32, |w, (i, &b)| w | ((b as u32) << i)))
                .collect()
        };
        PeripheralSnapshot {
            rcc: self.rcc.clone(),
            gpios,
            timers: self.timers.to_vec(),
            usarts: self.usarts.to_vec(),
            systick: self.systick.clone(),
            vtor: self.nvic.vtor,
            uart1_text: String::from_utf8_lossy(&self.usarts[0].tx_buffer).into_owned(),
            exti: self.exti.clone(),
            afio: self.afio.clone(),
            flash_acr: self.flash_acr,
            nvic_enabled: words(&self.nvic.enabled),
            nvic_pending: words(&self.nvic.pending),
            nvic_active: words(&self.nvic.active),
        }
    }
}
