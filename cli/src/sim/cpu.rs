//! Cortex-M3 (ARMv7-M) core: Thumb/Thumb-2 execution on top of [`crate::thumb`], the
//! exception model (stacking with STKALIGN, EXC_RETURN, tail-chaining, priority
//! arbitration with PRIMASK/FAULTMASK/BASEPRI), synchronous faults with escalation to
//! HardFault and lockup, sleep (WFI/WFE) and an approximate cycle model (Cortex-M3 TRM
//! chapter 18 plus flash wait states).

use crate::sim::memory::{MemError, Memory};
use crate::sim::nvic::*;
use crate::thumb::{
    self, Addr, Alu, Barrier, ExtKind, HintKind, Instr, LongMul, RevKind, SRType, Size,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HaltReason {
    Running,
    Breakpoint(u32),
    Bkpt(u8),
    SelfLoop(u32),
    MaxSteps,
    Fault(String),
}

/// A synchronous fault detected while executing one instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fault {
    Undef,
    InvState,
    InvPc,
    NoCp,
    Unaligned,
    DivByZero,
    /// Precise data bus error at the given address.
    Bus(u32),
    /// Instruction fetch bus error.
    IBus,
    /// Instruction fetch from an execute-never region.
    Iaccviol,
}

#[derive(Debug, Clone)]
pub struct Cpu {
    pub r: [u32; 16], // 13 = current SP, 14 = LR, 15 = PC
    pub msp: u32,
    pub psp: u32,
    pub xpsr: u32,
    pub primask: u32,
    pub control: u32,
    pub basepri: u32,
    pub faultmask: u32,
    pub cycles: u64,
    /// Current HCLK frequency (follows the RCC configuration).
    pub clock_hz: u32,
    /// Simulated time, integrated over the actual HCLK of every step.
    pub time_sec: f64,
    pub halted_reason: HaltReason,
    pub sleeping: bool,
    pub event_reg: bool,
    pub locked_up: bool,
    /// Stop the run loop when a fault exception is taken (the debugger shows the fault).
    pub halt_on_fault: bool,
    /// Description of the most recent fault exception.
    pub last_fault: Option<String>,
    excl: Option<u32>,
    cur_pc: u32,
    /// Extra cycles (exception return / tail-chain) charged to the current step.
    extra: u32,
}

impl Default for Cpu {
    fn default() -> Self {
        Self {
            r: [0; 16],
            msp: 0x2000_1000,
            psp: 0,
            xpsr: 0x0100_0000,
            primask: 0,
            control: 0,
            basepri: 0,
            faultmask: 0,
            cycles: 0,
            clock_hz: 8_000_000,
            time_sec: 0.0,
            halted_reason: HaltReason::Running,
            sleeping: false,
            event_reg: false,
            locked_up: false,
            halt_on_fault: true,
            last_fault: None,
            excl: None,
            cur_pc: 0,
            extra: 0,
        }
    }
}

#[inline]
fn bit(v: u32, n: u32) -> bool {
    (v >> n) & 1 != 0
}

fn add_with_carry(x: u32, y: u32, carry_in: bool) -> (u32, bool, bool) {
    let sum = x as u64 + y as u64 + carry_in as u64;
    let res = sum as u32;
    let c = sum > 0xFFFF_FFFF;
    let v = ((x ^ res) & (y ^ res)) >> 31 != 0;
    (res, c, v)
}

/// Shift_C(value, type, amount, carry_in).
pub fn shift_c(v: u32, t: SRType, n: u32, c: bool) -> (u32, bool) {
    if n == 0 && t != SRType::Rrx {
        return (v, c);
    }
    match t {
        SRType::Lsl => match n {
            1..=31 => (v << n, bit(v, 32 - n)),
            32 => (0, v & 1 != 0),
            _ => (0, false),
        },
        SRType::Lsr => match n {
            1..=31 => (v >> n, bit(v, n - 1)),
            32 => (0, bit(v, 31)),
            _ => (0, false),
        },
        SRType::Asr => match n {
            1..=31 => (((v as i32) >> n) as u32, bit(v, n - 1)),
            _ => (((v as i32) >> 31) as u32, bit(v, 31)),
        },
        SRType::Ror => {
            let m = n % 32;
            let r = v.rotate_right(m);
            (r, bit(r, 31))
        }
        SRType::Rrx => (((c as u32) << 31) | (v >> 1), v & 1 != 0),
    }
}

fn describe_fault(f: Fault) -> &'static str {
    match f {
        Fault::Undef => "UsageFault UNDEFINSTR (undefined or unpredictable instruction)",
        Fault::InvState => "UsageFault INVSTATE (branch to an address with bit 0 clear / not Thumb state)",
        Fault::InvPc => "UsageFault INVPC (invalid EXC_RETURN or exception return)",
        Fault::NoCp => "UsageFault NOCP (coprocessor instruction; Cortex-M3 has no coprocessors)",
        Fault::Unaligned => "UsageFault UNALIGNED (unaligned LDM/STM/LDRD/STRD/LDREX/STREX or UNALIGN_TRP)",
        Fault::DivByZero => "UsageFault DIVBYZERO",
        Fault::Bus(_) => "BusFault PRECISERR (data access to an invalid address or a read-only region)",
        Fault::IBus => "BusFault IBUSERR (instruction fetch from an invalid address)",
        Fault::Iaccviol => "MemManage IACCVIOL (instruction fetch from an execute-never region)",
    }
}

impl Cpu {
    // ---- reset / state helpers -------------------------------------------------------------

