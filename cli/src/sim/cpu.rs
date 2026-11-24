use crate::sim::memory::Memory;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HaltReason {
    Running,
    Breakpoint(u32),
    Bkpt(u8),
    SelfLoop(u32),
    MaxSteps,
    Fault(String),
}

#[derive(Debug, Clone)]
pub struct Cpu {
    pub r: [u32; 16], // 13=SP, 14=LR, 15=PC
    pub msp: u32,
    pub psp: u32,
    pub xpsr: u32,
    pub primask: u32,
    pub control: u32,
    pub cycles: u64,
    pub clock_hz: u32,
    pub halted_reason: HaltReason,
}

impl Default for Cpu {
    fn default() -> Self {
        Self {
            r: [0; 16],
            msp: 0x2000_1000,
            psp: 0,
            xpsr: 0x0100_0000, // T=1
            primask: 0,
            control: 0,
            cycles: 0,
            clock_hz: 8_000_000,
            halted_reason: HaltReason::Running,
        }
    }
}

impl Cpu {
    pub fn reset(&mut self, mem: &mut Memory, entry_fallback: Option<u32>) {
        self.r = [0; 16];
        let vtor = mem.peripherals.vtor;
        let init_sp = mem.read32(vtor);
        let reset_vec = mem.read32(vtor.wrapping_add(4));

        self.msp = if init_sp != 0xFFFF_FFFF && init_sp != 0 {
            init_sp & !3
        } else {
            0x2000_1000
        };
        self.psp = 0;
        self.r[13] = self.msp;
        self.r[14] = 0xFFFF_FFFF;

        let start_pc = if reset_vec != 0xFFFF_FFFF && reset_vec != 0 {
            reset_vec & !1
        } else {
            entry_fallback.unwrap_or(0x0800_0000) & !1
        };
        self.r[15] = start_pc;
        self.xpsr = 0x0100_0000;
        self.primask = 0;
        self.control = 0;
        self.cycles = 0;
        self.halted_reason = HaltReason::Running;
    }

    #[inline]
    pub fn flag_n(&self) -> bool { (self.xpsr >> 31) & 1 != 0 }
    #[inline]
    pub fn flag_z(&self) -> bool { (self.xpsr >> 30) & 1 != 0 }
    #[inline]
    pub fn flag_c(&self) -> bool { (self.xpsr >> 29) & 1 != 0 }
    #[inline]
    pub fn flag_v(&self) -> bool { (self.xpsr >> 28) & 1 != 0 }
    #[inline]
    pub fn flag_q(&self) -> bool { (self.xpsr >> 27) & 1 != 0 }

    #[inline]
    pub fn set_nz(&mut self, res: u32) {
        self.xpsr = (self.xpsr & !(3 << 30))
            | (((res >> 31) & 1) << 31)
            | (if res == 0 { 1 << 30 } else { 0 });
    }

    #[inline]
    pub fn set_nzcv(&mut self, res: u32, c: bool, v: bool) {
        self.set_nz(res);
        self.xpsr = (self.xpsr & !(3 << 28)) | ((c as u32) << 29) | ((v as u32) << 28);
    }

    #[inline]
    pub fn set_nzc(&mut self, res: u32, c: bool) {
        self.set_nz(res);
        self.xpsr = (self.xpsr & !(1 << 29)) | ((c as u32) << 29);
    }

    #[inline]
    fn get_itstate(&self) -> u8 {
        let hi = ((self.xpsr >> 10) & 0x3F) as u8;
        let lo = ((self.xpsr >> 25) & 0x03) as u8;
        (hi << 2) | lo
    }

    #[inline]
    fn set_itstate(&mut self, it: u8) {
        let hi = ((it >> 2) & 0x3F) as u32;
        let lo = (it & 0x03) as u32;
        self.xpsr = (self.xpsr & !((0x3F << 10) | (0x03 << 25))) | (hi << 10) | (lo << 25);
    }

    #[inline]
    fn in_it_block(&self) -> bool {
        (self.get_itstate() & 0x0F) != 0
    }

    fn advance_itstate(&mut self) {
        let it = self.get_itstate();
        if (it & 0x0F) == 0 {
            return;
        }
        let next = if (it & 0x07) == 0 {
            0
        } else {
            (it & 0xE0) | ((it << 1) & 0x1F)
        };
        self.set_itstate(next);
    }

