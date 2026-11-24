use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

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
}

impl Default for RccState {
    fn default() -> Self {
        Self {
            cr: 0x0303_0083, // HSIRDY | HSERDY | PLLRDY | HSION
            cfgr: 0x0000_0000,
            cir: 0,
            apb2rstr: 0,
            apb1rstr: 0,
            ahbenr: 0x0000_0014,
            apb2enr: 0,
            apb1enr: 0,
            bdcr: 0,
            csr: 0x0C00_0000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpioPort {
    pub name: String,
    pub crl: u32,
    pub crh: u32,
    pub idr: u32,
    pub odr: u32,
    pub lckr: u32,
    pub ext_pins: u16,
    pub ext_mask: u16,
}

impl GpioPort {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            crl: 0x4444_4444,
            crh: 0x4444_4444,
            idr: 0,
            odr: 0,
            lckr: 0,
            ext_pins: 0,
            ext_mask: 0,
        }
    }

    pub fn refresh_idr(&mut self) {
        let mut out_mask: u16 = 0;
        for pin in 0..8 {
            let mode = (self.crl >> (pin * 4)) & 0x3;
            if mode != 0 {
                out_mask |= 1 << pin;
            }
        }
        for pin in 0..8 {
            let mode = (self.crh >> (pin * 4)) & 0x3;
            if mode != 0 {
                out_mask |= 1 << (pin + 8);
            }
        }
        let odr16 = (self.odr & 0xFFFF) as u16;
        // Input pins take external stimulus; output pins reflect ODR unless overridden by ext_mask
        let driven = (odr16 & out_mask) | (self.ext_pins & !out_mask);
        let merged = (driven & !self.ext_mask) | (self.ext_pins & self.ext_mask);
        self.idr = merged as u32;
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

    pub fn read(&mut self, offset: u32) -> u32 {
        self.refresh_idr();
        match offset {
            0x00 => self.crl,
            0x04 => self.crh,
            0x08 => self.idr,
            0x0C => self.odr,
            0x10 | 0x14 => 0,
            0x18 => self.lckr,
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: u32, val: u32) {
        match offset {
            0x00 => self.crl = val,
            0x04 => self.crh = val,
            0x0C => self.odr = val & 0xFFFF,
            0x10 => {
                let set_bits = val & 0xFFFF;
                let reset_bits = (val >> 16) & 0xFFFF;
                self.odr = (self.odr & !reset_bits) | set_bits;
            }
            0x14 => {
                let reset_bits = val & 0xFFFF;
                self.odr &= !reset_bits;
            }
            0x18 => self.lckr = val,
            _ => {}
        }
        self.refresh_idr();
    }
}

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
    pub psc_counter: u32,
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
        }
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
            0x30 => self.rcr,
            0x34 => self.ccr[0],
            0x38 => self.ccr[1],
            0x3C => self.ccr[2],
            0x40 => self.ccr[3],
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: u32, val: u32) {
        match offset {
            0x00 => self.cr1 = val,
            0x04 => self.cr2 = val,
            0x08 => self.smcr = val,
            0x0C => self.dier = val,
            0x10 => self.sr &= val, // rc_w0
            0x14 => {
                // EGR
                if (val & 1) != 0 {
                    self.cnt = 0;
                    self.psc_counter = 0;
                    self.sr |= 1;
                }
            }
            0x18 => self.ccmr1 = val,
            0x1C => self.ccmr2 = val,
            0x20 => self.ccer = val,
            0x24 => self.cnt = val & 0xFFFF,
            0x28 => self.psc = val & 0xFFFF,
            0x2C => self.arr = val & 0xFFFF,
            0x30 => self.rcr = val & 0xFF,
            0x34 => self.ccr[0] = val & 0xFFFF,
            0x38 => self.ccr[1] = val & 0xFFFF,
            0x3C => self.ccr[2] = val & 0xFFFF,
            0x40 => self.ccr[3] = val & 0xFFFF,
            _ => {}
        }
    }

    pub fn tick(&mut self, cycles: u32) -> bool {
        if (self.cr1 & 1) == 0 {
            return false;
        }
        let mut irq = false;
        self.psc_counter += cycles;
        let div = self.psc + 1;
        while self.psc_counter >= div {
            self.psc_counter -= div;
            self.cnt += 1;
            if self.arr > 0 && self.cnt >= self.arr {
                self.cnt = 0;
                self.sr |= 1; // UIF
                if (self.dier & 1) != 0 {
                    irq = true;
                }
                if (self.cr1 & (1 << 3)) != 0 {
                    // OPM
                    self.cr1 &= !1;
                    break;
                }
            } else if self.cnt > 0xFFFF {
                self.cnt = 0;
                self.sr |= 1;
                if (self.dier & 1) != 0 {
                    irq = true;
                }
            }
        }
        irq
    }
}

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
}