    pub fn reset(&mut self, mem: &mut Memory, _entry_fallback: Option<u32>) {
        let keep_halt_on_fault = self.halt_on_fault;
        *self = Cpu { halt_on_fault: keep_halt_on_fault, ..Cpu::default() };
        self.r[14] = 0xFFFF_FFFF;
        let vtor = mem.peripherals.vtor();
        let sp = mem.read(vtor, 4, true);
        let pc = mem.read(vtor.wrapping_add(4), 4, true);
        self.clock_hz = mem.peripherals.hclk_hz();
        match (sp, pc) {
            (Ok(sp), Ok(pc)) if pc != 0xFFFF_FFFF && sp != 0xFFFF_FFFF => {
                self.msp = sp & !3;
                self.r[13] = self.msp;
                self.r[15] = pc & !1;
                self.xpsr = if pc & 1 != 0 { 1 << 24 } else { 0 };
            }
            _ => {
                self.locked_up = true;
                self.halted_reason = HaltReason::Fault(format!(
                    "Lockup: no valid vector table at 0x{:08X} (initial SP / Reset_Handler missing)",
                    vtor
                ));
            }
        }
    }

    #[inline]
    pub fn flag_n(&self) -> bool {
        bit(self.xpsr, 31)
    }
    #[inline]
    pub fn flag_z(&self) -> bool {
        bit(self.xpsr, 30)
    }
    #[inline]
    pub fn flag_c(&self) -> bool {
        bit(self.xpsr, 29)
    }
    #[inline]
    pub fn flag_v(&self) -> bool {
        bit(self.xpsr, 28)
    }
    #[inline]
    pub fn flag_q(&self) -> bool {
        bit(self.xpsr, 27)
    }

    fn set_nz(&mut self, res: u32) {
        self.xpsr = (self.xpsr & !(3 << 30)) | (res & 0x8000_0000) | if res == 0 { 1 << 30 } else { 0 };
    }

    fn set_c(&mut self, c: bool) {
        self.xpsr = (self.xpsr & !(1 << 29)) | ((c as u32) << 29);
    }

    fn set_v(&mut self, v: bool) {
        self.xpsr = (self.xpsr & !(1 << 28)) | ((v as u32) << 28);
    }

    pub fn ipsr(&self) -> u32 {
        self.xpsr & 0x1FF
    }

    pub fn handler_mode(&self) -> bool {
        self.ipsr() != 0
    }

    pub fn privileged(&self) -> bool {
        self.handler_mode() || self.control & 1 == 0
    }

    fn itstate(&self) -> u8 {
        ((((self.xpsr >> 10) & 0x3F) << 2) | ((self.xpsr >> 25) & 3)) as u8
    }

    fn set_itstate(&mut self, it: u8) {
        let it = it as u32;
        self.xpsr = (self.xpsr & !((0x3F << 10) | (3 << 25))) | (((it >> 2) & 0x3F) << 10) | ((it & 3) << 25);
    }

    pub fn cond_passed(&self, cond: u8) -> bool {
        let (n, z, c, v) = (self.flag_n(), self.flag_z(), self.flag_c(), self.flag_v());
        match cond & 0xF {
            0x0 => z,
            0x1 => !z,
            0x2 => c,
            0x3 => !c,
            0x4 => n,
            0x5 => !n,
            0x6 => v,
            0x7 => !v,
            0x8 => c && !z,
            0x9 => !c || z,
            0xA => n == v,
            0xB => n != v,
            0xC => !z && n == v,
            0xD => z || n != v,
            _ => true,
        }
    }

    fn using_psp(&self) -> bool {
        !self.handler_mode() && self.control & 2 != 0
    }

    /// Save the current SP into its bank.
    fn bank_sp(&mut self) {
        if self.using_psp() {
            self.psp = self.r[13];
        } else {
            self.msp = self.r[13];
        }
    }

    /// Load the current SP from its bank.
    fn load_sp(&mut self) {
        self.r[13] = if self.using_psp() { self.psp } else { self.msp };
    }

    fn write_sp(&mut self, v: u32) {
        self.r[13] = v & !3;
        self.bank_sp();
    }

    #[inline]
    fn rd(&self, r: u8) -> u32 {
        if r == 15 {
            self.cur_pc.wrapping_add(4)
        } else {
            self.r[r as usize]
        }
    }

    /// Register write from a data-processing instruction (ALUWritePC for the PC).
    fn wr(&mut self, r: u8, v: u32) {
        match r {
            15 => self.r[15] = v & !1,
            13 => self.write_sp(v),
            _ => self.r[r as usize] = v,
        }
    }

    // ---- priorities ------------------------------------------------------------------------

    fn basepri_group(&self, nvic: &Nvic) -> i32 {
        let sub_bits = nvic.prigroup as u32 + 1;
        let mask = if sub_bits >= 8 { 0 } else { 0xFFu32 << sub_bits };
        (self.basepri & mask) as i32
    }

    /// Current execution priority (B1.5.4).
    pub fn execution_priority(&self, nvic: &Nvic) -> i32 {
        let mut p = nvic.active_priority();
        if self.basepri & 0xFF != 0 {
            p = p.min(self.basepri_group(nvic));
        }
        if self.primask & 1 != 0 {
            p = p.min(0);
        }
        if self.faultmask & 1 != 0 {
            p = p.min(-1);
        }
        p
    }

    /// Execution priority ignoring PRIMASK (WFI wake-up condition).
    fn wake_priority(&self, nvic: &Nvic) -> i32 {
        let mut p = nvic.active_priority();
        if self.basepri & 0xFF != 0 {
            p = p.min(self.basepri_group(nvic));
        }
        if self.faultmask & 1 != 0 {
            p = p.min(-1);
        }
        p
    }

    // ---- main loop -------------------------------------------------------------------------

    fn account(&mut self, mem: &mut Memory, cycles: u64) {
        let hz = mem.peripherals.hclk_hz().max(1);
        self.cycles += cycles;
        self.time_sec += cycles as f64 / hz as f64;
        mem.peripherals.tick(cycles);
        self.clock_hz = mem.peripherals.hclk_hz();
    }