    pub fn cond_passed(&self, cond: u8) -> bool {
        let n = self.flag_n();
        let z = self.flag_z();
        let c = self.flag_c();
        let v = self.flag_v();
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
            0xC => !z && (n == v),
            0xD => z || (n != v),
            _ => true,
        }
    }

    #[inline]
    fn read_reg(&self, reg: usize, instr_pc: u32) -> u32 {
        if reg == 15 {
            instr_pc.wrapping_add(4)
        } else {
            self.r[reg]
        }
    }

    #[inline]
    fn write_reg(&mut self, reg: usize, val: u32) {
        if reg == 15 {
            self.r[15] = val & !1;
        } else if reg == 13 {
            self.r[13] = val & !3;
            if (self.control & 2) != 0 && (self.xpsr & 0x1FF) == 0 {
                self.psp = self.r[13];
            } else {
                self.msp = self.r[13];
            }
        } else {
            self.r[reg] = val;
        }
    }

    pub fn step(&mut self, mem: &mut Memory) -> u32 {
        let pc = self.r[15];
        let hw1 = mem.read16(pc);
        let is_32 = is_thumb32(hw1);
        let instr_len = if is_32 { 4 } else { 2 };
        let hw2 = if is_32 { mem.read16(pc.wrapping_add(2)) } else { 0 };

        let it = self.get_itstate();
        let in_it = (it & 0x0F) != 0;
        let it_exec = if in_it { self.cond_passed(it >> 4) } else { true };

        self.r[15] = pc.wrapping_add(instr_len);

        let mut is_it_instr = false;
        let mut cost = 1u32;

        if it_exec {
            if is_32 {
                cost = self.exec_thumb32(mem, pc, hw1, hw2);
            } else {
                let (c, it_set) = self.exec_thumb16(mem, pc, hw1, in_it);
                cost = c;
                is_it_instr = it_set;
            }
        }

        if in_it && !is_it_instr {
            self.advance_itstate();
        }

        self.cycles += cost as u64;
        let pending_irqs = mem.peripherals.tick(cost);
        if self.primask == 0 {
            if let Some(&exc) = pending_irqs.first() {
                self.enter_exception(mem, exc);
            }
        }
        cost
    }

    fn enter_exception(&mut self, mem: &mut Memory, exc_num: u16) {
        let cur_exc = (self.xpsr & 0x1FF) as u16;
        if cur_exc != 0 {
            return;
        }
        let sp = self.r[13].wrapping_sub(32) & !7;
        mem.write32(sp, self.r[0]);
        mem.write32(sp + 4, self.r[1]);
        mem.write32(sp + 8, self.r[2]);
        mem.write32(sp + 12, self.r[3]);
        mem.write32(sp + 16, self.r[12]);
        mem.write32(sp + 20, self.r[14]);
        mem.write32(sp + 24, self.r[15]);
        mem.write32(sp + 28, self.xpsr);
        self.write_reg(13, sp);
        self.r[14] = 0xFFFF_FFF9;
        self.xpsr = (self.xpsr & !0x1FF) | (exc_num as u32);
        let vec = mem.read32(mem.peripherals.vtor.wrapping_add((exc_num as u32) * 4));
        if vec != 0 && vec != 0xFFFF_FFFF {
            self.r[15] = vec & !1;
        }
    }

    fn exception_return(&mut self, mem: &mut Memory, exc_ret: u32) {
        let sp = if (exc_ret & 0x4) != 0 { self.psp } else { self.msp };
        self.r[0] = mem.read32(sp);
        self.r[1] = mem.read32(sp + 4);
        self.r[2] = mem.read32(sp + 8);
        self.r[3] = mem.read32(sp + 12);
        self.r[12] = mem.read32(sp + 16);
        self.r[14] = mem.read32(sp + 20);
        let ret_pc = mem.read32(sp + 24);
        self.xpsr = mem.read32(sp + 28) | (1 << 24);
        self.write_reg(13, sp.wrapping_add(32));
        self.r[15] = ret_pc & !1;
    }

    fn bx_target(&mut self, mem: &mut Memory, target: u32) {
        if (target & 0xF000_0000) == 0xF000_0000 && (self.xpsr & 0x1FF) != 0 {
            self.exception_return(mem, target);
        } else {
            self.r[15] = target & !1;
        }
    }

    fn exec_thumb16(&mut self, mem: &mut Memory, pc: u32, op: u16, in_it: bool) -> (u32, bool) {
        let setflags = !in_it;
        let top3 = (op >> 13) & 0x7;

        match top3 {
            0b000 => {
                let op2 = (op >> 11) & 0x3;
                let rd = (op & 0x7) as usize;
                let rm_rn = ((op >> 3) & 0x7) as usize;
                let imm5 = ((op >> 6) & 0x1F) as u32;
                if op2 != 0b11 {
                    let val = self.r[rm_rn];
                    let (res, c) = match op2 {
                        0b00 => shift_c(val, 0, imm5, self.flag_c()),
                        0b01 => shift_c(val, 1, if imm5 == 0 { 32 } else { imm5 }, self.flag_c()),
                        _ => shift_c(val, 2, if imm5 == 0 { 32 } else { imm5 }, self.flag_c()),
                    };
                    self.r[rd] = res;
                    if setflags {
                        self.set_nzc(res, c);
                    }
                } else {
                    let sub_op = (op >> 9) & 0x3;
                    let rn_val = self.r[rm_rn];
                    let rhs = match sub_op & 1 {
                        0 => self.r[((op >> 6) & 0x7) as usize],
                        _ => ((op >> 6) & 0x7) as u32,
                    };
                    let (res, c, v) = if (sub_op & 2) == 0 {
                        add_with_carry(rn_val, rhs, false)
                    } else {
                        add_with_carry(rn_val, !rhs, true)
                    };
                    self.r[rd] = res;
                    if setflags {
                        self.set_nzcv(res, c, v);
                    }
                }
                (1, false)
            }
            0b001 => {
                let op2 = (op >> 11) & 0x3;
                let rdn = ((op >> 8) & 0x7) as usize;
                let imm8 = (op & 0xFF) as u32;
                match op2 {
                    0b00 => {
                        self.r[rdn] = imm8;
                        if setflags {
                            self.set_nz(imm8);
                        }
                    }
                    0b01 => {
                        let (res, c, v) = add_with_carry(self.r[rdn], !imm8, true);
                        self.set_nzcv(res, c, v);
                    }
                    0b10 => {
                        let (res, c, v) = add_with_carry(self.r[rdn], imm8, false);
                        self.r[rdn] = res;
                        if setflags {
                            self.set_nzcv(res, c, v);
                        }
                    }
                    _ => {
                        let (res, c, v) = add_with_carry(self.r[rdn], !imm8, true);
                        self.r[rdn] = res;
                        if setflags {
                            self.set_nzcv(res, c, v);
                        }
                    }
                }
                (1, false)
            }
            0b010 => {
                if (op & 0xFC00) == 0x4000 {
                    // Data-processing 16-bit
                    let alu_op = (op >> 6) & 0xF;
                    let rm = ((op >> 3) & 0x7) as usize;
                    let rdn = (op & 0x7) as usize;
                    let a = self.r[rdn];
                    let b = self.r[rm];
                    match alu_op {
                        0x0 => {
                            let res = a & b;
                            self.r[rdn] = res;
                            if setflags { self.set_nz(res); }
                        }
                        0x1 => {
                            let res = a ^ b;
                            self.r[rdn] = res;
                            if setflags { self.set_nz(res); }
                        }
                        0x2 => {
                            let (res, c) = shift_c(a, 0, b & 0xFF, self.flag_c());
                            self.r[rdn] = res;
                            if setflags { self.set_nzc(res, c); }
                        }
                        0x3 => {
                            let (res, c) = shift_c(a, 1, b & 0xFF, self.flag_c());
                            self.r[rdn] = res;
                            if setflags { self.set_nzc(res, c); }
                        }
                        0x4 => {
                            let (res, c) = shift_c(a, 2, b & 0xFF, self.flag_c());
                            self.r[rdn] = res;
                            if setflags { self.set_nzc(res, c); }
                        }
                        0x5 => {
                            let (res, c, v) = add_with_carry(a, b, self.flag_c());
                            self.r[rdn] = res;
                            if setflags { self.set_nzcv(res, c, v); }
                        }
                        0x6 => {
                            let (res, c, v) = add_with_carry(a, !b, self.flag_c());
                            self.r[rdn] = res;
                            if setflags { self.set_nzcv(res, c, v); }
                        }
                        0x7 => {
                            let (res, c) = shift_c(a, 3, b & 0xFF, self.flag_c());
                            self.r[rdn] = res;
                            if setflags { self.set_nzc(res, c); }
                        }
                        0x8 => {
                            let res = a & b;
                            self.set_nz(res);
                        }
                        0x9 => {
                            let (res, c, v) = add_with_carry(!b, 0, true);
                            self.r[rdn] = res;
                            if setflags { self.set_nzcv(res, c, v); }
                        }
                        0xA => {
                            let (res, c, v) = add_with_carry(a, !b, true);
                            self.set_nzcv(res, c, v);
                        }
                        0xB => {
                            let (res, c, v) = add_with_carry(a, b, false);
                            self.set_nzcv(res, c, v);
                        }
                        0xC => {
                            let res = a | b;
                            self.r[rdn] = res;
                            if setflags { self.set_nz(res); }
                        }
                        0xD => {
                            let res = a.wrapping_mul(b);
                            self.r[rdn] = res;
                            if setflags { self.set_nz(res); }
                        }
                        0xE => {
                            let res = a & !b;
                            self.r[rdn] = res;
                            if setflags { self.set_nz(res); }
                        }
                        _ => {
                            let res = !b;
                            self.r[rdn] = res;
                            if setflags { self.set_nz(res); }
                        }
                    }
                    (1, false)
                } else if (op & 0xFC00) == 0x4400 {
                    // Special data instructions & branch/exchange
                    let sub = (op >> 8) & 0x3;
                    let rm = ((op >> 3) & 0xF) as usize;
                    let rdn = (((op >> 7) & 1) << 3 | (op & 0x7)) as usize;
                    match sub {
                        0b00 => {
                            let a = self.read_reg(rdn, pc);
                            let b = self.read_reg(rm, pc);
                            self.write_reg(rdn, a.wrapping_add(b));
                        }
                        0b01 => {
                            let a = self.read_reg(rdn, pc);
                            let b = self.read_reg(rm, pc);
                            let (res, c, v) = add_with_carry(a, !b, true);
                            self.set_nzcv(res, c, v);
                        }
                        0b10 => {
                            let b = self.read_reg(rm, pc);
                            self.write_reg(rdn, b);
                        }
                        _ => {
                            let target = self.read_reg(rm, pc);
                            if (op & (1 << 7)) != 0 {
                                // BLX Rm
                                self.r[14] = pc.wrapping_add(2) | 1;
                                self.r[15] = target & !1;
                            } else {
                                self.bx_target(mem, target);
                            }
                            return (2, false);
                        }
                    }
                    (1, false)
                } else if (op & 0xF800) == 0x4800 {
                    // LDR Rt, [PC, #imm8*4]
                    let rt = ((op >> 8) & 0x7) as usize;
                    let imm = ((op & 0xFF) as u32) << 2;
                    let base = (pc.wrapping_add(4)) & !3;
                    self.r[rt] = mem.read32(base.wrapping_add(imm));
                    (2, false)
                } else {
                    // Load/store register offset (0101)
                    let sub = (op >> 9) & 0x7;
                    let rm = ((op >> 6) & 0x7) as usize;
                    let rn = ((op >> 3) & 0x7) as usize;
                    let rt = (op & 0x7) as usize;
                    let addr = self.r[rn].wrapping_add(self.r[rm]);
                    match sub {
                        0b000 => mem.write32(addr, self.r[rt]),
                        0b001 => mem.write16(addr, self.r[rt] as u16),
                        0b010 => mem.write8(addr, self.r[rt] as u8),
                        0b011 => self.r[rt] = mem.read8(addr) as i8 as i32 as u32,
                        0b100 => self.r[rt] = mem.read32(addr),
                        0b101 => self.r[rt] = mem.read16(addr) as u32,
                        0b110 => self.r[rt] = mem.read8(addr) as u32,
                        _ => self.r[rt] = mem.read16(addr) as i16 as i32 as u32,
                    }
                    (2, false)
                }
            }
            0b011 => {
                let is_byte = (op & (1 << 12)) != 0;
                let is_load = (op & (1 << 11)) != 0;
                let imm5 = ((op >> 6) & 0x1F) as u32;
                let rn = ((op >> 3) & 0x7) as usize;
                let rt = (op & 0x7) as usize;
                let addr = self.r[rn].wrapping_add(if is_byte { imm5 } else { imm5 << 2 });
                match (is_byte, is_load) {
                    (false, false) => mem.write32(addr, self.r[rt]),
                    (false, true) => self.r[rt] = mem.read32(addr),
                    (true, false) => mem.write8(addr, self.r[rt] as u8),
                    (true, true) => self.r[rt] = mem.read8(addr) as u32,
                }
                (2, false)
            }
            0b100 => {
                if (op & (1 << 12)) == 0 {
                    // STRH / LDRH imm5*2
                    let is_load = (op & (1 << 11)) != 0;
                    let imm5 = (((op >> 6) & 0x1F) as u32) << 1;
                    let rn = ((op >> 3) & 0x7) as usize;
                    let rt = (op & 0x7) as usize;
                    let addr = self.r[rn].wrapping_add(imm5);
                    if is_load {
                        self.r[rt] = mem.read16(addr) as u32;
                    } else {
                        mem.write16(addr, self.r[rt] as u16);
                    }
                } else {
                    // STR / LDR [SP, #imm8*4]
                    let is_load = (op & (1 << 11)) != 0;
                    let rt = ((op >> 8) & 0x7) as usize;
                    let imm8 = ((op & 0xFF) as u32) << 2;
                    let addr = self.r[13].wrapping_add(imm8);
                    if is_load {
                        self.r[rt] = mem.read32(addr);
                    } else {
                        mem.write32(addr, self.r[rt]);
                    }
                }
                (2, false)
            }
            0b101 => {
                if (op & (1 << 12)) == 0 {
                    // ADR / ADD Rd, SP, #imm8*4
                    let rd = ((op >> 8) & 0x7) as usize;
                    let imm = ((op & 0xFF) as u32) << 2;
                    if (op & (1 << 11)) == 0 {
                        self.r[rd] = ((pc.wrapping_add(4)) & !3).wrapping_add(imm);
                    } else {
                        self.r[rd] = self.r[13].wrapping_add(imm);
                    }
                    (1, false)
                } else {
                    // Misc 16-bit (0xB000..0xBFFF)
                    self.exec_thumb16_misc(mem, pc, op)
                }
            }
            0b110 => {
                if (op & (1 << 12)) == 0 {
                    // STM / LDM
                    let is_load = (op & (1 << 11)) != 0;
                    let rn = ((op >> 8) & 0x7) as usize;
                    let list = op & 0xFF;
                    let mut addr = self.r[rn];
                    let count = list.count_ones();
                    if is_load {
                        for i in 0..8 {
                            if (list & (1 << i)) != 0 {
                                self.r[i] = mem.read32(addr);
                                addr = addr.wrapping_add(4);
                            }
                        }
                        if (list & (1 << rn)) == 0 {
                            self.r[rn] = self.r[rn].wrapping_add(count * 4);
                        }
                    } else {
                        for i in 0..8 {
                            if (list & (1 << i)) != 0 {
                                mem.write32(addr, self.r[i]);
                                addr = addr.wrapping_add(4);
                            }
                        }
                        self.r[rn] = self.r[rn].wrapping_add(count * 4);
                    }
                    (1 + count.max(1), false)
                } else {
                    // B<cond>, UDF, SVC
                    let cond = ((op >> 8) & 0xF) as u8;
                    if cond == 0xE {
                        self.halted_reason = HaltReason::Fault("Undefined 16-bit instruction".to_string());
                        return (1, false);
                    } else if cond == 0xF {
                        self.enter_exception(mem, 11); // SVCall
                        return (2, false);
                    }
                    if self.cond_passed(cond) {
                        let imm8 = (op & 0xFF) as i8 as i32;
                        let target = (pc.wrapping_add(4) as i32).wrapping_add(imm8 << 1) as u32;
                        self.r[15] = target & !1;
                        return (2, false);
                    }
                    (1, false)
                }
            }
            _ => {
                // Unconditional 16-bit B
                let imm11 = sign_extend((op & 0x7FF) as u32, 11) << 1;
                let target = (pc.wrapping_add(4) as i32).wrapping_add(imm11) as u32;
                self.r[15] = target & !1;
                (2, false)
            }
        }
    }

    fn exec_thumb16_misc(&mut self, mem: &mut Memory, pc: u32, op: u16) -> (u32, bool) {
        let sub = (op >> 8) & 0xF;
        match sub {
            0x0 => {
                let imm = ((op & 0x7F) as u32) << 2;
                let sp = self.r[13];
                if (op & 0x80) == 0 {
                    self.write_reg(13, sp.wrapping_add(imm));
                } else {
                    self.write_reg(13, sp.wrapping_sub(imm));
                }
                (1, false)
            }
            0x1 | 0x3 | 0x9 | 0xB => {
                // CBZ / CBNZ
                let nonzero = (op & (1 << 11)) != 0;
                let i = ((op >> 9) & 1) as u32;
                let imm5 = ((op >> 3) & 0x1F) as u32;
                let rn = (op & 0x7) as usize;
                let offset = ((i << 5) | imm5) << 1;
                let is_zero = self.r[rn] == 0;
                if is_zero != nonzero {
                    self.r[15] = pc.wrapping_add(4).wrapping_add(offset) & !1;
                    (2, false)
                } else {
                    (1, false)
                }
            }
            0x2 => {
                let kind = (op >> 6) & 0x3;
                let rm = ((op >> 3) & 0x7) as usize;
                let rd = (op & 0x7) as usize;
                let v = self.r[rm];
                self.r[rd] = match kind {
                    0b00 => (v as u16) as i16 as i32 as u32,
                    0b01 => (v as u8) as i8 as i32 as u32,
                    0b10 => v & 0xFFFF,
                    _ => v & 0xFF,
                };
                (1, false)
            }
            0x4 | 0x5 => {
                // PUSH {regs, LR}
                let list = op & 0xFF;
                let push_lr = (op & 0x100) != 0;
                let count = list.count_ones() + (push_lr as u32);
                let mut sp = self.r[13].wrapping_sub(count * 4);
                self.write_reg(13, sp);
                for i in 0..8 {
                    if (list & (1 << i)) != 0 {
                        mem.write32(sp, self.r[i]);
                        sp = sp.wrapping_add(4);
                    }
                }
                if push_lr {
                    mem.write32(sp, self.r[14]);
                }
                (1 + count, false)
            }
            0x6 => {
                if (op & 0xFFF0) == 0xB660 {
                    // CPSIE / CPSID
                    let disable = (op & 0x10) != 0;
                    if (op & 0x2) != 0 {
                        self.primask = if disable { 1 } else { 0 };
                    }
                }
                (1, false)
            }
            0xA => {
                let kind = (op >> 6) & 0x3;
                let rm = ((op >> 3) & 0x7) as usize;
                let rd = (op & 0x7) as usize;
                let v = self.r[rm];
                self.r[rd] = match kind {
                    0b00 => v.swap_bytes(),
                    0b01 => {
                        let lo = (v as u16).swap_bytes() as u32;
                        let hi = ((v >> 16) as u16).swap_bytes() as u32;
                        (hi << 16) | lo
                    }
                    _ => ((v as u16).swap_bytes() as i16) as i32 as u32,
                };
                (1, false)
            }
            0xC | 0xD => {
                // POP {regs, PC}
                let list = op & 0xFF;
                let pop_pc = (op & 0x100) != 0;
                let count = list.count_ones() + (pop_pc as u32);
                let mut sp = self.r[13];
                for i in 0..8 {
                    if (list & (1 << i)) != 0 {
                        self.r[i] = mem.read32(sp);
                        sp = sp.wrapping_add(4);
                    }
                }
                let next_pc = if pop_pc {
                    let v = mem.read32(sp);
                    sp = sp.wrapping_add(4);
                    Some(v)
                } else {
                    None
                };
                self.write_reg(13, sp);
                if let Some(target) = next_pc {
                    self.bx_target(mem, target);
                }
                (1 + count, false)
            }
            0xE => {
                self.halted_reason = HaltReason::Bkpt((op & 0xFF) as u8);
                (1, false)
            }
            0xF => {
                let mask = (op & 0xF) as u8;
                if mask != 0 {
                    self.set_itstate((op & 0xFF) as u8);
                    return (1, true);
                }
                (1, false)
            }
            _ => (1, false),
        }
    }

    fn exec_thumb32(&mut self, mem: &mut Memory, pc: u32, hw1: u16, hw2: u16) -> u32 {
        let op1 = (hw1 >> 11) & 0x3;
        let op2 = (hw1 >> 4) & 0x7F;
        let op = (hw2 >> 15) & 1;

        match (op1, op2, op) {
            // 11101 | Data processing (shifted register)
            (0b01, x, _) if (x & 0x60) == 0x20 => {
                self.exec_t32_dp_shifted_reg(pc, hw1, hw2)
            }
            // 11101 | Load/store multiple (STM, LDM, PUSH.W, POP.W)
            (0b01, x, _) if (x & 0x64) == 0x00 => {
                self.exec_t32_ldm_stm(mem, hw1, hw2)
            }
            // 11101 | Load/store dual / TBB / TBH / STREX / LDREX
            (0b01, x, _) if (x & 0x64) == 0x04 => {
                self.exec_t32_ldst_dual_tb(mem, pc, hw1, hw2)
            }
            // 11110 | Branches & misc control (op == 1)
            (0b10, _, 1) => self.exec_t32_branch_misc(mem, pc, hw1, hw2),
            // 11110 | Data processing modified immediate (op2 bit5 == 0)
            (0b10, x, 0) if (x & 0x20) == 0 => {
                self.exec_t32_dp_mod_imm(hw1, hw2)
            }
            // 11110 | Data processing plain binary immediate (op2 bit5 == 1)
            (0b10, x, 0) if (x & 0x20) != 0 => {
                self.exec_t32_dp_plain_imm(pc, hw1, hw2)
            }
            // 11111 | Load/store single data item
            (0b11, x, _) if (x & 0x60) == 0x00 => {
                self.exec_t32_ldst_single(mem, pc, hw1, hw2)
            }
            // 11111 | Data processing (register): shifts, sign/zero extend, CLZ, RBIT, REV
            (0b11, x, _) if (x & 0x70) == 0x20 => {
                self.exec_t32_dp_reg(hw1, hw2)
            }
            // 11111 | Multiply & Divide
            (0b11, x, _) if (x & 0x70) == 0x30 => {
                self.exec_t32_mul_div(hw1, hw2)
            }
            _ => 1,
        }
    }

    fn exec_t32_dp_mod_imm(&mut self, hw1: u16, hw2: u16) -> u32 {
        let op = ((hw1 >> 5) & 0xF) as u8;
        let s = ((hw1 >> 4) & 1) != 0;
        let rn = (hw1 & 0xF) as usize;
        let rd = ((hw2 >> 8) & 0xF) as usize;
        let i = ((hw1 >> 10) & 1) as u32;
        let imm3 = ((hw2 >> 12) & 0x7) as u32;
        let imm8 = (hw2 & 0xFF) as u32;
        let imm12 = (i << 11) | (imm3 << 8) | imm8;
        let (imm32, carry_imm) = thumb_expand_imm_c(imm12, self.flag_c());
        let rn_val = if rn == 15 { 0 } else { self.r[rn] };

        match op {
            0x0 => {
                // AND / TST
                let res = rn_val & imm32;
                if rd != 15 { self.write_reg(rd, res); }
                if s { self.set_nzc(res, carry_imm); }
            }
            0x1 => {
                // BIC
                let res = rn_val & !imm32;
                self.write_reg(rd, res);
                if s { self.set_nzc(res, carry_imm); }
            }
            0x2 => {
                // ORR / MOV.W
                let res = if rn == 15 { imm32 } else { rn_val | imm32 };
                self.write_reg(rd, res);
                if s { self.set_nzc(res, carry_imm); }
            }
            0x3 => {
                // ORN / MVN.W
                let res = if rn == 15 { !imm32 } else { rn_val | !imm32 };
                self.write_reg(rd, res);
                if s { self.set_nzc(res, carry_imm); }
            }
            0x4 => {
                // EOR / TEQ
                let res = rn_val ^ imm32;
                if rd != 15 { self.write_reg(rd, res); }
                if s { self.set_nzc(res, carry_imm); }
            }
            0x8 => {
                // ADD / CMN
                let (res, c, v) = add_with_carry(rn_val, imm32, false);
                if rd != 15 { self.write_reg(rd, res); }
                if s { self.set_nzcv(res, c, v); }
            }
            0xA => {
                // ADC
                let (res, c, v) = add_with_carry(rn_val, imm32, self.flag_c());
                self.write_reg(rd, res);
                if s { self.set_nzcv(res, c, v); }
            }
            0xB => {
                // SBC
                let (res, c, v) = add_with_carry(rn_val, !imm32, self.flag_c());
                self.write_reg(rd, res);
                if s { self.set_nzcv(res, c, v); }
            }
            0xD => {
                // SUB / CMP
                let (res, c, v) = add_with_carry(rn_val, !imm32, true);
                if rd != 15 { self.write_reg(rd, res); }
                if s { self.set_nzcv(res, c, v); }
            }
            0xE => {
                // RSB
                let (res, c, v) = add_with_carry(!rn_val, imm32, true);
                self.write_reg(rd, res);
                if s { self.set_nzcv(res, c, v); }
            }
            _ => {}
        }
        1
    }

    fn exec_t32_dp_plain_imm(&mut self, pc: u32, hw1: u16, hw2: u16) -> u32 {
        let op = ((hw1 >> 4) & 0x1F) as u8;
        let rn = (hw1 & 0xF) as usize;
        let rd = ((hw2 >> 8) & 0xF) as usize;
        let i = ((hw1 >> 10) & 1) as u32;
        let imm3 = ((hw2 >> 12) & 0x7) as u32;
        let imm8 = (hw2 & 0xFF) as u32;
        let imm12 = (i << 11) | (imm3 << 8) | imm8;

        match op {
            0x00 => {
                // ADDW / ADR (if rn == 15)
                let base = if rn == 15 { (pc.wrapping_add(4)) & !3 } else { self.r[rn] };
                self.write_reg(rd, base.wrapping_add(imm12));
            }
            0x04 => {
                // MOVW
                let imm4 = (hw1 & 0xF) as u32;
                let imm16 = (imm4 << 12) | imm12;
                self.write_reg(rd, imm16);
            }
            0x0A => {
                // SUBW / ADR.W (sub)
                let base = if rn == 15 { (pc.wrapping_add(4)) & !3 } else { self.r[rn] };
                self.write_reg(rd, base.wrapping_sub(imm12));
            }
            0x0C => {
                // MOVT
                let imm4 = (hw1 & 0xF) as u32;
                let imm16 = (imm4 << 12) | imm12;
                let cur = self.r[rd] & 0xFFFF;
                self.write_reg(rd, (imm16 << 16) | cur);
            }
            0x14 => {
                // SBFX
                let lsb = ((imm3 << 2) | ((hw2 >> 6) & 0x3) as u32) & 0x1F;
                let width = ((hw2 & 0x1F) as u32) + 1;
                let val = (self.r[rn] >> lsb) & mask_bits(width);
                self.write_reg(rd, sign_extend(val, width) as u32);
            }
            0x16 => {
                // BFI / BFC
                let lsb = ((imm3 << 2) | ((hw2 >> 6) & 0x3) as u32) & 0x1F;
                let msb = (hw2 & 0x1F) as u32;
                if msb >= lsb {
                    let width = msb - lsb + 1;
                    let mask = mask_bits(width) << lsb;
                    let src = if rn == 15 { 0 } else { (self.r[rn] & mask_bits(width)) << lsb };
                    self.write_reg(rd, (self.r[rd] & !mask) | src);
                }
            }
            0x1C => {
                // UBFX
                let lsb = ((imm3 << 2) | ((hw2 >> 6) & 0x3) as u32) & 0x1F;
                let width = ((hw2 & 0x1F) as u32) + 1;
                let val = (self.r[rn] >> lsb) & mask_bits(width);
                self.write_reg(rd, val);
            }
            _ => {}
        }
        1
    }

    fn exec_t32_dp_shifted_reg(&mut self, pc: u32, hw1: u16, hw2: u16) -> u32 {
        let op = ((hw1 >> 5) & 0xF) as u8;
        let s = ((hw1 >> 4) & 1) != 0;
        let rn = (hw1 & 0xF) as usize;
        let rd = ((hw2 >> 8) & 0xF) as usize;
        let rm = (hw2 & 0xF) as usize;
        let stype = ((hw2 >> 4) & 0x3) as u8;
        let imm3 = ((hw2 >> 12) & 0x7) as u32;
        let imm2 = ((hw2 >> 6) & 0x3) as u32;
        let imm5 = (imm3 << 2) | imm2;

        let (shifted, carry_s) = if stype == 3 && imm5 == 0 {
            let rm_val = self.read_reg(rm, pc);
            let res = ((self.flag_c() as u32) << 31) | (rm_val >> 1);
            (res, (rm_val & 1) != 0)
        } else {
            let shift_n = match stype {
                1 | 2 if imm5 == 0 => 32,
                _ => imm5,
            };
            let rm_val = self.read_reg(rm, pc);
            shift_c(rm_val, stype, shift_n, self.flag_c())
        };
        let rn_val = if rn == 15 { 0 } else { self.read_reg(rn, pc) };

        match op {
            0x0 => {
                let res = rn_val & shifted;
                if rd != 15 { self.write_reg(rd, res); }
                if s { self.set_nzc(res, carry_s); }
            }
            0x1 => {
                let res = rn_val & !shifted;
                self.write_reg(rd, res);
                if s { self.set_nzc(res, carry_s); }
            }
            0x2 => {
                // ORR.W / MOV.W (if rn==15) / LSL/LSR/ASR/ROR.W
                let res = if rn == 15 { shifted } else { rn_val | shifted };
                self.write_reg(rd, res);
                if s { self.set_nzc(res, carry_s); }
            }
            0x3 => {
                let res = if rn == 15 { !shifted } else { rn_val | !shifted };
                self.write_reg(rd, res);
                if s { self.set_nzc(res, carry_s); }
            }
            0x4 => {
                let res = rn_val ^ shifted;
                if rd != 15 { self.write_reg(rd, res); }
                if s { self.set_nzc(res, carry_s); }
            }
            0x8 => {
                let (res, c, v) = add_with_carry(rn_val, shifted, false);
                if rd != 15 { self.write_reg(rd, res); }
                if s { self.set_nzcv(res, c, v); }
            }
            0xA => {
                let (res, c, v) = add_with_carry(rn_val, shifted, self.flag_c());
                self.write_reg(rd, res);
                if s { self.set_nzcv(res, c, v); }
            }
            0xB => {
                let (res, c, v) = add_with_carry(rn_val, !shifted, self.flag_c());
                self.write_reg(rd, res);
                if s { self.set_nzcv(res, c, v); }
            }
            0xD => {
                let (res, c, v) = add_with_carry(rn_val, !shifted, true);
                if rd != 15 { self.write_reg(rd, res); }
                if s { self.set_nzcv(res, c, v); }
            }
            0xE => {
                let (res, c, v) = add_with_carry(!rn_val, shifted, true);
                self.write_reg(rd, res);
                if s { self.set_nzcv(res, c, v); }
            }
            _ => {}
        }
        1
    }

    fn exec_t32_dp_reg(&mut self, hw1: u16, hw2: u16) -> u32 {
        let op1 = ((hw1 >> 4) & 0xF) as u8;
        let op2 = ((hw2 >> 4) & 0xF) as u8;
        let rn = (hw1 & 0xF) as usize;
        let rd = ((hw2 >> 8) & 0xF) as usize;
        let rm = (hw2 & 0xF) as usize;
        let s = (hw1 & (1 << 4)) != 0;

        if (op2 & 0x8) == 0 && op1 <= 0x7 {
            // LSL.W / LSR.W / ASR.W / ROR.W (register)
            let stype = (op1 >> 1) & 0x3;
            let (res, c) = shift_c(self.r[rn], stype, self.r[rm] & 0xFF, self.flag_c());
            self.write_reg(rd, res);
            if s { self.set_nzc(res, c); }
            return 1;
        }

        if (op2 & 0x8) != 0 && op1 <= 0x5 {
            // SXTH / UXTH / SXTB / UXTB (.W)
            let rot = (((hw2 >> 4) & 0x3) * 8) as u32;
            let rotated = self.r[rm].rotate_right(rot);
            let extended = match op1 & 0x7 {
                0x0 => (rotated as u16) as i16 as i32 as u32,
                0x1 => rotated & 0xFFFF,
                0x4 => (rotated as u8) as i8 as i32 as u32,
                0x5 => rotated & 0xFF,
                _ => rotated,
            };
            let res = if rn == 15 { extended } else { self.r[rn].wrapping_add(extended) };
            self.write_reg(rd, res);
            return 1;
        }

        if (hw1 & 0x0FF0) == 0x0A90 && (op2 & 0xC) == 0x8 {
            // REV / REV16 / RBIT / REVSH
            let v = self.r[rm];
            let res = match (hw2 >> 4) & 0x3 {
                0x0 => v.swap_bytes(),
                0x1 => {
                    let lo = (v as u16).swap_bytes() as u32;
                    let hi = ((v >> 16) as u16).swap_bytes() as u32;
                    (hi << 16) | lo
                }
                0x2 => v.reverse_bits(),
                _ => ((v as u16).swap_bytes() as i16) as i32 as u32,
            };
            self.write_reg(rd, res);
            return 1;
        }

        if (hw1 & 0x0FF0) == 0x0AB0 {
            // CLZ
            self.write_reg(rd, self.r[rm].leading_zeros());
            return 1;
        }
        1
    }

    fn exec_t32_mul_div(&mut self, hw1: u16, hw2: u16) -> u32 {
        let op1 = ((hw1 >> 4) & 0x7) as u8;
        let op2 = ((hw2 >> 4) & 0x3) as u8;
        let rn = (hw1 & 0xF) as usize;
        let ra = ((hw2 >> 12) & 0xF) as usize;
        let rd = ((hw2 >> 8) & 0xF) as usize;
        let rm = (hw2 & 0xF) as usize;

        if (hw1 & 0x0080) == 0 {
            // 32-bit result multiply: MUL, MLA, MLS
            match (op1, op2) {
                (0, 0) => {
                    let prod = self.r[rn].wrapping_mul(self.r[rm]);
                    let res = if ra == 15 { prod } else { prod.wrapping_add(self.r[ra]) };
                    self.write_reg(rd, res);
                }
                (0, 1) => {
                    let prod = self.r[rn].wrapping_mul(self.r[rm]);
                    self.write_reg(rd, self.r[ra].wrapping_sub(prod));
                }
                _ => {}
            }
            1
        } else {
            // 64-bit multiply or divide
            match (op1, (hw2 >> 4) & 0xF) {
                (0b001, 0xF) => {
                    // SDIV
                    let d = self.r[rm] as i32;
                    let q = if d == 0 { 0 } else { (self.r[rn] as i32).wrapping_div(d) };
                    self.write_reg(rd, q as u32);
                }
                (0b011, 0xF) => {
                    // UDIV
                    let d = self.r[rm];
                    let q = if d == 0 { 0 } else { self.r[rn] / d };
                    self.write_reg(rd, q);
                }
                (0b000, 0x0) => {
                    // SMULL
                    let p = (self.r[rn] as i32 as i64).wrapping_mul(self.r[rm] as i32 as i64) as u64;
                    self.write_reg(ra, p as u32);
                    self.write_reg(rd, (p >> 32) as u32);
                }
                (0b010, 0x0) => {
                    // UMULL
                    let p = (self.r[rn] as u64).wrapping_mul(self.r[rm] as u64);
                    self.write_reg(ra, p as u32);
                    self.write_reg(rd, (p >> 32) as u32);
                }
                (0b100, 0x0) => {
                    // SMLAL
                    let acc = (((self.r[rd] as u64) << 32) | (self.r[ra] as u64)) as i64;
                    let p = (self.r[rn] as i32 as i64).wrapping_mul(self.r[rm] as i32 as i64);
                    let sum = acc.wrapping_add(p) as u64;
                    self.write_reg(ra, sum as u32);
                    self.write_reg(rd, (sum >> 32) as u32);
                }
                (0b110, 0x0) => {
                    // UMLAL
                    let acc = ((self.r[rd] as u64) << 32) | (self.r[ra] as u64);
                    let p = (self.r[rn] as u64).wrapping_mul(self.r[rm] as u64);
                    let sum = acc.wrapping_add(p);
                    self.write_reg(ra, sum as u32);
                    self.write_reg(rd, (sum >> 32) as u32);
                }
                _ => {}
            }
            2
        }
    }

    fn exec_t32_ldm_stm(&mut self, mem: &mut Memory, hw1: u16, hw2: u16) -> u32 {
        let op = ((hw1 >> 7) & 0x3) as u8;
        let is_load = (hw1 & (1 << 4)) != 0;
        let wback = (hw1 & (1 << 5)) != 0;
        let rn = (hw1 & 0xF) as usize;
        let count = hw2.count_ones();

        // op == 1: LDMIA/STMIA, op == 2: LDMDB/STMDB (PUSH.W is STMDB SP!)
        let base = self.r[rn];
        let mut addr = if op == 2 {
            base.wrapping_sub(count * 4)
        } else {
            base
        };
        let wb_addr = if op == 2 { addr } else { base.wrapping_add(count * 4) };

        let mut new_pc: Option<u32> = None;
        for i in 0..16 {
            if (hw2 & (1 << i)) != 0 {
                if is_load {
                    let val = mem.read32(addr);
                    if i == 15 {
                        new_pc = Some(val);
                    } else {
                        self.write_reg(i, val);
                    }
                } else {
                    mem.write32(addr, self.r[i]);
                }
                addr = addr.wrapping_add(4);
            }
        }
        if wback && !(is_load && (hw2 & (1 << rn)) != 0) {
            self.write_reg(rn, wb_addr);
        }
        if let Some(target) = new_pc {
            self.bx_target(mem, target);
        }
        1 + count
    }

    fn exec_t32_ldst_dual_tb(&mut self, mem: &mut Memory, pc: u32, hw1: u16, hw2: u16) -> u32 {
        // LDRD / STRD or TBB / TBH
        let p = (hw1 & (1 << 8)) != 0;
        let u = (hw1 & (1 << 7)) != 0;
        let w = (hw1 & (1 << 5)) != 0;
        let is_load = (hw1 & (1 << 4)) != 0;
        let rn = (hw1 & 0xF) as usize;

        if (hw1 & 0x0FF0) == 0x08D0 && (hw2 & 0x00E0) == 0x0000 {
            // TBB / TBH
            let is_h = (hw2 & (1 << 4)) != 0;
            let rm = (hw2 & 0xF) as usize;
            let base = if rn == 15 { pc.wrapping_add(4) } else { self.r[rn] };
            let idx = self.r[rm];
            let half = if is_h {
                mem.read16(base.wrapping_add(idx * 2)) as u32
            } else {
                mem.read8(base.wrapping_add(idx)) as u32
            };
            self.r[15] = pc.wrapping_add(4).wrapping_add(half * 2) & !1;
            return 3;
        }

        if (hw1 & (1 << 7)) != 0 || p || w {
            let rt = ((hw2 >> 12) & 0xF) as usize;
            let rt2 = ((hw2 >> 8) & 0xF) as usize;
            let imm8 = ((hw2 & 0xFF) as u32) << 2;
            let base = if rn == 15 { (pc.wrapping_add(4)) & !3 } else { self.r[rn] };
            let off_addr = if u { base.wrapping_add(imm8) } else { base.wrapping_sub(imm8) };
            let addr = if p { off_addr } else { base };
            if is_load {
                self.write_reg(rt, mem.read32(addr));
                self.write_reg(rt2, mem.read32(addr.wrapping_add(4)));
            } else {
                mem.write32(addr, self.r[rt]);
                mem.write32(addr.wrapping_add(4), self.r[rt2]);
            }
            if w && rn != 15 {
                self.write_reg(rn, off_addr);
            }
            return 3;
        }
        1
    }

    fn exec_t32_ldst_single(&mut self, mem: &mut Memory, pc: u32, hw1: u16, hw2: u16) -> u32 {
        let size = (hw1 >> 5) & 0x3; // 0=B, 1=H, 2=W
        let is_signed = (hw1 & (1 << 8)) != 0;
        let is_load = (hw1 & (1 << 4)) != 0;
        let rn = (hw1 & 0xF) as usize;
        let rt = ((hw2 >> 12) & 0xF) as usize;

        let addr = if rn == 15 {
            let base = (pc.wrapping_add(4)) & !3;
            let imm12 = (hw2 & 0xFFF) as u32;
            if (hw1 & (1 << 7)) != 0 {
                base.wrapping_add(imm12)
            } else {
                base.wrapping_sub(imm12)
            }
        } else if (hw1 & (1 << 7)) != 0 {
            // 12-bit positive immediate offset
            self.r[rn].wrapping_add((hw2 & 0xFFF) as u32)
        } else if (hw2 & (1 << 11)) != 0 {
            // 8-bit immediate with P/U/W
            let p = (hw2 & (1 << 10)) != 0;
            let u = (hw2 & (1 << 9)) != 0;
            let w = (hw2 & (1 << 8)) != 0;
            let imm8 = (hw2 & 0xFF) as u32;
            let base = self.r[rn];
            let off_addr = if u { base.wrapping_add(imm8) } else { base.wrapping_sub(imm8) };
            let eff = if p { off_addr } else { base };
            if w {
                self.write_reg(rn, off_addr);
            }
            eff
        } else {
            // Shifted register offset: [Rn, Rm, LSL #imm2]
            let rm = (hw2 & 0xF) as usize;
            let shift = ((hw2 >> 4) & 0x3) as u32;
            self.r[rn].wrapping_add(self.r[rm] << shift)
        };

        if is_load {
            let val = match (size, is_signed) {
                (0, false) => mem.read8(addr) as u32,
                (0, true) => mem.read8(addr) as i8 as i32 as u32,
                (1, false) => mem.read16(addr) as u32,
                (1, true) => mem.read16(addr) as i16 as i32 as u32,
                _ => mem.read32(addr),
            };
            if rt == 15 {
                self.bx_target(mem, val);
            } else {
                self.write_reg(rt, val);
            }
        } else {
            match size {
                0 => mem.write8(addr, self.r[rt] as u8),
                1 => mem.write16(addr, self.r[rt] as u16),
                _ => mem.write32(addr, self.r[rt]),
            }
        }
        2
    }

    fn exec_t32_branch_misc(&mut self, mem: &mut Memory, pc: u32, hw1: u16, hw2: u16) -> u32 {
        let s = ((hw1 >> 10) & 1) as u32;
        let j1 = ((hw2 >> 13) & 1) as u32;
        let j2 = ((hw2 >> 11) & 1) as u32;
        let op2 = (hw2 >> 12) & 0x7;

        if (op2 & 0x5) == 0 {
            let cond_bits = ((hw1 >> 6) & 0xF) as u8;
            if (cond_bits & 0xE) != 0xE {
                // Conditional B.W
                if self.cond_passed(cond_bits) {
                    let imm6 = (hw1 & 0x3F) as u32;
                    let imm11 = (hw2 & 0x7FF) as u32;
                    let raw = (s << 20) | (j2 << 19) | (j1 << 18) | (imm6 << 12) | (imm11 << 1);
                    let off = sign_extend(raw, 21);
                    self.r[15] = (pc.wrapping_add(4) as i32).wrapping_add(off) as u32 & !1;
                    return 2;
                }
                return 1;
            } else {
                // Misc control: MSR, MRS, NOP/DSB/DMB/ISB
                let op_ctrl = (hw1 >> 4) & 0x7F;
                if op_ctrl == 0x38 || op_ctrl == 0x39 {
                    let sysm = (hw2 & 0xFF) as u8;
                    let rn = (hw1 & 0xF) as usize;
                    let v = self.r[rn];
                    match sysm {
                        8 => { self.msp = v & !3; if (self.control & 2) == 0 { self.r[13] = self.msp; } }
                        9 => { self.psp = v & !3; if (self.control & 2) != 0 { self.r[13] = self.psp; } }
                        16 => self.primask = v & 1,
                        20 => self.control = v & 3,
                        _ => {}
                    }
                } else if op_ctrl == 0x3E || op_ctrl == 0x3F {
                    let sysm = (hw2 & 0xFF) as u8;
                    let rd = ((hw2 >> 8) & 0xF) as usize;
                    let v = match sysm {
                        0..=3 => self.xpsr & 0xF800_0000,
                        8 => self.msp,
                        9 => self.psp,
                        16 => self.primask,
                        20 => self.control,
                        _ => 0,
                    };
                    self.write_reg(rd, v);
                }
                return 1;
            }
        }

        // Unconditional B.W ((op2 & 5) == 1) or BL ((op2 & 5) == 5)
        let i1 = !(j1 ^ s) & 1;
        let i2 = !(j2 ^ s) & 1;
        let imm10 = (hw1 & 0x3FF) as u32;
        let imm11 = (hw2 & 0x7FF) as u32;
        let raw = (s << 24) | (i1 << 23) | (i2 << 22) | (imm10 << 12) | (imm11 << 1);
        let off = sign_extend(raw, 25);
        let target = (pc.wrapping_add(4) as i32).wrapping_add(off) as u32 & !1;
        if (op2 & 4) != 0 {
            // BL
            self.r[14] = pc.wrapping_add(4) | 1;
            let _ = mem;
        }
        self.r[15] = target;
        2
    }
}