impl UsartState {
    pub fn new(name: &str, irq_num: u8) -> Self {
        Self {
            name: name.to_string(),
            irq_num,
            sr: 0x00C0, // TXE | TC set by default
            brr: 0,
            cr1: 0,
            cr2: 0,
            cr3: 0,
            gtpr: 0,
            tx_buffer: Vec::new(),
            rx_queue: VecDeque::new(),
        }
    }

    pub fn push_rx_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.rx_queue.push_back(b);
        }
        if !self.rx_queue.is_empty() {
            self.sr |= 1 << 5; // RXNE
        }
    }

    pub fn read(&mut self, offset: u32) -> u32 {
        match offset {
            0x00 => {
                if !self.rx_queue.is_empty() {
                    self.sr |= 1 << 5;
                } else {
                    self.sr &= !(1 << 5);
                }
                self.sr
            }
            0x04 => {
                if let Some(b) = self.rx_queue.pop_front() {
                    if self.rx_queue.is_empty() {
                        self.sr &= !(1 << 5);
                    }
                    b as u32
                } else {
                    0
                }
            }
            0x08 => self.brr,
            0x0C => self.cr1,
            0x10 => self.cr2,
            0x14 => self.cr3,
            0x18 => self.gtpr,
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: u32, val: u32) {
        match offset {
            0x00 => self.sr &= val,
            0x04 => {
                let ch = (val & 0xFF) as u8;
                self.tx_buffer.push(ch);
                self.sr |= 0x00C0; // TXE | TC remain ready
            }
            0x08 => self.brr = val,
            0x0C => self.cr1 = val,
            0x10 => self.cr2 = val,
            0x14 => self.cr3 = val,
            0x18 => self.gtpr = val,
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SysTickState {
    pub ctrl: u32,
    pub load: u32,
    pub val: u32,
    pub calib: u32,
}

impl Default for SysTickState {
    fn default() -> Self {
        Self {
            ctrl: 0x0000_0004, // CLKSOURCE=processor
            load: 0,
            val: 0,
            calib: 0x0000_2328,
        }
    }
}

impl SysTickState {
    pub fn read(&mut self, offset: u32) -> u32 {
        match offset {
            0x00 => {
                let v = self.ctrl;
                self.ctrl &= !(1 << 16); // COUNTFLAG clears on read
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
            0x00 => self.ctrl = (self.ctrl & (1 << 16)) | (val & 0x0001_0007),
            0x04 => self.load = val & 0x00FF_FFFF,
            0x08 => {
                self.val = 0;
                self.ctrl &= !(1 << 16);
            }
            _ => {}
        }
    }

    pub fn tick(&mut self, cycles: u32) -> bool {
        if (self.ctrl & 1) == 0 || self.load == 0 {
            return false;
        }
        let mut irq = false;
        let dec = cycles;
        if self.val == 0 {
            self.val = self.load;
        } else if dec >= self.val {
            self.ctrl |= 1 << 16; // COUNTFLAG
            if (self.ctrl & 2) != 0 {
                irq = true;
            }
            self.val = self.load;
        } else {
            self.val -= dec;
        }
        irq
    }
}

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
            self.channels.push(LogicChannel {
                signal: signal.to_string(),
                samples: Vec::new(),
                last_value: None,
            });
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeripheralSnapshot {
    pub rcc: RccState,
    pub gpios: Vec<GpioPort>,
    pub timers: Vec<TimerState>,
    pub usarts: Vec<UsartState>,
    pub systick: SysTickState,
    pub vtor: u32,
    pub uart1_text: String,
}

#[derive(Debug, Clone)]
pub struct Peripherals {
    pub rcc: RccState,
    pub gpios: [GpioPort; 7],
    pub timers: [TimerState; 4], // TIM1, TIM2, TIM3, TIM4
    pub usarts: [UsartState; 3], // USART1, USART2, USART3
    pub systick: SysTickState,
    pub nvic_iser: [u32; 8],
    pub nvic_ispr: [u32; 8],
    pub vtor: u32,
    pub afio_evcr: u32,
    pub afio_mapr: u32,
    pub flash_acr: u32,
}

impl Default for Peripherals {
    fn default() -> Self {
        Self {
            rcc: RccState::default(),
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
                TimerState::new("TIM1", 25),
                TimerState::new("TIM2", 28),
                TimerState::new("TIM3", 29),
                TimerState::new("TIM4", 30),
            ],
            usarts: [
                UsartState::new("USART1", 37),
                UsartState::new("USART2", 38),
                UsartState::new("USART3", 39),
            ],
            systick: SysTickState::default(),
            nvic_iser: [0; 8],
            nvic_ispr: [0; 8],
            vtor: 0x0800_0000,
            afio_evcr: 0,
            afio_mapr: 0,
            flash_acr: 0x0000_0030,
        }
    }
}

impl Peripherals {
    pub fn read32(&mut self, addr: u32) -> u32 {
        match addr {
            0x4002_1000..=0x4002_1028 => match addr - 0x4002_1000 {
                0x00 => self.rcc.cr,
                0x04 => self.rcc.cfgr,
                0x08 => self.rcc.cir,
                0x0C => self.rcc.apb2rstr,
                0x10 => self.rcc.apb1rstr,
                0x14 => self.rcc.ahbenr,
                0x18 => self.rcc.apb2enr,
                0x1C => self.rcc.apb1enr,
                0x20 => self.rcc.bdcr,
                0x24 => self.rcc.csr,
                _ => 0,
            },
            0x4002_2000..=0x4002_20FF => {
                if addr == 0x4002_2000 { self.flash_acr } else { 0 }
            }
            0x4001_0000..=0x4001_03FF => match addr - 0x4001_0000 {
                0x00 => self.afio_evcr,
                0x04 => self.afio_mapr,
                _ => 0,
            },
            0x4001_0800..=0x4001_23FF => {
                let idx = ((addr - 0x4001_0800) / 0x400) as usize;
                let off = (addr - 0x4001_0800) % 0x400;
                self.gpios.get_mut(idx).map(|g| g.read(off)).unwrap_or(0)
            }
            0x4001_2C00..=0x4001_2FFF => self.timers[0].read(addr - 0x4001_2C00),
            0x4000_0000..=0x4000_03FF => self.timers[1].read(addr - 0x4000_0000),
            0x4000_0400..=0x4000_07FF => self.timers[2].read(addr - 0x4000_0400),
            0x4000_0800..=0x4000_0BFF => self.timers[3].read(addr - 0x4000_0800),
            0x4001_3800..=0x4001_3BFF => self.usarts[0].read(addr - 0x4001_3800),
            0x4000_4400..=0x4000_47FF => self.usarts[1].read(addr - 0x4000_4400),
            0x4000_4800..=0x4000_4BFF => self.usarts[2].read(addr - 0x4000_4800),
            0xE000_E010..=0xE000_E01F => self.systick.read(addr - 0xE000_E010),
            0xE000_E100..=0xE000_E11F => {
                let idx = ((addr - 0xE000_E100) / 4) as usize;
                self.nvic_iser.get(idx).copied().unwrap_or(0)
            }
            0xE000_E180..=0xE000_E19F => {
                let idx = ((addr - 0xE000_E180) / 4) as usize;
                self.nvic_iser.get(idx).copied().unwrap_or(0)
            }
            0xE000_E200..=0xE000_E21F => {
                let idx = ((addr - 0xE000_E200) / 4) as usize;
                self.nvic_ispr.get(idx).copied().unwrap_or(0)
            }
            0xE000_ED00 => 0x411FC231, // CPUID Cortex-M3 r1p1
            0xE000_ED08 => self.vtor,
            _ => 0,
        }
    }

    pub fn write32(&mut self, addr: u32, val: u32) {
        match addr {
            0x4002_1000..=0x4002_1028 => match addr - 0x4002_1000 {
                0x00 => {
                    // Keep HSIRDY/HSERDY/PLLRDY locked when enabled
                    let mut v = val;
                    if (v & (1 << 0)) != 0 { v |= 1 << 1; }
                    if (v & (1 << 16)) != 0 { v |= 1 << 17; }
                    if (v & (1 << 24)) != 0 { v |= 1 << 25; }
                    self.rcc.cr = v;
                }
                0x04 => {
                    let sw = val & 0x3;
                    self.rcc.cfgr = (val & !0xC) | (sw << 2);
                }
                0x08 => self.rcc.cir = val,
                0x0C => self.rcc.apb2rstr = val,
                0x10 => self.rcc.apb1rstr = val,
                0x14 => self.rcc.ahbenr = val,
                0x18 => self.rcc.apb2enr = val,
                0x1C => self.rcc.apb1enr = val,
                0x20 => self.rcc.bdcr = val,
                0x24 => self.rcc.csr = val,
                _ => {}
            },
            0x4002_2000 => self.flash_acr = val,
            0x4001_0000..=0x4001_03FF => match addr - 0x4001_0000 {
                0x00 => self.afio_evcr = val,
                0x04 => self.afio_mapr = val,
                _ => {}
            },
            0x4001_0800..=0x4001_23FF => {
                let idx = ((addr - 0x4001_0800) / 0x400) as usize;
                let off = (addr - 0x4001_0800) % 0x400;
                if let Some(g) = self.gpios.get_mut(idx) {
                    g.write(off, val);
                }
            }
            0x4001_2C00..=0x4001_2FFF => self.timers[0].write(addr - 0x4001_2C00, val),
            0x4000_0000..=0x4000_03FF => self.timers[1].write(addr - 0x4000_0000, val),
            0x4000_0400..=0x4000_07FF => self.timers[2].write(addr - 0x4000_0400, val),
            0x4000_0800..=0x4000_0BFF => self.timers[3].write(addr - 0x4000_0800, val),
            0x4001_3800..=0x4001_3BFF => self.usarts[0].write(addr - 0x4001_3800, val),
            0x4000_4400..=0x4000_47FF => self.usarts[1].write(addr - 0x4000_4400, val),
            0x4000_4800..=0x4000_4BFF => self.usarts[2].write(addr - 0x4000_4800, val),
            0xE000_E010..=0xE000_E01F => self.systick.write(addr - 0xE000_E010, val),
            0xE000_E100..=0xE000_E11F => {
                let idx = ((addr - 0xE000_E100) / 4) as usize;
                if let Some(slot) = self.nvic_iser.get_mut(idx) {
                    *slot |= val;
                }
            }
            0xE000_E180..=0xE000_E19F => {
                let idx = ((addr - 0xE000_E180) / 4) as usize;
                if let Some(slot) = self.nvic_iser.get_mut(idx) {
                    *slot &= !val;
                }
            }
            0xE000_E200..=0xE000_E21F => {
                let idx = ((addr - 0xE000_E200) / 4) as usize;
                if let Some(slot) = self.nvic_ispr.get_mut(idx) {
                    *slot |= val;
                }
            }
            0xE000_E280..=0xE000_E29F => {
                let idx = ((addr - 0xE000_E280) / 4) as usize;
                if let Some(slot) = self.nvic_ispr.get_mut(idx) {
                    *slot &= !val;
                }
            }
            0xE000_ED08 => self.vtor = val & !0x7F,
            _ => {}
        }
    }

    pub fn tick(&mut self, cycles: u32) -> Vec<u16> {
        let mut pending = Vec::new();
        if self.systick.tick(cycles) {
            pending.push(15); // SysTick exception
        }
        for tim in &mut self.timers {
            if tim.tick(cycles) {
                let irq = tim.irq_num as usize;
                let word = irq / 32;
                let bit = 1u32 << (irq % 32);
                self.nvic_ispr[word] |= bit;
                if (self.nvic_iser[word] & bit) != 0 {
                    pending.push(16 + tim.irq_num as u16);
                }
            }
        }
        pending
    }

    pub fn eval_signal_field(&self, signal: &str) -> Option<u64> {
        let s = signal.trim().to_ascii_uppercase();
        if let Some((port_name, field)) = s.split_once('.') {
            if let Some(gpio) = self.gpios.iter().find(|g| g.name == port_name) {
                return match field {
                    "ODR" => Some(gpio.odr as u64),
                    "IDR" => Some(gpio.idr as u64),
                    "CRL" => Some(gpio.crl as u64),
                    "CRH" => Some(gpio.crh as u64),
                    _ => {
                        if let Ok(pin) = field.parse::<u8>() {
                            if pin < 16 {
                                return Some(((gpio.odr >> pin) & 1) as u64);
                            }
                        }
                        None
                    }
                };
            }
            if let Some(tim) = self.timers.iter().find(|t| t.name == port_name) {
                return match field {
                    "CNT" => Some(tim.cnt as u64),
                    "ARR" => Some(tim.arr as u64),
                    "PSC" => Some(tim.psc as u64),
                    "SR" => Some(tim.sr as u64),
                    _ => None,
                };
            }
        }
        None
    }

    pub fn snapshot(&self) -> PeripheralSnapshot {
        let mut gpios = self.gpios.to_vec();
        for g in &mut gpios {
            g.refresh_idr();
        }
        let uart1_text = String::from_utf8_lossy(&self.usarts[0].tx_buffer).into_owned();
        PeripheralSnapshot {
            rcc: self.rcc.clone(),
            gpios,
            timers: self.timers.to_vec(),
            usarts: self.usarts.to_vec(),
            systick: self.systick.clone(),
            vtor: self.vtor,
            uart1_text,
        }
    }
}