    /// Execute one instruction (or take one exception / sleep for a while). Returns the
    /// number of HCLK cycles consumed.
    pub fn step(&mut self, mem: &mut Memory) -> u32 {
        if self.locked_up {
            if let HaltReason::Running = self.halted_reason {
                self.halted_reason = HaltReason::Fault("Lockup".into());
            }
            return 0;
        }
        if let Some(err) = mem.peripherals.clock_error.clone() {
            self.halted_reason = HaltReason::Fault(err);
            return 0;
        }
        if mem.peripherals.nvic.reset_requested {
            mem.system_reset();
            self.reset(mem, None);
            return 1;
        }
        mem.peripherals.ipsr = self.ipsr();

        // Exception arbitration between instructions
        if let Some((exc, prio)) = mem.peripherals.nvic.highest_pending() {
            let exec = self.execution_priority(&mem.peripherals.nvic);
            if prio < exec {
                self.sleeping = false;
                let ret = self.r[15];
                self.exception_entry(mem, exc, ret);
                self.account(mem, 12);
                return 12;
            }
            if self.sleeping && prio < self.wake_priority(&mem.peripherals.nvic) {
                self.sleeping = false; // WFI wakes even when PRIMASK blocks the exception
            }
        }
        if self.sleeping {
            let n = mem.peripherals.cycles_to_next_event().unwrap_or(10_000).clamp(1, 1_000_000);
            self.account(mem, n);
            return n as u32;
        }

        let snapshot = (self.r, self.msp, self.psp, self.xpsr, self.control, self.primask, self.basepri, self.faultmask);
        let pc = self.r[15];
        self.cur_pc = pc;
        self.extra = 0;
        let cost = match self.exec_one(mem, pc) {
            Ok(c) => c + std::mem::take(&mut self.extra),
            Err(f) => {
                (self.r, self.msp, self.psp, self.xpsr, self.control, self.primask, self.basepri, self.faultmask) =
                    snapshot;
                self.raise_fault(mem, f, pc);
                12
            }
        };
        self.account(mem, cost as u64);
        cost
    }

    fn fetch(&mut self, mem: &mut Memory, addr: u32) -> Result<u16, Fault> {
        mem.fetch16(addr).map_err(|e| match e {
            MemError::Xn => Fault::Iaccviol,
            MemError::Bus => Fault::IBus,
        })
    }

    fn exec_one(&mut self, mem: &mut Memory, pc: u32) -> Result<u32, Fault> {
        if self.xpsr & (1 << 24) == 0 {
            return Err(Fault::InvState);
        }
        let hw1 = self.fetch(mem, pc)?;
        let is32 = thumb::is_thumb32(hw1);
        let hw2 = if is32 { self.fetch(mem, pc.wrapping_add(2))? } else { 0 };
        let it = self.itstate();
        let in_it = it & 0xF != 0;
        let last_in_it = it & 0xF == 0x8;
        let d = thumb::decode(hw1, hw2, in_it, last_in_it);
        self.r[15] = pc.wrapping_add(d.len as u32);
        if in_it {
            self.set_itstate(thumb::it_advance(it));
            if !self.cond_passed(it >> 4) {
                return Ok(1);
            }
        }
        if d.unpredictable {
            return Err(Fault::Undef);
        }
        self.execute(mem, d.instr)
    }

    // ---- memory helpers --------------------------------------------------------------------

    fn mem_read(&mut self, mem: &mut Memory, addr: u32, size: u32, unpriv: bool) -> Result<u32, Fault> {
        if size > 1 && addr & (size - 1) != 0 && mem.peripherals.nvic.ccr & (1 << 3) != 0 {
            return Err(Fault::Unaligned);
        }
        let privileged = self.privileged() && !unpriv;
        mem.read(addr, size, privileged).map_err(|_| Fault::Bus(addr))
    }

    fn mem_write(&mut self, mem: &mut Memory, addr: u32, size: u32, val: u32, unpriv: bool) -> Result<(), Fault> {
        if size > 1 && addr & (size - 1) != 0 && mem.peripherals.nvic.ccr & (1 << 3) != 0 {
            return Err(Fault::Unaligned);
        }
        let privileged = self.privileged() && !unpriv;
        mem.write(addr, size, val, privileged).map_err(|_| Fault::Bus(addr))
    }

    fn wait_states(&self, mem: &Memory, addr: u32) -> u32 {
        if mem.is_flash(addr) {
            mem.peripherals.flash_latency()
        } else {
            0
        }
    }

    /// Pipeline refill penalty for a taken branch to `target`.
    fn refill(&self, mem: &Memory, target: u32) -> u32 {
        1 + self.wait_states(mem, target)
    }

    /// Resolve an addressing mode: (access address, optional (register, write-back value)).
    fn effective(&self, a: Addr) -> (u32, Option<(u8, u32)>) {
        match a {
            Addr::Imm { rn, imm, add, index, wback } => {
                let base = self.rd(rn);
                let off = if add { base.wrapping_add(imm) } else { base.wrapping_sub(imm) };
                (if index { off } else { base }, if wback { Some((rn, off)) } else { None })
            }
            Addr::Reg { rn, rm, lsl } => (self.rd(rn).wrapping_add(self.rd(rm) << lsl), None),
            Addr::Lit { imm, add } => {
                let base = self.cur_pc.wrapping_add(4) & !3;
                (if add { base.wrapping_add(imm) } else { base.wrapping_sub(imm) }, None)
            }
        }
    }

    /// BXWritePC / LoadWritePC: interworking branch, exception return in Handler mode.
    fn bx_write_pc(&mut self, mem: &mut Memory, target: u32) -> Result<(), Fault> {
        if self.handler_mode() && target >> 28 == 0xF {
            return self.exception_return(mem, target);
        }
        if target & 1 != 0 {
            self.xpsr |= 1 << 24;
        } else {
            self.xpsr &= !(1 << 24);
        }
        self.r[15] = target & !1;
        Ok(())
    }

    // ---- data processing -------------------------------------------------------------------

