//! ARMv7-M (Cortex-M3) Thumb / Thumb-2 instruction decoder.
//!
//! One decoder feeds both the simulator (`sim::cpu`) and the disassembler (`disasm`), so the
//! two can never disagree about what a bit pattern means. Encodings follow the ARMv7-M
//! Architecture Reference Manual (DDI 0403E), chapter A5 / A7. Anything that is UNDEFINED on
//! a Cortex-M3 (DSP / v7E-M extensions, coprocessor space, BLX immediate, ...) decodes to
//! [`Instr::Undefined`] or [`Instr::Coproc`] so the simulator raises a UsageFault exactly
//! where the silicon would.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SRType {
    Lsl,
    Lsr,
    Asr,
    Ror,
    Rrx,
}

impl SRType {
    pub fn from_bits(b: u32) -> SRType {
        match b & 3 {
            0 => SRType::Lsl,
            1 => SRType::Lsr,
            2 => SRType::Asr,
            _ => SRType::Ror,
        }
    }
    pub fn bits(self) -> u32 {
        match self {
            SRType::Lsl => 0,
            SRType::Lsr => 1,
            SRType::Asr => 2,
            SRType::Ror | SRType::Rrx => 3,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            SRType::Lsl => "LSL",
            SRType::Lsr => "LSR",
            SRType::Asr => "ASR",
            SRType::Ror => "ROR",
            SRType::Rrx => "RRX",
        }
    }
}

/// A decoded immediate shift (DecodeImmShift). `n == 0` with `Lsl` means "no shift".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shift {
    pub t: SRType,
    pub n: u8,
}