#[inline]
pub fn is_thumb32(hw1: u16) -> bool {
    let top5 = hw1 >> 11;
    top5 == 0b11101 || top5 == 0b11110 || top5 == 0b11111
}

#[inline]
fn sign_extend(val: u32, bits: u32) -> i32 {
    let shift = 32 - bits;
    ((val << shift) as i32) >> shift
}

#[inline]
fn mask_bits(width: u32) -> u32 {
    if width >= 32 {
        u32::MAX
    } else {
        (1u32 << width) - 1
    }
}

#[inline]
fn add_with_carry(x: u32, y: u32, carry_in: bool) -> (u32, bool, bool) {
    let cin = carry_in as u64;
    let usum = (x as u64) + (y as u64) + cin;
    let ssum = (x as i32 as i64) + (y as i32 as i64) + (cin as i64);
    let res = usum as u32;
    let carry = (res as u64) != usum;
    let overflow = (res as i32 as i64) != ssum;
    (res, carry, overflow)
}

#[inline]
fn shift_c(val: u32, stype: u8, amount: u32, carry_in: bool) -> (u32, bool) {
    if amount == 0 {
        if stype == 3 {
            // RRX (when shift_n==0 in decoded shifted reg with stype==3)
            // Note: callers pass 0 for ROR #0; handled by caller if RRX needed
        }
        return (val, carry_in);
    }
    match stype {
        0 => {
            // LSL
            if amount < 32 {
                (val << amount, ((val >> (32 - amount)) & 1) != 0)
            } else if amount == 32 {
                (0, (val & 1) != 0)
            } else {
                (0, false)
            }
        }
        1 => {
            // LSR
            if amount < 32 {
                (val >> amount, ((val >> (amount - 1)) & 1) != 0)
            } else if amount == 32 {
                (0, (val >> 31) != 0)
            } else {
                (0, false)
            }
        }
        2 => {
            // ASR
            if amount < 32 {
                (((val as i32) >> amount) as u32, ((val >> (amount - 1)) & 1) != 0)
            } else {
                let s = (val as i32) >> 31;
                (s as u32, s != 0)
            }
        }
        _ => {
            // ROR
            let m = amount % 32;
            let res = if m == 0 { val } else { val.rotate_right(m) };
            (res, (res >> 31) != 0)
        }
    }
}

#[inline]
pub fn thumb_expand_imm_c(imm12: u32, carry_in: bool) -> (u32, bool) {
    let top2 = (imm12 >> 10) & 0x3;
    if top2 == 0 {
        let byte = imm12 & 0xFF;
        let res = match (imm12 >> 8) & 0x3 {
            0 => byte,
            1 => (byte << 16) | byte,
            2 => (byte << 24) | (byte << 8),
            _ => (byte << 24) | (byte << 16) | (byte << 8) | byte,
        };
        (res, carry_in)
    } else {
        let unrotated = 0x80 | (imm12 & 0x7F);
        let rot = (imm12 >> 7) & 0x1F;
        let res = unrotated.rotate_right(rot);
        (res, (res >> 31) != 0)
    }
}