    fn alu(&mut self, op: Alu, s: bool, rd: u8, a: u32, b: u32, shc: Option<bool>) {
        let c_in = self.flag_c();
        let (res, c, v) = match op {
            Alu::And | Alu::Tst => (a & b, shc, None),
            Alu::Bic => (a & !b, shc, None),
            Alu::Orr => (a | b, shc, None),
            Alu::Orn => (a | !b, shc, None),
            Alu::Eor | Alu::Teq => (a ^ b, shc, None),
            Alu::Mov => (b, shc, None),
            Alu::Mvn => (!b, shc, None),
            Alu::Add | Alu::Cmn => {
                let (r, c, v) = add_with_carry(a, b, false);
                (r, Some(c), Some(v))
            }
            Alu::Adc => {
                let (r, c, v) = add_with_carry(a, b, c_in);
                (r, Some(c), Some(v))
            }
            Alu::Sub | Alu::Cmp => {
                let (r, c, v) = add_with_carry(a, !b, true);
                (r, Some(c), Some(v))
            }
            Alu::Sbc => {
                let (r, c, v) = add_with_carry(a, !b, c_in);
                (r, Some(c), Some(v))
            }
            Alu::Rsb => {
                let (r, c, v) = add_with_carry(!a, b, true);
                (r, Some(c), Some(v))
            }
        };
        if !op.is_compare() {
            self.wr(rd, res);
        }
        if s && !(rd == 15 && !op.is_compare()) {
            self.set_nz(res);
            if let Some(c) = c {
                self.set_c(c);
            }
            if let Some(v) = v {
                self.set_v(v);
            }
        }
    }