impl Shift {
    pub const NONE: Shift = Shift { t: SRType::Lsl, n: 0 };
    /// DecodeImmShift(type, imm5).
    pub fn decode(ty: u32, imm5: u32) -> Shift {
        match ty & 3 {
            0 => Shift { t: SRType::Lsl, n: imm5 as u8 },
            1 => Shift { t: SRType::Lsr, n: if imm5 == 0 { 32 } else { imm5 as u8 } },
            2 => Shift { t: SRType::Asr, n: if imm5 == 0 { 32 } else { imm5 as u8 } },
            _ => {
                if imm5 == 0 {
                    Shift { t: SRType::Rrx, n: 1 }
                } else {
                    Shift { t: SRType::Ror, n: imm5 as u8 }
                }
            }
        }
    }
    pub fn is_none(self) -> bool {
        self.t == SRType::Lsl && self.n == 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alu {
    And,
    Bic,
    Orr,
    Orn,
    Eor,
    Add,
    Adc,
    Sbc,
    Sub,
    Rsb,
    Mov,
    Mvn,
    Tst,
    Teq,
    Cmp,
    Cmn,
}

impl Alu {
    pub fn name(self) -> &'static str {
        match self {
            Alu::And => "AND",
            Alu::Bic => "BIC",
            Alu::Orr => "ORR",
            Alu::Orn => "ORN",
            Alu::Eor => "EOR",
            Alu::Add => "ADD",
            Alu::Adc => "ADC",
            Alu::Sbc => "SBC",
            Alu::Sub => "SUB",
            Alu::Rsb => "RSB",
            Alu::Mov => "MOV",
            Alu::Mvn => "MVN",
            Alu::Tst => "TST",
            Alu::Teq => "TEQ",
            Alu::Cmp => "CMP",
            Alu::Cmn => "CMN",
        }
    }
    /// Comparison-style ops write no register and always set flags.
    pub fn is_compare(self) -> bool {
        matches!(self, Alu::Tst | Alu::Teq | Alu::Cmp | Alu::Cmn)
    }
    /// Logical ops take C from the shifter / immediate expansion instead of the adder.
    pub fn is_logical(self) -> bool {
        matches!(
            self,
            Alu::And | Alu::Bic | Alu::Orr | Alu::Orn | Alu::Eor | Alu::Mov | Alu::Mvn | Alu::Tst | Alu::Teq
        )
    }
    /// Has a first operand register (Rn).
    pub fn has_rn(self) -> bool {
        !matches!(self, Alu::Mov | Alu::Mvn)
    }
    /// The 4-bit opcode used by the 32-bit data-processing encodings.
    pub fn t32_op(self) -> u32 {
        match self {
            Alu::And | Alu::Tst => 0x0,
            Alu::Bic => 0x1,
            Alu::Orr | Alu::Mov => 0x2,
            Alu::Orn | Alu::Mvn => 0x3,
            Alu::Eor | Alu::Teq => 0x4,
            Alu::Add | Alu::Cmn => 0x8,
            Alu::Adc => 0xA,
            Alu::Sbc => 0xB,
            Alu::Sub | Alu::Cmp => 0xD,
            Alu::Rsb => 0xE,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    B,
    H,
    W,
}

impl Size {
    pub fn bytes(self) -> u32 {
        match self {
            Size::B => 1,
            Size::H => 2,
            Size::W => 4,
        }
    }
}

/// Addressing mode of a single load/store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Addr {
    /// `[Rn, #+/-imm]`, `[Rn, #+/-imm]!` or `[Rn], #+/-imm`.
    Imm { rn: u8, imm: u32, add: bool, index: bool, wback: bool },
    /// `[Rn, Rm{, LSL #n}]`.
    Reg { rn: u8, rm: u8, lsl: u8 },
    /// PC-relative literal: `[PC, #+/-imm]` with Align(PC, 4).
    Lit { imm: u32, add: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtKind {
    Sxtb,
    Sxth,
    Uxtb,
    Uxth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevKind {
    Rev,
    Rev16,
    Revsh,
    Rbit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LongMul {
    Smull,
    Umull,
    Smlal,
    Umlal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HintKind {
    Nop,
    Yield,
    Wfe,
    Wfi,
    Sev,
    Dbg(u8),
    /// Unallocated hint space: executes as NOP.
    Other(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Barrier {
    Dsb,
    Dmb,
    Isb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Instr {
    /// Data processing with an immediate. `carry` is the ThumbExpandImm_C carry-out when the
    /// expansion is a rotation (`None` = carry unchanged).
    AluImm { op: Alu, s: bool, rd: u8, rn: u8, imm: u32, carry: Option<bool> },
    /// Data processing with an (optionally immediate-shifted) register.
    AluReg { op: Alu, s: bool, rd: u8, rn: u8, rm: u8, shift: Shift },
    /// LSL/LSR/ASR/ROR by register: Rd = Rn <shift> Rm[7:0].
    ShiftReg { t: SRType, s: bool, rd: u8, rn: u8, rm: u8 },
    /// ADDW / SUBW (12-bit plain immediate, no flags), incl. `ADD Rd, SP, #imm12`.
    AddW { sub: bool, rd: u8, rn: u8, imm: u32 },
    /// ADR: Rd = Align(PC,4) +/- imm.
    Adr { rd: u8, imm: u32, add: bool },
    Movw { rd: u8, imm: u16 },
    Movt { rd: u8, imm: u16 },
    Mul { s: bool, rd: u8, rn: u8, rm: u8 },
    Mla { rd: u8, rn: u8, rm: u8, ra: u8 },
    Mls { rd: u8, rn: u8, rm: u8, ra: u8 },
    LongMul { kind: LongMul, rdlo: u8, rdhi: u8, rn: u8, rm: u8 },
    Div { signed: bool, rd: u8, rn: u8, rm: u8 },
    Extend { kind: ExtKind, rd: u8, rm: u8, rot: u8 },
    Rev { kind: RevKind, rd: u8, rm: u8 },
    Clz { rd: u8, rm: u8 },
    /// SBFX / UBFX.
    Bfx { signed: bool, rd: u8, rn: u8, lsb: u8, width: u8 },
    /// BFI (rn != 15) / BFC (rn == 15).
    Bfi { rd: u8, rn: u8, lsb: u8, msb: u8 },
    /// SSAT / USAT. `sat` is the saturate_to bit count (1..32 for SSAT, 0..31 for USAT).
    Sat { unsigned: bool, rd: u8, sat: u8, rn: u8, shift: Shift },
    Ldr { sz: Size, signed: bool, rt: u8, addr: Addr, unpriv: bool },
    Str { sz: Size, rt: u8, addr: Addr, unpriv: bool },
    Ldrd { rt: u8, rt2: u8, addr: Addr },
    Strd { rt: u8, rt2: u8, addr: Addr },
    Ldrex { sz: Size, rt: u8, rn: u8, imm: u32 },
    Strex { sz: Size, rd: u8, rt: u8, rn: u8, imm: u32 },
    Clrex,
    /// LDM/LDMIA/LDMDB/POP. `db` = decrement before.
    Ldm { rn: u8, regs: u16, wback: bool, db: bool },
    /// STM/STMIA/STMDB/PUSH.
    Stm { rn: u8, regs: u16, wback: bool, db: bool },
    /// PLD / PLI (memory hints; no architectural effect).
    Preload { data: bool, addr: Addr },
    Tb { half: bool, rn: u8, rm: u8 },
    /// Conditional or unconditional branch, offset relative to PC+4. `cond == 14` = always.
    B { cond: u8, off: i32 },
    Bl { off: i32 },
    Bx { rm: u8 },
    Blx { rm: u8 },
    Cbz { nz: bool, rn: u8, off: u32 },
    It { firstcond: u8, mask: u8 },
    Hint(HintKind),
    Svc(u8),
    Bkpt(u8),
    Cps { disable: bool, i: bool, f: bool },
    Msr { sysm: u8, mask: u8, rn: u8 },
    Mrs { rd: u8, sysm: u8 },
    Barrier { kind: Barrier, opt: u8 },
    /// Permanently undefined (UDF).
    Udf { imm: u16 },
    /// Unallocated / not implemented on Cortex-M3: UsageFault UNDEFINSTR.
    Undefined,
    /// Coprocessor space: UsageFault NOCP (no coprocessors on Cortex-M3).
    Coproc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decoded {
    pub instr: Instr,
    /// 2 or 4 bytes.
    pub len: u8,
    /// Decoded from a UNPREDICTABLE encoding (the simulator treats these as UNDEFINED so code
    /// that only "works by accident" fails loudly instead of silently).
    pub unpredictable: bool,
}

#[inline]
pub fn is_thumb32(hw1: u16) -> bool {
    (hw1 >> 11) >= 0b11101
}

#[inline]
fn bit(v: u32, n: u32) -> bool {
    (v >> n) & 1 != 0
}

#[inline]
fn bits(v: u32, hi: u32, lo: u32) -> u32 {
    (v >> lo) & ((1u32 << (hi - lo + 1)) - 1)
}

#[inline]
fn sign_extend(val: u32, nbits: u32) -> i32 {
    let sh = 32 - nbits;
    ((val << sh) as i32) >> sh
}

/// ThumbExpandImm_C(imm12, carry_in) -> (imm32, carry_out). Returns `None` for UNPREDICTABLE
/// encodings (replicated pattern with a zero byte).
pub fn thumb_expand_imm(imm12: u32) -> Option<(u32, Option<bool>)> {
    let imm12 = imm12 & 0xFFF;
    if bits(imm12, 11, 10) == 0 {
        let b = imm12 & 0xFF;
        let v = match bits(imm12, 9, 8) {
            0 => b,
            1 => {
                if b == 0 {
                    return None;
                }
                (b << 16) | b
            }
            2 => {
                if b == 0 {
                    return None;
                }
                (b << 24) | (b << 8)
            }
            _ => {
                if b == 0 {
                    return None;
                }
                b * 0x0101_0101
            }
        };
        Some((v, None))
    } else {
        let unrot = 0x80 | (imm12 & 0x7F);
        let v = unrot.rotate_right(bits(imm12, 11, 7));
        Some((v, Some(v >> 31 != 0)))
    }
}

/// Decode one instruction. `in_it` / `last_in_it` describe the IT state the instruction
/// executes under (they change the meaning of 16-bit flag-setting encodings and make some
/// encodings UNPREDICTABLE).
pub fn decode(hw1: u16, hw2: u16, in_it: bool, last_in_it: bool) -> Decoded {
    if is_thumb32(hw1) {
        let (instr, unpred) = decode32(hw1 as u32, hw2 as u32, in_it, last_in_it);
        Decoded { instr, len: 4, unpredictable: unpred }
    } else {
        let (instr, unpred) = decode16(hw1 as u32, in_it, last_in_it);
        Decoded { instr, len: 2, unpredictable: unpred }
    }
}

fn decode16(op: u32, in_it: bool, last_in_it: bool) -> (Instr, bool) {
    let s = !in_it; // 16-bit "S" forms only set flags outside an IT block
    let lo3 = |n: u32| ((op >> n) & 7) as u8;
    match op >> 10 {
        // 00xxxx: shift (immediate), add, subtract, move, compare
        0b000000..=0b001111 => {
            let opc = bits(op, 13, 9);
            let rd = lo3(0);
            let rm = lo3(3);
            match opc {
                0b00000..=0b00011 => {
                    let imm5 = bits(op, 10, 6);
                    if imm5 == 0 {
                        // MOVS Rd, Rm (MOV register T2) - UNPREDICTABLE inside IT
                        (Instr::AluReg { op: Alu::Mov, s: true, rd, rn: 15, rm, shift: Shift::NONE }, in_it)
                    } else {
                        (Instr::AluReg { op: Alu::Mov, s, rd, rn: 15, rm, shift: Shift::decode(0, imm5) }, false)
                    }
                }
                0b00100..=0b00111 => (
                    Instr::AluReg { op: Alu::Mov, s, rd, rn: 15, rm, shift: Shift::decode(1, bits(op, 10, 6)) },
                    false,
                ),
                0b01000..=0b01011 => (
                    Instr::AluReg { op: Alu::Mov, s, rd, rn: 15, rm, shift: Shift::decode(2, bits(op, 10, 6)) },
                    false,
                ),
                0b01100 => (Instr::AluReg { op: Alu::Add, s, rd, rn: rm, rm: lo3(6), shift: Shift::NONE }, false),
                0b01101 => (Instr::AluReg { op: Alu::Sub, s, rd, rn: rm, rm: lo3(6), shift: Shift::NONE }, false),
                0b01110 => (
                    Instr::AluImm { op: Alu::Add, s, rd, rn: rm, imm: bits(op, 8, 6), carry: None },
                    false,
                ),
                0b01111 => (
                    Instr::AluImm { op: Alu::Sub, s, rd, rn: rm, imm: bits(op, 8, 6), carry: None },
                    false,
                ),
                _ => {
                    let rdn = lo3(8);
                    let imm = op & 0xFF;
                    let alu = match bits(op, 12, 11) {
                        0 => Alu::Mov,
                        1 => Alu::Cmp,
                        2 => Alu::Add,
                        _ => Alu::Sub,
                    };
                    let rn = if alu == Alu::Mov { 15 } else { rdn };
                    let s = s || alu == Alu::Cmp;
                    (Instr::AluImm { op: alu, s, rd: rdn, rn, imm, carry: None }, false)
                }
            }
        }
        // 010000: data processing (register)
        0b010000 => {
            let rdn = lo3(0);
            let rm = lo3(3);
            let reg = |alu: Alu, s: bool| Instr::AluReg { op: alu, s, rd: rdn, rn: rdn, rm, shift: Shift::NONE };
            let sh = |t: SRType| Instr::ShiftReg { t, s, rd: rdn, rn: rdn, rm };
            let i = match bits(op, 9, 6) {
                0x0 => reg(Alu::And, s),
                0x1 => reg(Alu::Eor, s),
                0x2 => sh(SRType::Lsl),
                0x3 => sh(SRType::Lsr),
                0x4 => sh(SRType::Asr),
                0x5 => reg(Alu::Adc, s),
                0x6 => reg(Alu::Sbc, s),
                0x7 => sh(SRType::Ror),
                0x8 => Instr::AluReg { op: Alu::Tst, s: true, rd: 15, rn: rdn, rm, shift: Shift::NONE },
                0x9 => Instr::AluImm { op: Alu::Rsb, s, rd: rdn, rn: rm, imm: 0, carry: None },
                0xA => Instr::AluReg { op: Alu::Cmp, s: true, rd: 15, rn: rdn, rm, shift: Shift::NONE },
                0xB => Instr::AluReg { op: Alu::Cmn, s: true, rd: 15, rn: rdn, rm, shift: Shift::NONE },
                0xC => reg(Alu::Orr, s),
                0xD => Instr::Mul { s, rd: rdn, rn: rm, rm: rdn },
                0xE => reg(Alu::Bic, s),
                _ => Instr::AluReg { op: Alu::Mvn, s, rd: rdn, rn: 15, rm, shift: Shift::NONE },
            };
            (i, false)
        }
        // 010001: special data instructions and branch and exchange
        0b010001 => {
            let rdn = ((bit(op, 7) as u8) << 3) | lo3(0);
            let rm = bits(op, 6, 3) as u8;
            match bits(op, 9, 6) {
                0b0000..=0b0011 => {
                    let unpred = (rdn == 15 && rm == 15) || (rdn == 15 && in_it && !last_in_it);
                    (Instr::AluReg { op: Alu::Add, s: false, rd: rdn, rn: rdn, rm, shift: Shift::NONE }, unpred)
                }
                0b0100 => (Instr::Undefined, true),
                0b0101..=0b0111 => {
                    let unpred = (rdn < 8 && rm < 8) || rdn == 15 || rm == 15;
                    (Instr::AluReg { op: Alu::Cmp, s: true, rd: 15, rn: rdn, rm, shift: Shift::NONE }, unpred)
                }
                0b1000..=0b1011 => {
                    let unpred = rdn == 15 && in_it && !last_in_it;
                    (Instr::AluReg { op: Alu::Mov, s: false, rd: rdn, rn: 15, rm, shift: Shift::NONE }, unpred)
                }
                0b1100 | 0b1101 => {
                    let unpred = (op & 7) != 0 || (in_it && !last_in_it);
                    (Instr::Bx { rm }, unpred)
                }
                _ => {
                    let unpred = (op & 7) != 0 || rm == 15 || (in_it && !last_in_it);
                    (Instr::Blx { rm }, unpred)
                }
            }
        }
        // 01001x: LDR (literal)
        0b010010 | 0b010011 => (
            Instr::Ldr {
                sz: Size::W,
                signed: false,
                rt: lo3(8),
                addr: Addr::Lit { imm: (op & 0xFF) << 2, add: true },
                unpriv: false,
            },
            false,
        ),
        // 0101xx: load/store register offset
        0b010100..=0b010111 => {
            let rt = lo3(0);
            let addr = Addr::Reg { rn: lo3(3), rm: lo3(6), lsl: 0 };
            let i = match bits(op, 11, 9) {
                0 => Instr::Str { sz: Size::W, rt, addr, unpriv: false },
                1 => Instr::Str { sz: Size::H, rt, addr, unpriv: false },
                2 => Instr::Str { sz: Size::B, rt, addr, unpriv: false },
                3 => Instr::Ldr { sz: Size::B, signed: true, rt, addr, unpriv: false },
                4 => Instr::Ldr { sz: Size::W, signed: false, rt, addr, unpriv: false },
                5 => Instr::Ldr { sz: Size::H, signed: false, rt, addr, unpriv: false },
                6 => Instr::Ldr { sz: Size::B, signed: false, rt, addr, unpriv: false },
                _ => Instr::Ldr { sz: Size::H, signed: true, rt, addr, unpriv: false },
            };
            (i, false)
        }
        // 011xxx / 1000xx: load/store immediate offset
        0b011000..=0b100011 => {
            let rt = lo3(0);
            let rn = lo3(3);
            let imm5 = bits(op, 10, 6);
            let load = bit(op, 11);
            let (sz, imm) = match bits(op, 15, 12) {
                0b0110 => (Size::W, imm5 << 2),
                0b0111 => (Size::B, imm5),
                _ => (Size::H, imm5 << 1),
            };
            let addr = Addr::Imm { rn, imm, add: true, index: true, wback: false };
            if load {
                (Instr::Ldr { sz, signed: false, rt, addr, unpriv: false }, false)
            } else {
                (Instr::Str { sz, rt, addr, unpriv: false }, false)
            }
        }
        // 1001xx: SP-relative load/store
        0b100100..=0b100111 => {
            let rt = lo3(8);
            let addr = Addr::Imm { rn: 13, imm: (op & 0xFF) << 2, add: true, index: true, wback: false };
            if bit(op, 11) {
                (Instr::Ldr { sz: Size::W, signed: false, rt, addr, unpriv: false }, false)
            } else {
                (Instr::Str { sz: Size::W, rt, addr, unpriv: false }, false)
            }
        }
        // 10100x: ADR
        0b101000 | 0b101001 => (Instr::Adr { rd: lo3(8), imm: (op & 0xFF) << 2, add: true }, false),
        // 10101x: ADD Rd, SP, #imm8<<2
        0b101010 | 0b101011 => (Instr::AddW { sub: false, rd: lo3(8), rn: 13, imm: (op & 0xFF) << 2 }, false),
        // 1011xx: miscellaneous 16-bit
        0b101100..=0b101111 => decode16_misc(op, in_it, last_in_it),
        // 11000x: STM, 11001x: LDM
        0b110000..=0b110011 => {
            let rn = lo3(8);
            let regs = (op & 0xFF) as u16;
            let unpred = regs == 0;
            if bit(op, 11) {
                let wback = (regs >> rn) & 1 == 0;
                (Instr::Ldm { rn, regs, wback, db: false }, unpred)
            } else {
                (Instr::Stm { rn, regs, wback: true, db: false }, unpred)
            }
        }
        // 1101xx: conditional branch, UDF, SVC
        0b110100..=0b110111 => {
            let cond = bits(op, 11, 8) as u8;
            match cond {
                0xE => (Instr::Udf { imm: (op & 0xFF) as u16 }, false),
                0xF => (Instr::Svc((op & 0xFF) as u8), false),
                _ => (Instr::B { cond, off: sign_extend((op & 0xFF) << 1, 9) }, in_it),
            }
        }
        // 11100x: unconditional branch
        0b111000 | 0b111001 => (
            Instr::B { cond: 14, off: sign_extend((op & 0x7FF) << 1, 12) },
            in_it && !last_in_it,
        ),
        _ => (Instr::Undefined, false),
    }
}

fn decode16_misc(op: u32, in_it: bool, last_in_it: bool) -> (Instr, bool) {
    let lo3 = |n: u32| ((op >> n) & 7) as u8;
    let opc = bits(op, 11, 5);
    match opc {
        0b0000000..=0b0000011 => (Instr::AddW { sub: false, rd: 13, rn: 13, imm: (op & 0x7F) << 2 }, false),
        0b0000100..=0b0000111 => (Instr::AddW { sub: true, rd: 13, rn: 13, imm: (op & 0x7F) << 2 }, false),
        0b0001000..=0b0001111 | 0b0011000..=0b0011111 | 0b1001000..=0b1001111 | 0b1011000..=0b1011111 => {
            let nz = bit(op, 11);
            let imm = ((bit(op, 9) as u32) << 6) | (bits(op, 7, 3) << 1);
            (Instr::Cbz { nz, rn: lo3(0), off: imm }, in_it)
        }
        0b0010000..=0b0010111 => {
            let kind = match bits(op, 7, 6) {
                0 => ExtKind::Sxth,
                1 => ExtKind::Sxtb,
                2 => ExtKind::Uxth,
                _ => ExtKind::Uxtb,
            };
            (Instr::Extend { kind, rd: lo3(0), rm: lo3(3), rot: 0 }, false)
        }
        0b0100000..=0b0101111 => {
            let regs = ((op & 0xFF) | (if bit(op, 8) { 1 << 14 } else { 0 })) as u16;
            (Instr::Stm { rn: 13, regs, wback: true, db: true }, regs == 0)
        }
        0b0110011 => {
            let i = bit(op, 1);
            let f = bit(op, 0);
            let unpred = (!i && !f) || in_it || bits(op, 3, 2) != 0;
            (Instr::Cps { disable: bit(op, 4), i, f }, unpred)
        }
        0b1010000..=0b1010001 => (Instr::Rev { kind: RevKind::Rev, rd: lo3(0), rm: lo3(3) }, false),
        0b1010010..=0b1010011 => (Instr::Rev { kind: RevKind::Rev16, rd: lo3(0), rm: lo3(3) }, false),
        0b1010110..=0b1010111 => (Instr::Rev { kind: RevKind::Revsh, rd: lo3(0), rm: lo3(3) }, false),
        0b1100000..=0b1101111 => {
            let regs = ((op & 0xFF) | (if bit(op, 8) { 1 << 15 } else { 0 })) as u16;
            let unpred = regs == 0 || (bit(op, 8) && in_it && !last_in_it);
            (Instr::Ldm { rn: 13, regs, wback: true, db: false }, unpred)
        }
        0b1110000..=0b1110111 => (Instr::Bkpt((op & 0xFF) as u8), false),
        0b1111000..=0b1111111 => {
            let opa = bits(op, 7, 4) as u8;
            let opb = bits(op, 3, 0) as u8;
            if opb != 0 {
                let unpred = opa == 0xF || (opa == 0xE && opb.count_ones() != 1) || in_it;
                (Instr::It { firstcond: opa, mask: opb }, unpred)
            } else {
                let h = match opa {
                    0 => HintKind::Nop,
                    1 => HintKind::Yield,
                    2 => HintKind::Wfe,
                    3 => HintKind::Wfi,
                    4 => HintKind::Sev,
                    x => HintKind::Other(x),
                };
                (Instr::Hint(h), false)
            }
        }
        _ => (Instr::Undefined, false),
    }
}

fn decode32(h1: u32, h2: u32, in_it: bool, last_in_it: bool) -> (Instr, bool) {
    let op1 = bits(h1, 12, 11);
    let op2 = bits(h1, 10, 4);
    let op = bit(h2, 15);
    match op1 {
        0b01 => {
            if op2 & 0b1100100 == 0b0000000 {
                decode_ldm_stm(h1, h2, in_it, last_in_it)
            } else if op2 & 0b1100100 == 0b0000100 {
                decode_ldst_dual_excl(h1, h2, in_it, last_in_it)
            } else if op2 & 0b1100000 == 0b0100000 {
                decode_dp_shifted(h1, h2)
            } else {
                (Instr::Coproc, false)
            }
        }
        0b10 => {
            if op {
                decode_branch_misc(h1, h2, in_it, last_in_it)
            } else if op2 & 0b0100000 == 0 {
                decode_dp_modimm(h1, h2)
            } else {
                decode_dp_plain(h1, h2)
            }
        }
        _ => {
            if op2 & 0b1000000 != 0 {
                return (Instr::Coproc, false);
            }
            if op2 & 0b1110001 == 0b0000000 {
                decode_store_single(h1, h2)
            } else if op2 & 0b1100111 == 0b0000001 {
                decode_load(h1, h2, Size::B, in_it, last_in_it)
            } else if op2 & 0b1100111 == 0b0000011 {
                decode_load(h1, h2, Size::H, in_it, last_in_it)
            } else if op2 & 0b1100111 == 0b0000101 {
                decode_load(h1, h2, Size::W, in_it, last_in_it)
            } else if op2 & 0b1110000 == 0b0100000 {
                decode_dp_reg(h1, h2)
            } else if op2 & 0b1111000 == 0b0110000 {
                decode_mul(h1, h2)
            } else if op2 & 0b1111000 == 0b0111000 {
                decode_long_mul(h1, h2)
            } else {
                (Instr::Undefined, false)
            }
        }
    }
}

fn decode_ldm_stm(h1: u32, h2: u32, in_it: bool, last_in_it: bool) -> (Instr, bool) {
    let op = bits(h1, 8, 7);
    let load = bit(h1, 4);
    let wback = bit(h1, 5);
    let rn = (h1 & 0xF) as u8;
    let regs = h2 as u16;
    match (op, load) {
        (0b01, false) | (0b10, false) => {
            let db = op == 0b10;
            let unpred = rn == 15
                || regs.count_ones() < 2
                || regs & 0xA000 != 0
                || (wback && (regs >> rn) & 1 != 0);
            (Instr::Stm { rn, regs, wback, db }, unpred)
        }
        (0b01, true) | (0b10, true) => {
            let db = op == 0b10;
            let unpred = rn == 15
                || regs.count_ones() < 2
                || regs & 0xC000 == 0xC000
                || regs & 0x2000 != 0
                || (regs & 0x8000 != 0 && in_it && !last_in_it)
                || (wback && (regs >> rn) & 1 != 0);
            (Instr::Ldm { rn, regs, wback, db }, unpred)
        }
        _ => (Instr::Undefined, false),
    }
}

fn decode_ldst_dual_excl(h1: u32, h2: u32, in_it: bool, last_in_it: bool) -> (Instr, bool) {
    let op1 = bits(h1, 8, 7);
    let op2 = bits(h1, 5, 4);
    let op3 = bits(h2, 7, 4);
    let rn = (h1 & 0xF) as u8;
    let rt = bits(h2, 15, 12) as u8;
    let rt2 = bits(h2, 11, 8) as u8;
    let bad = |r: u8| r == 13 || r == 15;
    if op1 == 0b00 && op2 == 0b00 {
        // STREX Rd, Rt, [Rn, #imm]
        let rd = rt2;
        let imm = (h2 & 0xFF) << 2;
        let unpred = bad(rd) || bad(rt) || rn == 15 || rd == rn || rd == rt;
        return (Instr::Strex { sz: Size::W, rd, rt, rn, imm }, unpred);
    }
    if op1 == 0b00 && op2 == 0b01 {
        let imm = (h2 & 0xFF) << 2;
        let unpred = bad(rt) || rn == 15 || bits(h2, 11, 8) != 0xF;
        return (Instr::Ldrex { sz: Size::W, rt, rn, imm }, unpred);
    }
    if (op1 & 0b10) == 0 && (op2 & 0b10) != 0 || (op1 & 0b10) != 0 {
        // LDRD / STRD (immediate / literal)
        let p = bit(h1, 8);
        let u = bit(h1, 7);
        let w = bit(h1, 5);
        let load = bit(h1, 4);
        let imm = (h2 & 0xFF) << 2;
        if load {
            let addr = if rn == 15 {
                Addr::Lit { imm, add: u }
            } else {
                Addr::Imm { rn, imm, add: u, index: p, wback: w }
            };
            let unpred = bad(rt) || bad(rt2) || rt == rt2 || (w && (rn == rt || rn == rt2)) || (rn == 15 && w);
            return (Instr::Ldrd { rt, rt2, addr }, unpred);
        } else {
            let addr = Addr::Imm { rn, imm, add: u, index: p, wback: w };
            let unpred = bad(rt) || bad(rt2) || rn == 15 || (w && (rn == rt || rn == rt2));
            return (Instr::Strd { rt, rt2, addr }, unpred);
        }
    }
    // op1 == 01
    if op2 == 0b00 {
        let rd = (h2 & 0xF) as u8;
        let sz = match op3 {
            0b0100 => Size::B,
            0b0101 => Size::H,
            _ => return (Instr::Undefined, false),
        };
        let unpred = bad(rd) || bad(rt) || rn == 15 || rd == rn || rd == rt || bits(h2, 11, 8) != 0xF;
        return (Instr::Strex { sz, rd, rt, rn, imm: 0 }, unpred);
    }
    // op1 == 01, op2 == 01
    match op3 {
        0b0000 | 0b0001 => {
            let rm = (h2 & 0xF) as u8;
            let unpred = rn == 13 || rm == 13 || rm == 15 || bits(h2, 15, 8) != 0xF0 || (in_it && !last_in_it);
            (Instr::Tb { half: op3 == 1, rn, rm }, unpred)
        }
        0b0100 | 0b0101 => {
            let sz = if op3 == 0b0100 { Size::B } else { Size::H };
            let unpred = bad(rt) || rn == 15 || bits(h2, 11, 8) != 0xF || (h2 & 0xF) != 0xF;
            (Instr::Ldrex { sz, rt, rn, imm: 0 }, unpred)
        }
        _ => (Instr::Undefined, false),
    }
}

fn decode_dp_shifted(h1: u32, h2: u32) -> (Instr, bool) {
    let opc = bits(h1, 8, 5);
    let s = bit(h1, 4);
    let rn = (h1 & 0xF) as u8;
    let rd = bits(h2, 11, 8) as u8;
    let rm = (h2 & 0xF) as u8;
    let imm5 = (bits(h2, 14, 12) << 2) | bits(h2, 7, 6);
    let shift = Shift::decode(bits(h2, 5, 4), imm5);
    let mut unpred = bit(h2, 15);
    let bad = |r: u8| r == 13 || r == 15;
    let alu = match opc {
        0x0 => {
            if rd == 15 && s {
                Alu::Tst
            } else {
                Alu::And
            }
        }
        0x1 => Alu::Bic,
        0x2 => {
            if rn == 15 {
                Alu::Mov
            } else {
                Alu::Orr
            }
        }
        0x3 => {
            if rn == 15 {
                Alu::Mvn
            } else {
                Alu::Orn
            }
        }
        0x4 => {
            if rd == 15 && s {
                Alu::Teq
            } else {
                Alu::Eor
            }
        }
        0x8 => {
            if rd == 15 && s {
                Alu::Cmn
            } else {
                Alu::Add
            }
        }
        0xA => Alu::Adc,
        0xB => Alu::Sbc,
        0xD => {
            if rd == 15 && s {
                Alu::Cmp
            } else {
                Alu::Sub
            }
        }
        0xE => Alu::Rsb,
        _ => return (Instr::Undefined, false),
    };
    // Register restrictions (UNPREDICTABLE cases from the ARM ARM).
    let sp_form = matches!(alu, Alu::Add | Alu::Sub | Alu::Cmp | Alu::Cmn) && rn == 13;
    if alu.is_compare() {
        unpred |= (rn == 15) || bad(rm) || (rn == 13 && !sp_form);
    } else if alu == Alu::Mov {
        // MOV (register) T3 / shifts: SP allowed only for plain MOV without shift.
        if shift.is_none() && !s {
            unpred |= rd == 15 || rm == 15 || (rd == 13 && rm == 13);
        } else {
            unpred |= bad(rd) || bad(rm);
        }
    } else if alu == Alu::Mvn {
        unpred |= bad(rd) || bad(rm);
    } else if sp_form {
        // ADD/SUB (SP plus/minus register)
        unpred |= rd == 15 || bad(rm) || (rd == 13 && !(shift.t == SRType::Lsl && shift.n <= 3));
    } else {
        unpred |= bad(rd) || rn == 13 || rn == 15 || bad(rm);
    }
    (Instr::AluReg { op: alu, s: s || alu.is_compare(), rd: if alu.is_compare() { 15 } else { rd }, rn: if alu.has_rn() { rn } else { 15 }, rm, shift }, unpred)
}

fn decode_dp_modimm(h1: u32, h2: u32) -> (Instr, bool) {
    let opc = bits(h1, 8, 5);
    let s = bit(h1, 4);
    let rn = (h1 & 0xF) as u8;
    let rd = bits(h2, 11, 8) as u8;
    let imm12 = ((bit(h1, 10) as u32) << 11) | (bits(h2, 14, 12) << 8) | (h2 & 0xFF);
    let Some((imm, carry)) = thumb_expand_imm(imm12) else {
        return (Instr::Undefined, true);
    };
    let bad = |r: u8| r == 13 || r == 15;
    let alu = match opc {
        0x0 => {
            if rd == 15 && s {
                Alu::Tst
            } else {
                Alu::And
            }
        }
        0x1 => Alu::Bic,
        0x2 => {
            if rn == 15 {
                Alu::Mov
            } else {
                Alu::Orr
            }
        }
        0x3 => {
            if rn == 15 {
                Alu::Mvn
            } else {
                Alu::Orn
            }
        }
        0x4 => {
            if rd == 15 && s {
                Alu::Teq
            } else {
                Alu::Eor
            }
        }
        0x8 => {
            if rd == 15 && s {
                Alu::Cmn
            } else {
                Alu::Add
            }
        }
        0xA => Alu::Adc,
        0xB => Alu::Sbc,
        0xD => {
            if rd == 15 && s {
                Alu::Cmp
            } else {
                Alu::Sub
            }
        }
        0xE => Alu::Rsb,
        _ => return (Instr::Undefined, false),
    };
    let sp_form = matches!(alu, Alu::Add | Alu::Sub | Alu::Cmp | Alu::Cmn) && rn == 13;
    let unpred = if alu.is_compare() {
        rn == 15 || (rn == 13 && !sp_form)
    } else if !alu.has_rn() {
        bad(rd)
    } else if sp_form {
        rd == 15
    } else {
        bad(rd) || rn == 13 || rn == 15
    };
    // Carry-out matters only for logical ops.
    let carry = if alu.is_logical() { carry } else { None };
    (
        Instr::AluImm {
            op: alu,
            s: s || alu.is_compare(),
            rd: if alu.is_compare() { 15 } else { rd },
            rn: if alu.has_rn() { rn } else { 15 },
            imm,
            carry,
        },
        unpred || bit(h2, 15),
    )
}

fn decode_dp_plain(h1: u32, h2: u32) -> (Instr, bool) {
    let opc = bits(h1, 8, 4);
    let rn = (h1 & 0xF) as u8;
    let rd = bits(h2, 11, 8) as u8;
    let imm3 = bits(h2, 14, 12);
    let imm8 = h2 & 0xFF;
    let i = bit(h1, 10) as u32;
    let imm12 = (i << 11) | (imm3 << 8) | imm8;
    let imm16 = (bits(h1, 3, 0) << 12) | imm12;
    let imm2 = bits(h2, 7, 6);
    let bad = |r: u8| r == 13 || r == 15;
    match opc {
        0b00000 => {
            if rn == 15 {
                (Instr::Adr { rd, imm: imm12, add: true }, bad(rd))
            } else {
                (Instr::AddW { sub: false, rd, rn, imm: imm12 }, rd == 15 || (rd == 13 && rn != 13))
            }
        }
        0b01010 => {
            if rn == 15 {
                (Instr::Adr { rd, imm: imm12, add: false }, bad(rd))
            } else {
                (Instr::AddW { sub: true, rd, rn, imm: imm12 }, rd == 15 || (rd == 13 && rn != 13))
            }
        }
        0b00100 => (Instr::Movw { rd, imm: imm16 as u16 }, bad(rd)),
        0b01100 => (Instr::Movt { rd, imm: imm16 as u16 }, bad(rd)),
        0b10000 | 0b10010 | 0b11000 | 0b11010 => {
            let unsigned = opc & 0b01000 != 0;
            let sh = bit(h1, 5);
            let imm5 = (imm3 << 2) | imm2;
            if sh && imm5 == 0 {
                // SSAT16 / USAT16: DSP extension, not on Cortex-M3
                return (Instr::Undefined, false);
            }
            let shift = Shift::decode(if sh { 2 } else { 0 }, imm5);
            let sat_imm = (h2 & 0x1F) as u8;
            let sat = if unsigned { sat_imm } else { sat_imm + 1 };
            (Instr::Sat { unsigned, rd, sat, rn, shift }, bad(rd) || bad(rn) || i != 0 || bit(h2, 5))
        }
        0b10100 | 0b11100 => {
            let lsb = ((imm3 << 2) | imm2) as u8;
            let width = ((h2 & 0x1F) + 1) as u8;
            let unpred = bad(rd) || bad(rn) || (lsb as u32 + width as u32) > 32 || i != 0 || bit(h2, 5);
            (Instr::Bfx { signed: opc == 0b10100, rd, rn, lsb, width }, unpred)
        }
        0b10110 => {
            let lsb = ((imm3 << 2) | imm2) as u8;
            let msb = (h2 & 0x1F) as u8;
            let unpred = bad(rd) || rn == 13 || msb < lsb || i != 0 || bit(h2, 5);
            (Instr::Bfi { rd, rn, lsb, msb }, unpred)
        }
        _ => (Instr::Undefined, false),
    }
}

fn decode_branch_misc(h1: u32, h2: u32, in_it: bool, last_in_it: bool) -> (Instr, bool) {
    let op1 = bits(h2, 14, 12);
    let op = bits(h1, 10, 4);
    if op1 == 0b010 && op == 0b1111111 {
        // UDF.W (permanently undefined; still UNDEFINED when executed)
        return (Instr::Udf { imm: ((bits(h1, 3, 0) << 12) | (h2 & 0xFFF)) as u16 }, false);
    }
    if op1 & 0b101 == 0b000 {
        if op & 0b0111000 != 0b0111000 {
            // B<cond>.W (T3)
            let s = bit(h1, 10) as u32;
            let j1 = bit(h2, 13) as u32;
            let j2 = bit(h2, 11) as u32;
            let raw = (s << 20) | (j2 << 19) | (j1 << 18) | (bits(h1, 5, 0) << 12) | ((h2 & 0x7FF) << 1);
            return (Instr::B { cond: bits(h1, 9, 6) as u8, off: sign_extend(raw, 21) }, in_it);
        }
        match op {
            0b0111000 | 0b0111001 => {
                let rn = (h1 & 0xF) as u8;
                let mask = bits(h2, 11, 10) as u8;
                let sysm = (h2 & 0xFF) as u8;
                let valid_sysm = matches!(sysm, 0..=3 | 5..=9 | 16..=20);
                let unpred = rn == 13
                    || rn == 15
                    || !valid_sysm
                    || bits(h2, 9, 8) != 0
                    || bit(h1, 4)
                    || (sysm <= 3 && (mask == 0 || mask & 1 != 0))
                    || (sysm > 3 && mask != 0b10);
                return (Instr::Msr { sysm, mask, rn }, unpred);
            }
            0b0111010 => {
                if bits(h2, 10, 8) != 0 {
                    // CPS.W: not available on M-profile
                    return (Instr::Undefined, false);
                }
                let o = (h2 & 0xFF) as u8;
                let h = match o {
                    0 => HintKind::Nop,
                    1 => HintKind::Yield,
                    2 => HintKind::Wfe,
                    3 => HintKind::Wfi,
                    4 => HintKind::Sev,
                    0xF0..=0xFF => HintKind::Dbg(o & 0xF),
                    x => HintKind::Other(x),
                };
                return (Instr::Hint(h), false);
            }
            0b0111011 => {
                let opt = (h2 & 0xF) as u8;
                return match bits(h2, 7, 4) {
                    0b0010 => (Instr::Clrex, false),
                    0b0100 => (Instr::Barrier { kind: Barrier::Dsb, opt }, false),
                    0b0101 => (Instr::Barrier { kind: Barrier::Dmb, opt }, false),
                    0b0110 => (Instr::Barrier { kind: Barrier::Isb, opt }, false),
                    _ => (Instr::Undefined, false),
                };
            }
            0b0111110 | 0b0111111 => {
                let rd = bits(h2, 11, 8) as u8;
                let sysm = (h2 & 0xFF) as u8;
                let valid_sysm = matches!(sysm, 0..=3 | 5..=9 | 16..=20);
                let unpred = rd == 13 || rd == 15 || !valid_sysm;
                return (Instr::Mrs { rd, sysm }, unpred);
            }
            _ => return (Instr::Undefined, false),
        }
    }
    if op1 & 0b101 == 0b100 {
        // BLX (immediate): switches to ARM state, UNDEFINED on ARMv7-M
        return (Instr::Undefined, false);
    }
    // B.W (T4) / BL
    let s = bit(h1, 10) as u32;
    let j1 = bit(h2, 13) as u32;
    let j2 = bit(h2, 11) as u32;
    let i1 = !(j1 ^ s) & 1;
    let i2 = !(j2 ^ s) & 1;
    let raw = (s << 24) | (i1 << 23) | (i2 << 22) | (bits(h1, 9, 0) << 12) | ((h2 & 0x7FF) << 1);
    let off = sign_extend(raw, 25);
    let unpred = in_it && !last_in_it;
    if op1 & 0b100 != 0 {
        (Instr::Bl { off }, unpred)
    } else {
        (Instr::B { cond: 14, off }, unpred)
    }
}

fn decode_store_single(h1: u32, h2: u32) -> (Instr, bool) {
    let op1 = bits(h1, 7, 5);
    let rn = (h1 & 0xF) as u8;
    let rt = bits(h2, 15, 12) as u8;
    let sz = match op1 & 0b011 {
        0b00 => Size::B,
        0b01 => Size::H,
        0b10 => Size::W,
        _ => return (Instr::Undefined, false),
    };
    if rn == 15 {
        return (Instr::Undefined, false);
    }
    let rt_bad = rt == 15 || (rt == 13 && sz != Size::W);
    if op1 & 0b100 != 0 {
        let addr = Addr::Imm { rn, imm: h2 & 0xFFF, add: true, index: true, wback: false };
        return (Instr::Str { sz, rt, addr, unpriv: false }, rt_bad);
    }
    if bit(h2, 11) {
        let p = bit(h2, 10);
        let u = bit(h2, 9);
        let w = bit(h2, 8);
        if !p && !w {
            return (Instr::Undefined, false);
        }
        let unpriv = p && u && !w;
        let addr = Addr::Imm { rn, imm: h2 & 0xFF, add: u, index: p, wback: w };
        let unpred = rt_bad || (w && rn == rt);
        return (Instr::Str { sz, rt, addr, unpriv }, unpred);
    }
    if bits(h2, 10, 6) != 0 {
        return (Instr::Undefined, false);
    }
    let rm = (h2 & 0xF) as u8;
    let addr = Addr::Reg { rn, rm, lsl: bits(h2, 5, 4) as u8 };
    (Instr::Str { sz, rt, addr, unpriv: false }, rt_bad || rm == 13 || rm == 15)
}

fn decode_load(h1: u32, h2: u32, sz: Size, in_it: bool, last_in_it: bool) -> (Instr, bool) {
    let signed = bit(h1, 8);
    let rn = (h1 & 0xF) as u8;
    let rt = bits(h2, 15, 12) as u8;
    let u23 = bit(h1, 7);
    if sz == Size::W && signed {
        return (Instr::Undefined, false);
    }
    // Rt == PC on byte/halfword loads encodes PLD/PLI or unallocated hints.
    let hint = rt == 15 && sz != Size::W;
    let rt_bad = |pc_ok: bool| -> bool {
        if sz == Size::W {
            rt == 15 && !pc_ok
        } else {
            rt == 13 || rt == 15
        }
    };
    let pc_load_unpred = sz == Size::W && rt == 15 && in_it && !last_in_it;
    let mk = |addr: Addr, unpriv: bool| -> Instr {
        if hint {
            Instr::Preload { data: sz == Size::B && !signed || sz == Size::H, addr }
        } else {
            Instr::Ldr { sz, signed, rt, addr, unpriv }
        }
    };
    // LDRB/LDRH/LDRSB/LDRSH with Rt = SP are UNPREDICTABLE in every form (A7.7.46 etc.)
    let sp_narrow = sz != Size::W && rt == 13;
    if rn == 15 {
        let addr = Addr::Lit { imm: h2 & 0xFFF, add: u23 };
        return (mk(addr, false), pc_load_unpred || sp_narrow);
    }
    if u23 {
        let addr = Addr::Imm { rn, imm: h2 & 0xFFF, add: true, index: true, wback: false };
        return (mk(addr, false), pc_load_unpred || sp_narrow);
    }
    if bit(h2, 11) {
        let p = bit(h2, 10);
        let u = bit(h2, 9);
        let w = bit(h2, 8);
        if !p && !w {
            return (Instr::Undefined, false);
        }
        let unpriv = p && u && !w;
        if hint && (w || unpriv || u) {
            // Only the negative-offset form (1100) is a hint; wback forms with Rt==PC are UNPREDICTABLE
            return (Instr::Undefined, true);
        }
        let addr = Addr::Imm { rn, imm: h2 & 0xFF, add: u, index: p, wback: w };
        let unpred = (!hint && rt_bad(!unpriv)) || (w && rn == rt) || pc_load_unpred;
        return (mk(addr, unpriv), unpred);
    }
    if bits(h2, 10, 6) != 0 {
        return (Instr::Undefined, false);
    }
    let rm = (h2 & 0xF) as u8;
    let addr = Addr::Reg { rn, rm, lsl: bits(h2, 5, 4) as u8 };
    (mk(addr, false), (!hint && rt_bad(true)) || rm == 13 || rm == 15 || pc_load_unpred)
}

fn decode_dp_reg(h1: u32, h2: u32) -> (Instr, bool) {
    let op1 = bits(h1, 7, 4);
    let op2 = bits(h2, 7, 4);
    let rn = (h1 & 0xF) as u8;
    let rd = bits(h2, 11, 8) as u8;
    let rm = (h2 & 0xF) as u8;
    let bad = |r: u8| r == 13 || r == 15;
    if bits(h2, 15, 12) != 0xF {
        return (Instr::Undefined, false);
    }
    if op1 & 0b1000 == 0 && op2 == 0 {
        let t = SRType::from_bits(op1 >> 1);
        let s = op1 & 1 != 0;
        return (Instr::ShiftReg { t, s, rd, rn, rm }, bad(rd) || bad(rn) || bad(rm));
    }
    if op1 & 0b1000 == 0 && op2 & 0b1000 != 0 {
        let kind = match op1 {
            0b0000 => ExtKind::Sxth,
            0b0001 => ExtKind::Uxth,
            0b0100 => ExtKind::Sxtb,
            0b0101 => ExtKind::Uxtb,
            _ => return (Instr::Undefined, false), // SXTB16/UXTB16 (DSP)
        };
        if rn != 15 {
            // SXTAB / SXTAH / UXTAB / UXTAH are ARMv7E-M only
            return (Instr::Undefined, false);
        }
        let rot = (bits(h2, 5, 4) * 8) as u8;
        return (Instr::Extend { kind, rd, rm, rot }, bad(rd) || bad(rm) || bit(h2, 6));
    }
    if op1 & 0b1100 == 0b1000 && op2 & 0b1100 == 0b1000 {
        let unpred = bad(rd) || bad(rm) || rn != rm;
        return match (bits(op1, 1, 0), bits(op2, 1, 0)) {
            (0b01, 0b00) => (Instr::Rev { kind: RevKind::Rev, rd, rm }, unpred),
            (0b01, 0b01) => (Instr::Rev { kind: RevKind::Rev16, rd, rm }, unpred),
            (0b01, 0b10) => (Instr::Rev { kind: RevKind::Rbit, rd, rm }, unpred),
            (0b01, 0b11) => (Instr::Rev { kind: RevKind::Revsh, rd, rm }, unpred),
            (0b11, 0b00) => (Instr::Clz { rd, rm }, unpred),
            _ => (Instr::Undefined, false),
        };
    }
    (Instr::Undefined, false)
}

fn decode_mul(h1: u32, h2: u32) -> (Instr, bool) {
    let op1 = bits(h1, 6, 4);
    let op2 = bits(h2, 5, 4);
    let rn = (h1 & 0xF) as u8;
    let ra = bits(h2, 15, 12) as u8;
    let rd = bits(h2, 11, 8) as u8;
    let rm = (h2 & 0xF) as u8;
    let bad = |r: u8| r == 13 || r == 15;
    if op1 != 0 || bits(h2, 7, 6) != 0 {
        return (Instr::Undefined, false);
    }
    match op2 {
        0b00 => {
            if ra == 15 {
                (Instr::Mul { s: false, rd, rn, rm }, bad(rd) || bad(rn) || bad(rm))
            } else {
                (Instr::Mla { rd, rn, rm, ra }, bad(rd) || bad(rn) || bad(rm) || ra == 13)
            }
        }
        0b01 => (Instr::Mls { rd, rn, rm, ra }, bad(rd) || bad(rn) || bad(rm) || bad(ra)),
        _ => (Instr::Undefined, false),
    }
}

fn decode_long_mul(h1: u32, h2: u32) -> (Instr, bool) {
    let op1 = bits(h1, 6, 4);
    let op2 = bits(h2, 7, 4);
    let rn = (h1 & 0xF) as u8;
    let rdlo = bits(h2, 15, 12) as u8;
    let rdhi = bits(h2, 11, 8) as u8;
    let rm = (h2 & 0xF) as u8;
    let bad = |r: u8| r == 13 || r == 15;
    let long = |kind: LongMul| {
        let unpred = bad(rdlo) || bad(rdhi) || bad(rn) || bad(rm) || rdhi == rdlo;
        (Instr::LongMul { kind, rdlo, rdhi, rn, rm }, unpred)
    };
    match (op1, op2) {
        (0b000, 0b0000) => long(LongMul::Smull),
        (0b010, 0b0000) => long(LongMul::Umull),
        (0b100, 0b0000) => long(LongMul::Smlal),
        (0b110, 0b0000) => long(LongMul::Umlal),
        (0b001, 0b1111) | (0b011, 0b1111) => {
            let rd = rdhi;
            let unpred = bad(rd) || bad(rn) || bad(rm) || rdlo != 15;
            (Instr::Div { signed: op1 == 0b001, rd, rn, rm }, unpred)
        }
        _ => (Instr::Undefined, false),
    }
}

/// Advance an IT state byte (ITAdvance()).
pub fn it_advance(it: u8) -> u8 {
    if it & 0x07 == 0 {
        0
    } else {
        (it & 0xE0) | ((it << 1) & 0x1F)
    }
}

/// Condition-code mnemonic suffix.
pub fn cond_name(c: u8) -> &'static str {
    match c & 0xF {
        0 => "EQ",
        1 => "NE",
        2 => "CS",
        3 => "CC",
        4 => "MI",
        5 => "PL",
        6 => "VS",
        7 => "VC",
        8 => "HI",
        9 => "LS",
        10 => "GE",
        11 => "LT",
        12 => "GT",
        13 => "LE",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_wide_store_without_signed_bit() {
        // STR.W r8, [r1]  = F8C1 8000
        let d = decode(0xF8C1, 0x8000, false, false);
        assert_eq!(
            d.instr,
            Instr::Str {
                sz: Size::W,
                rt: 8,
                addr: Addr::Imm { rn: 1, imm: 0, add: true, index: true, wback: false },
                unpriv: false
            }
        );
        // F9C1 8000 has the S bit set on a store: UNDEFINED, not a store
        assert_eq!(decode(0xF9C1, 0x8000, false, false).instr, Instr::Undefined);
    }

    #[test]
    fn blx_immediate_is_undefined_on_m_profile() {
        assert_eq!(decode(0xF000, 0xE800, false, false).instr, Instr::Undefined);
    }

    #[test]
    fn dsp_extensions_are_undefined() {
        // SXTAB r0, r1, r2 (v7E-M)
        assert_eq!(decode(0xFA41, 0xF082, false, false).instr, Instr::Undefined);
        // SMLABB r0, r1, r2, r3 (v7E-M)
        assert_eq!(decode(0xFB11, 0x3002, false, false).instr, Instr::Undefined);
    }

    #[test]
    fn sixteen_bit_flag_setting_depends_on_it() {
        let out = decode(0x1840, 0, false, false).instr; // ADDS r0, r0, r1
        let inside = decode(0x1840, 0, true, true).instr; // ADD<c> r0, r0, r1
        assert!(matches!(out, Instr::AluReg { s: true, .. }));
        assert!(matches!(inside, Instr::AluReg { s: false, .. }));
    }
}