    fn execute(&mut self, mem: &mut Memory, ins: Instr) -> Result<u32, Fault> {
        let next = self.r[15];
        let mut cost = 1u32;
        match ins {
            Instr::AluImm { op, s, rd, rn, imm, carry } => {
                let a = if op.has_rn() { self.rd(rn) } else { 0 };
                self.alu(op, s, rd, a, imm, carry);
                if rd == 15 && !op.is_compare() {
                    cost += self.refill(mem, self.r[15]);
                }
            }
            Instr::AluReg { op, s, rd, rn, rm, shift } => {
                let a = if op.has_rn() { self.rd(rn) } else { 0 };
                let (b, c) = shift_c(self.rd(rm), shift.t, shift.n as u32, self.flag_c());
                self.alu(op, s, rd, a, b, Some(c));
                if rd == 15 && !op.is_compare() {
                    cost += self.refill(mem, self.r[15]);
                }
            }
            Instr::ShiftReg { t, s, rd, rn, rm } => {
                let amount = self.rd(rm) & 0xFF;
                let (res, c) = shift_c(self.rd(rn), t, amount, self.flag_c());
                self.wr(rd, res);
                if s {
                    self.set_nz(res);
                    self.set_c(c);
                }
            }
            Instr::AddW { sub, rd, rn, imm } => {
                let a = self.rd(rn);
                self.wr(rd, if sub { a.wrapping_sub(imm) } else { a.wrapping_add(imm) });
            }
            Instr::Adr { rd, imm, add } => {
                let base = self.cur_pc.wrapping_add(4) & !3;
                self.wr(rd, if add { base.wrapping_add(imm) } else { base.wrapping_sub(imm) });
            }
            Instr::Movw { rd, imm } => self.wr(rd, imm as u32),
            Instr::Movt { rd, imm } => {
                let v = (self.rd(rd) & 0xFFFF) | ((imm as u32) << 16);
                self.wr(rd, v);
            }
            Instr::Mul { s, rd, rn, rm } => {
                let res = self.rd(rn).wrapping_mul(self.rd(rm));
                self.wr(rd, res);
                if s {
                    self.set_nz(res);
                }
            }
            Instr::Mla { rd, rn, rm, ra } => {
                let res = self.rd(rn).wrapping_mul(self.rd(rm)).wrapping_add(self.rd(ra));
                self.wr(rd, res);
                cost = 2;
            }
            Instr::Mls { rd, rn, rm, ra } => {
                let res = self.rd(ra).wrapping_sub(self.rd(rn).wrapping_mul(self.rd(rm)));
                self.wr(rd, res);
                cost = 2;
            }
            Instr::LongMul { kind, rdlo, rdhi, rn, rm } => {
                let (a, b) = (self.rd(rn), self.rd(rm));
                let acc = ((self.rd(rdhi) as u64) << 32) | self.rd(rdlo) as u64;
                let res: u64 = match kind {
                    LongMul::Smull => (a as i32 as i64).wrapping_mul(b as i32 as i64) as u64,
                    LongMul::Umull => (a as u64).wrapping_mul(b as u64),
                    LongMul::Smlal => ((a as i32 as i64).wrapping_mul(b as i32 as i64) as u64).wrapping_add(acc),
                    LongMul::Umlal => (a as u64).wrapping_mul(b as u64).wrapping_add(acc),
                };
                self.wr(rdlo, res as u32);
                self.wr(rdhi, (res >> 32) as u32);
                cost = match kind {
                    LongMul::Smull | LongMul::Umull => 4,
                    _ => 5,
                };
            }
            Instr::Div { signed, rd, rn, rm } => {
                let (n, m) = (self.rd(rn), self.rd(rm));
                if m == 0 {
                    if mem.peripherals.nvic.ccr & (1 << 4) != 0 {
                        return Err(Fault::DivByZero);
                    }
                    self.wr(rd, 0);
                } else if signed {
                    let q = if n as i32 == i32::MIN && m as i32 == -1 { n } else { ((n as i32) / (m as i32)) as u32 };
                    self.wr(rd, q);
                } else {
                    self.wr(rd, n / m);
                }
                let (an, am) = if signed { ((n as i32).unsigned_abs(), (m as i32).unsigned_abs()) } else { (n, m) };
                let diff = am.leading_zeros().saturating_sub(an.leading_zeros());
                cost = (2 + diff.div_ceil(4)).clamp(2, 12);
            }
            Instr::Extend { kind, rd, rm, rot } => {
                let v = self.rd(rm).rotate_right(rot as u32);
                let res = match kind {
                    ExtKind::Sxtb => v as u8 as i8 as i32 as u32,
                    ExtKind::Sxth => v as u16 as i16 as i32 as u32,
                    ExtKind::Uxtb => v & 0xFF,
                    ExtKind::Uxth => v & 0xFFFF,
                };
                self.wr(rd, res);
            }
            Instr::Rev { kind, rd, rm } => {
                let v = self.rd(rm);
                let res = match kind {
                    RevKind::Rev => v.swap_bytes(),
                    RevKind::Rev16 => ((v & 0x00FF_00FF) << 8) | ((v >> 8) & 0x00FF_00FF),
                    RevKind::Revsh => ((v as u16).swap_bytes() as i16) as i32 as u32,
                    RevKind::Rbit => v.reverse_bits(),
                };
                self.wr(rd, res);
            }
            Instr::Clz { rd, rm } => {
                let v = self.rd(rm).leading_zeros();
                self.wr(rd, v);
            }
            Instr::Bfx { signed, rd, rn, lsb, width } => {
                let v = self.rd(rn) >> lsb;
                let res = if width as u32 >= 32 {
                    v
                } else if signed {
                    let sh = 32 - width as u32;
                    (((v << sh) as i32) >> sh) as u32
                } else {
                    v & ((1u32 << width) - 1)
                };
                self.wr(rd, res);
            }
            Instr::Bfi { rd, rn, lsb, msb } => {
                let width = msb as u32 - lsb as u32 + 1;
                let mask = if width >= 32 { u32::MAX } else { ((1u32 << width) - 1) << lsb };
                let src = if rn == 15 { 0 } else { self.rd(rn) << lsb };
                let v = (self.rd(rd) & !mask) | (src & mask);
                self.wr(rd, v);
            }
            Instr::Sat { unsigned, rd, sat, rn, shift } => {
                let (v, _) = shift_c(self.rd(rn), shift.t, shift.n as u32, self.flag_c());
                let v = v as i32 as i64;
                let (lo, hi) = if unsigned {
                    (0i64, (1i64 << sat) - 1)
                } else {
                    (-(1i64 << (sat - 1)), (1i64 << (sat - 1)) - 1)
                };
                let r = v.clamp(lo, hi);
                if r != v {
                    self.xpsr |= 1 << 27; // Q
                }
                self.wr(rd, r as u32);
            }
            Instr::Ldr { sz, signed, rt, addr, unpriv } => {
                let (ea, wb) = self.effective(addr);
                let raw = self.mem_read(mem, ea, sz.bytes(), unpriv)?;
                let v = match (sz, signed) {
                    (Size::B, true) => raw as u8 as i8 as i32 as u32,
                    (Size::H, true) => raw as u16 as i16 as i32 as u32,
                    _ => raw,
                };
                if let Some((rn, val)) = wb {
                    self.wr(rn, val);
                }
                cost = 2 + self.wait_states(mem, ea);
                if rt == 15 {
                    if ea & 3 != 0 {
                        return Err(Fault::Unaligned);
                    }
                    self.bx_write_pc(mem, v)?;
                    cost += self.refill(mem, self.r[15]);
                } else {
                    self.wr(rt, v);
                }
            }
            Instr::Str { sz, rt, addr, unpriv } => {
                let (ea, wb) = self.effective(addr);
                let v = self.rd(rt);
                self.mem_write(mem, ea, sz.bytes(), v, unpriv)?;
                if let Some((rn, val)) = wb {
                    self.wr(rn, val);
                }
                cost = 2;
            }
            Instr::Ldrd { rt, rt2, addr } => {
                let (ea, wb) = self.effective(addr);
                if ea & 3 != 0 {
                    return Err(Fault::Unaligned);
                }
                let a = self.mem_read(mem, ea, 4, false)?;
                let b = self.mem_read(mem, ea.wrapping_add(4), 4, false)?;
                if let Some((rn, val)) = wb {
                    self.wr(rn, val);
                }
                self.wr(rt, a);
                self.wr(rt2, b);
                cost = 3 + 2 * self.wait_states(mem, ea);
            }
            Instr::Strd { rt, rt2, addr } => {
                let (ea, wb) = self.effective(addr);
                if ea & 3 != 0 {
                    return Err(Fault::Unaligned);
                }
                let (a, b) = (self.rd(rt), self.rd(rt2));
                self.mem_write(mem, ea, 4, a, false)?;
                self.mem_write(mem, ea.wrapping_add(4), 4, b, false)?;
                if let Some((rn, val)) = wb {
                    self.wr(rn, val);
                }
                cost = 3;
            }
            Instr::Ldrex { sz, rt, rn, imm } => {
                let ea = self.rd(rn).wrapping_add(imm);
                if ea & (sz.bytes() - 1) != 0 {
                    return Err(Fault::Unaligned);
                }
                let v = self.mem_read(mem, ea, sz.bytes(), false)?;
                self.excl = Some(ea);
                self.wr(rt, v);
                cost = 2;
            }
            Instr::Strex { sz, rd, rt, rn, imm } => {
                let ea = self.rd(rn).wrapping_add(imm);
                if ea & (sz.bytes() - 1) != 0 {
                    return Err(Fault::Unaligned);
                }
                if self.excl == Some(ea) {
                    let v = self.rd(rt);
                    self.mem_write(mem, ea, sz.bytes(), v, false)?;
                    self.wr(rd, 0);
                } else {
                    self.wr(rd, 1);
                }
                self.excl = None;
                cost = 2;
            }
            Instr::Clrex => self.excl = None,
            Instr::Ldm { rn, regs, wback, db } => {
                let n = regs.count_ones();
                let base = self.rd(rn);
                let start = if db { base.wrapping_sub(4 * n) } else { base };
                if start & 3 != 0 {
                    return Err(Fault::Unaligned);
                }
                let mut vals = [0u32; 16];
                let mut a = start;
                for (r, v) in vals.iter_mut().enumerate() {
                    if regs & (1 << r) != 0 {
                        *v = self.mem_read(mem, a, 4, false)?;
                        a = a.wrapping_add(4);
                    }
                }
                if wback && regs & (1 << rn) == 0 {
                    let nb = if db { start } else { base.wrapping_add(4 * n) };
                    self.wr(rn, nb);
                }
                for r in 0..15u8 {
                    if regs & (1 << r) != 0 {
                        self.wr(r, vals[r as usize]);
                    }
                }
                cost = 1 + n + self.wait_states(mem, start) * n;
                if regs & 0x8000 != 0 {
                    self.bx_write_pc(mem, vals[15])?;
                    cost += self.refill(mem, self.r[15]);
                }
            }
            Instr::Stm { rn, regs, wback, db } => {
                let n = regs.count_ones();
                let base = self.rd(rn);
                let start = if db { base.wrapping_sub(4 * n) } else { base };
                if start & 3 != 0 {
                    return Err(Fault::Unaligned);
                }
                let mut a = start;
                for r in 0..16u8 {
                    if regs & (1 << r) != 0 {
                        let v = self.rd(r);
                        self.mem_write(mem, a, 4, v, false)?;
                        a = a.wrapping_add(4);
                    }
                }
                if wback {
                    let nb = if db { start } else { base.wrapping_add(4 * n) };
                    self.wr(rn, nb);
                }
                cost = 1 + n;
            }
            Instr::Preload { .. } => {}
            Instr::Tb { half, rn, rm } => {
                let base = self.rd(rn);
                let idx = self.rd(rm);
                let (ea, size) = if half { (base.wrapping_add(idx << 1), 2) } else { (base.wrapping_add(idx), 1) };
                let off = self.mem_read(mem, ea, size, false)?;
                let target = self.cur_pc.wrapping_add(4).wrapping_add(off * 2);
                self.r[15] = target;
                cost = 2 + self.wait_states(mem, ea) + self.refill(mem, target);
            }
            Instr::B { cond, off } => {
                if self.cond_passed(cond) {
                    let target = self.cur_pc.wrapping_add(4).wrapping_add(off as u32);
                    self.r[15] = target;
                    cost += self.refill(mem, target);
                }
            }
            Instr::Bl { off } => {
                let target = self.cur_pc.wrapping_add(4).wrapping_add(off as u32);
                self.r[14] = next | 1;
                self.r[15] = target;
                cost += self.refill(mem, target);
            }
            Instr::Bx { rm } => {
                let t = self.rd(rm);
                self.bx_write_pc(mem, t)?;
                cost += self.refill(mem, self.r[15]);
            }
            Instr::Blx { rm } => {
                let t = self.rd(rm);
                self.r[14] = next | 1;
                if t & 1 != 0 {
                    self.xpsr |= 1 << 24;
                } else {
                    self.xpsr &= !(1 << 24);
                }
                self.r[15] = t & !1;
                cost += self.refill(mem, self.r[15]);
            }
            Instr::Cbz { nz, rn, off } => {
                if (self.rd(rn) != 0) == nz {
                    let target = self.cur_pc.wrapping_add(4).wrapping_add(off);
                    self.r[15] = target;
                    cost += self.refill(mem, target);
                }
            }
            Instr::It { firstcond, mask } => {
                self.set_itstate((firstcond << 4) | mask);
            }
            Instr::Hint(h) => match h {
                HintKind::Wfi => {
                    self.sleeping = true;
                }
                HintKind::Wfe => {
                    if self.event_reg {
                        self.event_reg = false;
                    } else {
                        self.sleeping = true;
                    }
                }
                HintKind::Sev => self.event_reg = true,
                _ => {}
            },
            Instr::Svc(_) => {
                // SVC is taken synchronously; escalates to HardFault if it cannot preempt
                let nvic = &mem.peripherals.nvic;
                let exec = self.execution_priority(nvic);
                if nvic.group_priority(EXC_SVCALL) < exec {
                    self.exception_entry(mem, EXC_SVCALL, next);
                } else {
                    self.escalate(mem, next, "SVC executed at a priority that blocks SVCall")?;
                }
                cost = 12;
            }
            Instr::Bkpt(n) => {
                self.halted_reason = HaltReason::Bkpt(n);
            }
            Instr::Cps { disable, i, f } => {
                if self.privileged() {
                    if i {
                        self.primask = disable as u32;
                    }
                    if f {
                        if !disable {
                            self.faultmask = 0;
                        } else if self.execution_priority(&mem.peripherals.nvic) > -1 {
                            self.faultmask = 1;
                        }
                    }
                }
            }
            Instr::Msr { sysm, mask, rn } => {
                let v = self.rd(rn);
                self.msr(mem, sysm, mask, v);
                cost = 2;
            }
            Instr::Mrs { rd, sysm } => {
                let v = self.mrs(sysm);
                self.wr(rd, v);
                cost = 2;
            }
            Instr::Barrier { kind, .. } => {
                if kind == Barrier::Isb {
                    cost += self.refill(mem, next);
                }
            }
            Instr::Udf { .. } | Instr::Undefined => return Err(Fault::Undef),
            Instr::Coproc => return Err(Fault::NoCp),
        }
        Ok(cost)
    }

    fn mrs(&self, sysm: u8) -> u32 {
        let priv_ = self.privileged();
        match sysm {
            0..=7 => {
                let mut v = 0;
                if sysm & 1 != 0 {
                    v |= self.ipsr();
                }
                if sysm & 4 == 0 {
                    v |= self.xpsr & 0xF800_0000;
                }
                v
            }
            8 => {
                if !priv_ {
                    0
                } else if self.using_psp() {
                    self.msp
                } else {
                    self.r[13]
                }
            }
            9 => {
                if !priv_ {
                    0
                } else if self.using_psp() {
                    self.r[13]
                } else {
                    self.psp
                }
            }
            16 => self.primask & 1,
            17 | 18 => self.basepri & 0xFF,
            19 => self.faultmask & 1,
            20 => self.control & 3,
            _ => 0,
        }
    }

    fn msr(&mut self, mem: &mut Memory, sysm: u8, mask: u8, v: u32) {
        let priv_ = self.privileged();
        match sysm {
            0..=3 => {
                if mask & 2 != 0 {
                    self.xpsr = (self.xpsr & !0xF800_0000) | (v & 0xF800_0000);
                }
            }
            8 if priv_ => {
                if self.using_psp() {
                    self.msp = v & !3;
                } else {
                    self.write_sp(v);
                }
            }
            9 if priv_ => {
                if self.using_psp() {
                    self.write_sp(v);
                } else {
                    self.psp = v & !3;
                }
            }
            16 if priv_ => self.primask = v & 1,
            17 if priv_ => self.basepri = v & PRIO_MASK as u32,
            18 if priv_ => {
                let nv = v & PRIO_MASK as u32;
                if nv != 0 && (nv < self.basepri || self.basepri == 0) {
                    self.basepri = nv;
                }
            }
            19 if priv_ => {
                if self.execution_priority(&mem.peripherals.nvic) > -1 {
                    self.faultmask = v & 1;
                }
            }
            20 if priv_ => {
                self.bank_sp();
                let spsel = if self.handler_mode() { self.control & 2 } else { v & 2 };
                self.control = (v & 1) | spsel;
                self.load_sp();
            }
            _ => {}
        }
    }

    // ---- exceptions ------------------------------------------------------------------------

    /// PushStack + ExceptionTaken (B1.5.6 / B1.5.7).
    fn exception_entry(&mut self, mem: &mut Memory, exc: usize, ret_addr: u32) {
        let forcealign = mem.peripherals.nvic.ccr & (1 << 9) != 0;
        let sp = self.r[13];
        let align = forcealign && sp & 4 != 0;
        let frame = sp.wrapping_sub(0x20) & if forcealign { !4 } else { !0 };
        let psr = (self.xpsr & !(1 << 9)) | ((align as u32) << 9);
        let words = [self.r[0], self.r[1], self.r[2], self.r[3], self.r[12], self.r[14], ret_addr, psr];
        let mut stk_err = false;
        for (i, w) in words.iter().enumerate() {
            if mem.write(frame + 4 * i as u32, 4, *w, true).is_err() {
                stk_err = true;
            }
        }
        self.write_sp(frame);
        self.r[14] = if self.handler_mode() {
            0xFFFF_FFF1
        } else if self.control & 2 != 0 {
            0xFFFF_FFFD
        } else {
            0xFFFF_FFF9
        };
        self.take(mem, exc);
        if stk_err {
            let nvic = &mut mem.peripherals.nvic;
            nvic.cfsr |= CFSR_STKERR;
            nvic.pending[EXC_BUSFAULT] = true;
            self.note_fault(mem, "BusFault STKERR (exception stacking failed: stack pointer outside SRAM)");
        }
    }

    /// ExceptionTaken: enter the handler for `exc` (registers already stacked).
    fn take(&mut self, mem: &mut Memory, exc: usize) {
        let vtor = mem.peripherals.vtor();
        let vec = mem.read(vtor.wrapping_add(4 * exc as u32), 4, true);
        self.bank_sp();
        self.xpsr = (self.xpsr & 0xF800_0000) | (exc as u32) | (1 << 24);
        self.control &= !2;
        self.load_sp();
        let nvic = &mut mem.peripherals.nvic;
        nvic.active[exc] = true;
        nvic.pending[exc] = false;
        mem.peripherals.ipsr = exc as u32;
        self.excl = None;
        self.event_reg = true;
        self.sleeping = false;
        match vec {
            Ok(v) => {
                if v & 1 == 0 {
                    self.xpsr &= !(1 << 24);
                }
                self.r[15] = v & !1;
            }
            Err(_) => {
                mem.peripherals.nvic.hfsr |= HFSR_VECTTBL;
                self.r[15] = 0;
                self.xpsr &= !(1 << 24);
                self.lockup(format!("vector table read for exception {} failed (VTOR = 0x{:08X})", exc, vtor));
            }
        }
    }

    fn lockup(&mut self, why: String) {
        self.locked_up = true;
        self.halted_reason = HaltReason::Fault(format!("Lockup: {}", why));
    }

    fn note_fault(&mut self, _mem: &mut Memory, desc: &str) {
        let msg = format!("{} at 0x{:08X}", desc, self.cur_pc);
        self.last_fault = Some(msg.clone());
        if self.halt_on_fault && !self.locked_up {
            self.halted_reason = HaltReason::Fault(msg);
        }
    }

    /// Escalate to HardFault (FORCED) or lock up.
    fn escalate(&mut self, mem: &mut Memory, ret: u32, desc: &str) -> Result<(), Fault> {
        let exec = self.execution_priority(&mem.peripherals.nvic);
        if exec <= -1 {
            self.lockup(format!("{} while executing at priority {} (HardFault/NMI or FAULTMASK)", desc, exec));
            return Ok(());
        }
        mem.peripherals.nvic.hfsr |= HFSR_FORCED;
        self.exception_entry(mem, EXC_HARDFAULT, ret);
        self.note_fault(mem, &format!("HardFault (forced from {})", desc));
        Ok(())
    }

    fn raise_fault(&mut self, mem: &mut Memory, f: Fault, pc: u32) {
        self.cur_pc = pc;
        let (exc, bits) = match f {
            Fault::Undef => (EXC_USAGEFAULT, CFSR_UNDEFINSTR),
            Fault::InvState => (EXC_USAGEFAULT, CFSR_INVSTATE),
            Fault::InvPc => (EXC_USAGEFAULT, CFSR_INVPC),
            Fault::NoCp => (EXC_USAGEFAULT, CFSR_NOCP),
            Fault::Unaligned => (EXC_USAGEFAULT, CFSR_UNALIGNED),
            Fault::DivByZero => (EXC_USAGEFAULT, CFSR_DIVBYZERO),
            Fault::Bus(_) => (EXC_BUSFAULT, CFSR_PRECISERR | CFSR_BFARVALID),
            Fault::IBus => (EXC_BUSFAULT, CFSR_IBUSERR),
            Fault::Iaccviol => (EXC_MEMMANAGE, CFSR_IACCVIOL),
        };
        {
            let nvic = &mut mem.peripherals.nvic;
            nvic.cfsr |= bits;
            if let Fault::Bus(a) = f {
                nvic.bfar = a;
            }
        }
        let desc = describe_fault(f);
        let full = match f {
            Fault::Bus(a) => format!("{} [address 0x{:08X}]", desc, a),
            _ => desc.to_string(),
        };
        let nvic = &mem.peripherals.nvic;
        let exec = self.execution_priority(nvic);
        if nvic.is_enabled_exc(exc) && nvic.group_priority(exc) < exec {
            self.exception_entry(mem, exc, pc);
            self.note_fault(mem, &full);
        } else {
            let _ = self.escalate(mem, pc, &full);
        }
    }

    /// Exception return (B1.5.8) with tail-chaining.
    fn exception_return(&mut self, mem: &mut Memory, exc_return: u32) -> Result<(), Fault> {
        let cur = self.ipsr() as usize;
        let kind = exc_return & 0xF;
        let nvic = &mem.peripherals.nvic;
        if exc_return & 0x0FFF_FFF0 != 0x0FFF_FFF0 || !matches!(kind, 0x1 | 0x9 | 0xD) {
            return Err(Fault::InvPc);
        }
        if cur >= NUM_EXC || !nvic.active[cur] {
            return Err(Fault::InvPc);
        }
        let nested = nvic.active_count();
        if kind == 0x1 && nested == 1 {
            return Err(Fault::InvPc);
        }
        if kind != 0x1 && nested != 1 && nvic.ccr & 1 == 0 {
            return Err(Fault::InvPc);
        }
        // Deactivate
        mem.peripherals.nvic.active[cur] = false;
        if cur != EXC_NMI {
            self.faultmask = 0;
        }
        mem.peripherals.nvic.resample_lines();
        self.excl = None;

        // Tail-chain into a pending exception that can preempt the context we return to
        let target_prio = {
            let nvic = &mem.peripherals.nvic;
            let mut p = nvic.active_priority();
            if self.basepri & 0xFF != 0 {
                p = p.min(self.basepri_group(nvic));
            }
            if self.primask & 1 != 0 {
                p = p.min(0);
            }
            p
        };
        if let Some((exc, prio)) = mem.peripherals.nvic.highest_pending() {
            if prio < target_prio {
                self.r[14] = exc_return;
                self.take(mem, exc);
                self.extra += 6;
                return Ok(());
            }
        }

        // PopStack (we are in Handler mode, so R13 is the MSP)
        self.bank_sp();
        let frame = if kind == 0xD { self.psp } else { self.msp };
        let mut w = [0u32; 8];
        for (i, slot) in w.iter_mut().enumerate() {
            match mem.read(frame + 4 * i as u32, 4, true) {
                Ok(v) => *slot = v,
                Err(_) => {
                    mem.peripherals.nvic.cfsr |= CFSR_UNSTKERR;
                    mem.peripherals.nvic.pending[EXC_BUSFAULT] = true;
                }
            }
        }
        let psr = w[7];
        let forcealign = mem.peripherals.nvic.ccr & (1 << 9) != 0;
        let spmask = if psr & (1 << 9) != 0 && forcealign { 4 } else { 0 };
        let new_sp = (frame.wrapping_add(0x20)) | spmask;
        match kind {
            0x1 => {}
            0x9 => self.control &= !2,
            _ => self.control |= 2,
        }
        self.r[0] = w[0];
        self.r[1] = w[1];
        self.r[2] = w[2];
        self.r[3] = w[3];
        self.r[12] = w[4];
        self.r[14] = w[5];
        self.r[15] = w[6] & !1;
        self.xpsr = psr & !(1 << 9);
        // Restore SP into the bank selected by EXC_RETURN
        if kind == 0xD {
            self.psp = new_sp;
        } else {
            self.msp = new_sp;
        }
        self.load_sp();
        mem.peripherals.ipsr = self.ipsr();
        let ipsr = self.ipsr();
        if (kind == 0x1 && ipsr == 0) || (kind != 0x1 && ipsr != 0) {
            // Stacked IPSR inconsistent with EXC_RETURN
            mem.peripherals.nvic.cfsr |= CFSR_INVPC;
            self.cur_pc = self.r[15];
            let _ = self.escalate(mem, self.r[15], describe_fault(Fault::InvPc));
            return Ok(());
        }
        if kind != 0x1 && mem.peripherals.nvic.scr & 2 != 0 {
            self.sleeping = true; // SLEEPONEXIT
        }
        self.extra += 10;
        Ok(())
    }

    /// True when the core can make no further progress without an external event
    /// (sleeping with nothing armed, or locked up).
    pub fn is_stuck(&self, mem: &Memory) -> bool {
        self.locked_up || (self.sleeping && !mem.peripherals.interrupt_armed())
    }
}

/// Convenience re-exports used by the assembler and older callers.
pub fn is_thumb32(hw1: u16) -> bool {
    thumb::is_thumb32(hw1)
}

/// ThumbExpandImm_C(imm12, carry_in) -> (imm32, carry_out).
pub fn thumb_expand_imm_c(imm12: u32, carry_in: bool) -> (u32, bool) {
    match thumb::thumb_expand_imm(imm12) {
        Some((v, c)) => (v, c.unwrap_or(carry_in)),
        None => (0, carry_in),
    }
}
