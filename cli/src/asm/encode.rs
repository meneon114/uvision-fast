//! Thumb / Thumb-2 (ARMv7-M) instruction encoder.
//!
//! Operands are parsed once into [`Op`]s; encoding happens per layout pass with the current
//! symbol values. Narrow/wide selection follows the same rules as LLVM's integrated
//! assembler (llvm-mc), which the golden tests compare against byte for byte:
//!
//! * a 16-bit data-processing encoding sets flags outside an IT block and does not inside
//!   one, so `ADDS` is narrow only outside IT and `ADD` only inside (except the SP / high
//!   register forms, which never set flags);
//! * immediates that do not fit flip to the complementary instruction when that one fits
//!   (`ADD #-4` -> `SUB #4`, `AND #~x` -> `BIC #x`, `MOV #-2` -> `MVN #1`, ...);
//! * `.W` forces a 32-bit encoding, `.N` forces 16-bit (error if impossible).
//!
//! Every encoding is decoded again with the shared decoder; UNPREDICTABLE register
//! combinations are rejected here rather than becoming a UsageFault on the chip.

use super::expr::{ExprError, Val};
use crate::thumb::{self, Instr, SRType};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Width {
    Any,
    Narrow,
    Wide,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MemOff {
    None,
    Imm(String),
    Reg { rm: u8, neg: bool, lsl: Option<String> },
}

#[derive(Clone, Debug, PartialEq)]
pub enum ShiftAmt {
    Imm(String),
    Reg(u8),
    Rrx,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Reg(u8),
    /// `Rn!`
    RegWb(u8),
    /// `#expr`
    Imm(String),
    /// Bare expression (label, address, special register name, barrier option ...).
    Expr(String),
    /// `=expr`
    Lit(String),
    List(u16),
    Mem { rn: u8, off: MemOff, wb: bool },
    Shift { t: SRType, amt: ShiftAmt },
}

/// A parsed instruction.
#[derive(Clone, Debug)]
pub struct Insn {
    pub base: &'static str,
    pub s: bool,
    /// Condition from the mnemonic suffix (14 = none / AL).
    pub cond: u8,
    pub width: Width,
    pub ops: Vec<Op>,
    /// For IT instructions: the T/E pattern after "IT" (e.g. "TE").
    pub it_pattern: String,
    /// Mnemonic as written (for messages).
    pub text: String,
}

/// Per-statement encoding context.
#[derive(Clone, Copy, Debug)]
pub struct Ctx {
    pub pc: u32,
    pub in_it: bool,
    pub last_in_it: bool,
    /// Reserve 4 bytes: emit the 32-bit form even when a 16-bit one would fit (relaxation).
    pub force_wide: bool,
    /// Address of this instruction's literal pool slot (LDR Rt, =expr).
    pub lit_addr: Option<u32>,
}

pub trait Env {
    fn eval(&self, expr: &str, pc: u32) -> Result<Val, ExprError>;
}

pub type EResult<T> = Result<T, String>;

fn ee(e: ExprError) -> String {
    e.to_string()
}

// ---------------------------------------------------------------------------------------
// Mnemonic and operand parsing
// ---------------------------------------------------------------------------------------

/// (base, S allowed)
const BASES: &[(&str, bool)] = &[
    ("ADD", true),
    ("ADC", true),
    ("SUB", true),
    ("SBC", true),
    ("RSB", true),
    ("AND", true),
    ("ORR", true),
    ("EOR", true),
    ("BIC", true),
    ("ORN", true),
    ("MOV", true),
    ("MVN", true),
    ("LSL", true),
    ("ASL", true),
    ("LSR", true),
    ("ASR", true),
    ("ROR", true),
    ("RRX", true),
    ("MUL", true),
    ("NEG", true),
    ("CPY", false),
    ("CMP", false),
    ("CMN", false),
    ("TST", false),
    ("TEQ", false),
    ("ADDW", false),
    ("SUBW", false),
    ("MOVW", false),
    ("MOVT", false),
    ("ADR", false),
    ("MLA", false),
    ("MLS", false),
    ("SMULL", false),
    ("UMULL", false),
    ("SMLAL", false),
    ("UMLAL", false),
    ("SDIV", false),
    ("UDIV", false),
    ("SXTB", false),
    ("SXTH", false),
    ("UXTB", false),
    ("UXTH", false),
    ("REV", false),
    ("REV16", false),
    ("REVSH", false),
    ("RBIT", false),
    ("CLZ", false),
    ("BFC", false),
    ("BFI", false),
    ("SBFX", false),
    ("UBFX", false),
    ("SSAT", false),
    ("USAT", false),
    ("LDR", false),
    ("LDRB", false),
    ("LDRH", false),
    ("LDRSB", false),
    ("LDRSH", false),
    ("LDRT", false),
    ("LDRBT", false),
    ("LDRHT", false),
    ("LDRSBT", false),
    ("LDRSHT", false),
    ("LDRD", false),
    ("LDREX", false),
    ("LDREXB", false),
    ("LDREXH", false),
    ("STR", false),
    ("STRB", false),
    ("STRH", false),
    ("STRT", false),
    ("STRBT", false),
    ("STRHT", false),
    ("STRD", false),
    ("STREX", false),
    ("STREXB", false),
    ("STREXH", false),
    ("LDM", false),
    ("LDMIA", false),
    ("LDMFD", false),
    ("LDMDB", false),
    ("LDMEA", false),
    ("STM", false),
    ("STMIA", false),
    ("STMEA", false),
    ("STMDB", false),
    ("STMFD", false),
    ("PUSH", false),
    ("POP", false),
    ("B", false),
    ("BL", false),
    ("BX", false),
    ("BLX", false),
    ("CBZ", false),
    ("CBNZ", false),
    ("TBB", false),
    ("TBH", false),
    ("NOP", false),
    ("YIELD", false),
    ("WFE", false),
    ("WFI", false),
    ("SEV", false),
    ("SVC", false),
    ("SWI", false),
    ("BKPT", false),
    ("CPSIE", false),
    ("CPSID", false),
    ("MRS", false),
    ("MSR", false),
    ("DMB", false),
    ("DSB", false),
    ("ISB", false),
    ("CLREX", false),
    ("PLD", false),
    ("PLI", false),
    ("UDF", false),
];

pub fn parse_cond(c: &str) -> Option<u8> {
    Some(match c {
        "EQ" => 0,
        "NE" => 1,
        "CS" | "HS" => 2,
        "CC" | "LO" => 3,
        "MI" => 4,
        "PL" => 5,
        "VS" => 6,
        "VC" => 7,
        "HI" => 8,
        "LS" => 9,
        "GE" => 10,
        "LT" => 11,
        "GT" => 12,
        "LE" => 13,
        "AL" => 14,
        _ => return None,
    })
}

/// Is this word an instruction mnemonic (used to tell labels from opcodes)?
pub fn is_mnemonic(word: &str) -> bool {
    split_mnemonic(word).is_some()
}

/// Split a mnemonic into (base, S, cond, width, IT pattern).
pub fn split_mnemonic(word: &str) -> Option<(&'static str, bool, u8, Width, String)> {
    let up = word.to_ascii_uppercase();
    let (core, width) = if let Some(c) = up.strip_suffix(".W") {
        (c.to_string(), Width::Wide)
    } else if let Some(c) = up.strip_suffix(".N") {
        (c.to_string(), Width::Narrow)
    } else {
        (up.clone(), Width::Any)
    };
    if let Some(pat) = core.strip_prefix("IT") {
        if pat.len() <= 3 && pat.chars().all(|c| c == 'T' || c == 'E') {
            return Some(("IT", false, 14, width, pat.to_string()));
        }
    }
    let mut best: Option<(&'static str, bool, u8)> = None;
    for &(base, s_ok) in BASES {
        let Some(rest) = core.strip_prefix(base) else { continue };
        let parsed = if rest.is_empty() {
            Some((false, 14))
        } else if rest == "S" && s_ok {
            Some((true, 14))
        } else if let Some(c) = parse_cond(rest) {
            Some((false, c))
        } else if rest.len() == 3 && s_ok {
            // UAL ADDSEQ or pre-UAL ADDEQS
            if let Some(c) = rest.strip_prefix('S').and_then(parse_cond) {
                Some((true, c))
            } else {
                rest.strip_suffix('S').and_then(parse_cond).map(|c| (true, c))
            }
        } else {
            None
        };
        if let Some((s, c)) = parsed {
            if best.map(|(b, _, _)| base.len() > b.len()).unwrap_or(true) {
                best = Some((base, s, c));
            }
        }
    }
    let (base, s, c) = best?;
    let base = match base {
        "SWI" => "SVC",
        "ASL" => "LSL",
        "CPY" => "MOV",
        b => b,
    };
    Some((base, s, c, width, String::new()))
}

pub fn parse_reg(s: &str) -> Option<u8> {
    let t = s.trim().to_ascii_uppercase();
    Some(match t.as_str() {
        "SP" => 13,
        "LR" => 14,
        "PC" => 15,
        "A1" => 0,
        "A2" => 1,
        "A3" => 2,
        "A4" => 3,
        "V1" => 4,
        "V2" => 5,
        "V3" => 6,
        "V4" => 7,
        "V5" => 8,
        "V6" | "SB" => 9,
        "V7" | "SL" => 10,
        "V8" | "FP" => 11,
        "IP" => 12,
        _ => {
            let n = t.strip_prefix('R')?;
            if n.is_empty() || n.len() > 2 || !n.bytes().all(|b| b.is_ascii_digit()) || (n.len() == 2 && n.starts_with('0')) {
                return None;
            }
            let v: u8 = n.parse().ok()?;
            if v > 15 {
                return None;
            }
            v
        }
    })
}

/// Split an operand list on top-level commas.
pub fn split_operands(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut sq, mut br, mut par) = (0i32, 0i32, 0i32);
    let mut in_str = false;
    let mut in_chr = false;
    for ch in s.chars() {
        match ch {
            '"' if !in_chr => in_str = !in_str,
            '\'' if !in_str => {
                in_chr = !in_chr;
            }
            '[' if !in_str && !in_chr => sq += 1,
            ']' if !in_str && !in_chr => sq -= 1,
            '{' if !in_str && !in_chr => br += 1,
            '}' if !in_str && !in_chr => br -= 1,
            '(' if !in_str && !in_chr => par += 1,
            ')' if !in_str && !in_chr => par -= 1,
            ',' if !in_str && !in_chr && sq == 0 && br == 0 && par == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(ch);
    }
    if !cur.trim().is_empty() || !out.is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

fn parse_shift(s: &str, regs: &dyn Fn(&str) -> Option<u8>) -> Option<Op> {
    let t = s.trim();
    let up = t.to_ascii_uppercase();
    if up == "RRX" {
        return Some(Op::Shift { t: SRType::Rrx, amt: ShiftAmt::Rrx });
    }
    if up.len() < 4 {
        return None;
    }
    let ty = match &up[..3] {
        "LSL" | "ASL" => SRType::Lsl,
        "LSR" => SRType::Lsr,
        "ASR" => SRType::Asr,
        "ROR" => SRType::Ror,
        _ => return None,
    };
    let rest = t[3..].trim();
    if t[3..].chars().next().map(|c| !c.is_whitespace() && c != '#').unwrap_or(true) {
        return None;
    }
    if let Some(r) = regs(rest) {
        return Some(Op::Shift { t: ty, amt: ShiftAmt::Reg(r) });
    }
    let e = rest.strip_prefix('#').unwrap_or(rest).trim();
    Some(Op::Shift { t: ty, amt: ShiftAmt::Imm(e.to_string()) })
}

fn parse_reg_list(s: &str, regs: &dyn Fn(&str) -> Option<u8>) -> EResult<u16> {
    let inner = s.trim().trim_start_matches('{');
    let inner = inner.strip_suffix('}').ok_or_else(|| format!("Missing '}}' in register list '{}'", s))?;
    let mut mask = 0u16;
    for tok in inner.split(',') {
        let t = tok.trim();
        if t.is_empty() {
            continue;
        }
        if let Some((a, b)) = t.split_once('-') {
            let r1 = regs(a).ok_or_else(|| format!("Bad register '{}' in list", a.trim()))?;
            let r2 = regs(b).ok_or_else(|| format!("Bad register '{}' in list", b.trim()))?;
            if r2 < r1 {
                return Err(format!("Register range '{}' must be ascending", t));
            }
            for r in r1..=r2 {
                mask |= 1 << r;
            }
        } else {
            let r = regs(t).ok_or_else(|| format!("Bad register '{}' in list", t))?;
            mask |= 1 << r;
        }
    }
    if mask == 0 {
        return Err("Empty register list".into());
    }
    Ok(mask)
}

fn parse_mem(s: &str, regs: &dyn Fn(&str) -> Option<u8>) -> EResult<Op> {
    let t = s.trim();
    let (body, wb) = if let Some(b) = t.strip_suffix('!') { (b.trim(), true) } else { (t, false) };
    let inner = body
        .strip_prefix('[')
        .and_then(|b| b.strip_suffix(']'))
        .ok_or_else(|| format!("Malformed memory operand '{}'", s))?;
    let parts = split_operands(inner);
    if parts.is_empty() || parts.len() > 3 {
        return Err(format!("Malformed memory operand '{}'", s));
    }
    let rn = regs(&parts[0]).ok_or_else(|| format!("Expected base register in '{}'", s))?;
    let off = if parts.len() == 1 {
        MemOff::None
    } else {
        let p1 = parts[1].trim();
        let (neg, p1r) = if let Some(r) = p1.strip_prefix('-') { (true, r.trim()) } else { (false, p1.strip_prefix('+').unwrap_or(p1).trim()) };
        if let Some(rm) = regs(p1r) {
            let lsl = if parts.len() == 3 {
                match parse_shift(&parts[2], regs) {
                    Some(Op::Shift { t: SRType::Lsl, amt: ShiftAmt::Imm(e) }) => Some(e),
                    _ => return Err(format!("Only LSL #0-3 is allowed for a register offset in '{}'", s)),
                }
            } else {
                None
            };
            MemOff::Reg { rm, neg, lsl }
        } else {
            if parts.len() == 3 {
                return Err(format!("Malformed memory operand '{}'", s));
            }
            let e = p1.strip_prefix('#').unwrap_or(p1).trim();
            MemOff::Imm(e.to_string())
        }
    };
    Ok(Op::Mem { rn, off, wb })
}

/// Parse one operand.
pub fn parse_operand(s: &str, regs: &dyn Fn(&str) -> Option<u8>) -> EResult<Op> {
    let t = s.trim();
    if t.is_empty() {
        return Err("Missing operand".into());
    }
    if t.starts_with('{') {
        return parse_reg_list(t, regs).map(Op::List);
    }
    if t.starts_with('[') {
        return parse_mem(t, regs);
    }
    if let Some(e) = t.strip_prefix('=') {
        return Ok(Op::Lit(e.trim().to_string()));
    }
    if let Some(e) = t.strip_prefix('#') {
        return Ok(Op::Imm(e.trim().to_string()));
    }
    if let Some(r) = t.strip_suffix('!').and_then(regs) {
        return Ok(Op::RegWb(r));
    }
    if let Some(r) = regs(t) {
        return Ok(Op::Reg(r));
    }
    if let Some(sh) = parse_shift(t, regs) {
        return Ok(sh);
    }
    Ok(Op::Expr(t.to_string()))
}

// ---------------------------------------------------------------------------------------
// Encoding helpers
// ---------------------------------------------------------------------------------------

fn n16(h: u32) -> Vec<u8> {
    (h as u16).to_le_bytes().to_vec()
}

fn w32(h1: u32, h2: u32) -> Vec<u8> {
    let mut v = (h1 as u16).to_le_bytes().to_vec();
    v.extend_from_slice(&(h2 as u16).to_le_bytes());
    v
}

#[inline]
fn lo(r: u8) -> bool {
    r < 8
}

#[inline]
fn bad(r: u8) -> bool {
    r == 13 || r == 15
}

/// Encode a 32-bit value as a Thumb-2 modified immediate (imm12 = i:imm3:imm8).
pub fn modimm(v: u32) -> Option<u32> {
    if v <= 0xFF {
        return Some(v);
    }
    let b = v & 0xFF;
    if v == (b << 16) | b {
        return Some(0x100 | b);
    }
    if v == (b << 24) | (b << 8) && b != 0 {
        // checked below with the byte from bits 15:8
    }
    let b1 = (v >> 8) & 0xFF;
    if v == (b1 << 24) | (b1 << 8) {
        return Some(0x200 | b1);
    }
    if v == b * 0x0101_0101 {
        return Some(0x300 | b);
    }
    for rot in 8..32u32 {
        let unrot = v.rotate_left(rot);
        if unrot <= 0xFF && unrot & 0x80 != 0 {
            return Some((rot << 7) | (unrot & 0x7F));
        }
    }
    None
}

fn split_imm12(imm12: u32) -> (u32, u32) {
    // returns (h1 bits (i << 10), h2 bits (imm3 << 12 | imm8))
    (((imm12 >> 11) & 1) << 10, (((imm12 >> 8) & 7) << 12) | (imm12 & 0xFF))
}

fn dp_imm(op: u32, s: bool, rn: u8, rd: u8, imm12: u32) -> Vec<u8> {
    let (i, h2) = split_imm12(imm12);
    w32(0xF000 | i | (op << 5) | ((s as u32) << 4) | rn as u32, h2 | ((rd as u32) << 8))
}

fn plain_imm12(base: u32, rn: u8, rd: u8, imm: u32) -> Vec<u8> {
    let (i, h2) = split_imm12(imm);
    w32(base | i | rn as u32, h2 | ((rd as u32) << 8))
}

fn shift_bits(t: SRType, n: u32) -> EResult<(u32, u32)> {
    // returns (type, imm5)
    Ok(match t {
        SRType::Lsl if n <= 31 => (0, n),
        SRType::Lsr if (1..=32).contains(&n) => (1, n & 31),
        SRType::Asr if (1..=32).contains(&n) => (2, n & 31),
        SRType::Ror if (1..=31).contains(&n) => (3, n),
        SRType::Rrx => (3, 0),
        _ => return Err(format!("Shift amount #{} out of range for {}", n, t.name())),
    })
}

fn dp_reg(op: u32, s: bool, rn: u8, rd: u8, rm: u8, ty: u32, imm5: u32) -> Vec<u8> {
    w32(
        0xEA00 | (op << 5) | ((s as u32) << 4) | rn as u32,
        ((imm5 >> 2) << 12) | ((rd as u32) << 8) | ((imm5 & 3) << 6) | (ty << 4) | rm as u32,
    )
}

fn cond_suffix(c: u8) -> &'static str {
    thumb::cond_name(c)
}

struct E<'a> {
    insn: &'a Insn,
    ctx: Ctx,
    env: &'a dyn Env,
}

impl E<'_> {
    fn wide(&self) -> bool {
        self.insn.width == Width::Wide || self.ctx.force_wide
    }
    fn may_narrow(&self) -> bool {
        !self.wide()
    }
    /// Narrow flag-setting encodings are usable when the requested S matches "outside IT".
    fn narrow_s_ok(&self) -> bool {
        self.insn.s != self.ctx.in_it
    }
    fn val(&self, e: &str) -> EResult<Val> {
        self.env.eval(e, self.ctx.pc).map_err(ee)
    }
    fn num(&self, e: &str) -> EResult<i64> {
        let v = self.val(e)?;
        Ok(v.v)
    }
    fn imm_of(&self, op: &Op) -> EResult<i64> {
        match op {
            Op::Imm(e) | Op::Expr(e) => self.num(e),
            _ => Err(format!("Expected an immediate (#expr), found {}", describe(op))),
        }
    }
    fn u32_imm(&self, op: &Op) -> EResult<u32> {
        let v = self.imm_of(op)?;
        if v < i32::MIN as i64 || v > u32::MAX as i64 {
            return Err(format!("Immediate 0x{:X} does not fit in 32 bits", v));
        }
        Ok(v as u32)
    }
    fn small_imm(&self, op: &Op, lo_: i64, hi: i64, what: &str) -> EResult<u32> {
        let v = self.imm_of(op)?;
        if v < lo_ || v > hi {
            return Err(format!("{} #{} out of range ({}..{})", what, v, lo_, hi));
        }
        Ok(v as u32)
    }
    fn target(&self, op: &Op) -> EResult<u32> {
        match op {
            Op::Expr(e) | Op::Imm(e) => {
                let v = self.val(e)?;
                Ok((v.v as u32) & !1)
            }
            _ => Err(format!("Expected a label or address, found {}", describe(op))),
        }
    }
    fn narrow_fail(&self) -> String {
        format!("No 16-bit encoding for '{}' with these operands (remove .N)", self.insn.text)
    }
}

fn describe(op: &Op) -> String {
    match op {
        Op::Reg(r) => format!("register r{}", r),
        Op::RegWb(r) => format!("'r{}!'", r),
        Op::Imm(e) => format!("'#{}'", e),
        Op::Expr(e) => format!("'{}'", e),
        Op::Lit(e) => format!("'={}'", e),
        Op::List(_) => "a register list".into(),
        Op::Mem { .. } => "a memory operand".into(),
        Op::Shift { .. } => "a shift".into(),
    }
}

pub(crate) fn reg_of(op: &Op) -> EResult<u8> {
    match op {
        Op::Reg(r) => Ok(*r),
        _ => Err(format!("Expected a register, found {}", describe(op))),
    }
}

fn nops(insn: &Insn, n: &[usize]) -> EResult<()> {
    if n.contains(&insn.ops.len()) {
        Ok(())
    } else {
        Err(format!(
            "'{}' expects {} operand(s), found {}",
            insn.text,
            n.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" or "),
            insn.ops.len()
        ))
    }
}

// ---------------------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------------------

/// Does `LDR Rt, =expr` with this value need a literal pool slot (vs. MOV/MVN/MOVW)?
/// SP and PC always load from the pool: MOV/MVN/MOVW cannot write them.
pub fn literal_needs_pool(v: &Val, rt: Option<u8>) -> bool {
    if !v.is_const() || matches!(rt, Some(13 | 15)) {
        return true;
    }
    let u = v.v as u32;
    !(modimm(u).is_some() || modimm(!u).is_some() || u <= 0xFFFF)
}

/// Encode an instruction. Returns 2 or 4 bytes.
pub fn encode(insn: &Insn, ctx: Ctx, env: &dyn Env) -> EResult<Vec<u8>> {
    let e = E { insn, ctx, env };
    let bytes = encode_inner(&e)?;
    if insn.width == Width::Narrow && bytes.len() != 2 {
        return Err(e.narrow_fail());
    }
    if insn.width == Width::Wide && bytes.len() != 4 {
        return Err(format!("No 32-bit encoding for '{}' (remove .W)", insn.text));
    }
    verify(insn, ctx, &bytes)?;
    Ok(bytes)
}

/// Decode what we produced and reject UNPREDICTABLE / inconsistent results.
fn verify(insn: &Insn, ctx: Ctx, bytes: &[u8]) -> EResult<()> {
    let hw1 = u16::from_le_bytes([bytes[0], bytes[1]]);
    let hw2 = if bytes.len() == 4 { u16::from_le_bytes([bytes[2], bytes[3]]) } else { 0 };
    let d = thumb::decode(hw1, hw2, ctx.in_it, ctx.last_in_it);
    if d.len as usize != bytes.len() {
        return Err(format!("internal encoder error for '{}' (length mismatch)", insn.text));
    }
    if matches!(d.instr, Instr::Undefined | Instr::Coproc) && insn.base != "UDF" {
        return Err(format!("'{}': this operand combination is not a valid ARMv7-M instruction", insn.text));
    }
    if d.unpredictable {
        return Err(format!(
            "'{}': UNPREDICTABLE register usage on Cortex-M3 (e.g. SP/PC not allowed here, or a PC write that is not last in its IT block)",
            insn.text
        ));
    }
    // Rd = PC with S set is the CMP/CMN/TST/TEQ encoding: never let `SUBS pc, lr, #4`
    // (an ARM-state idiom) silently become a compare.
    if let Instr::AluImm { op, .. } | Instr::AluReg { op, .. } = d.instr {
        if op.is_compare() && !matches!(insn.base, "CMP" | "CMN" | "TST" | "TEQ") {
            return Err(format!(
                "'{}': PC cannot be the destination of a flag-setting data-processing instruction on Cortex-M3 (return from a handler with BX LR or POP {{PC}})",
                insn.text
            ));
        }
    }
    let s_of = match d.instr {
        Instr::AluImm { op, s, .. } | Instr::AluReg { op, s, .. } if !op.is_compare() => Some(s),
        Instr::ShiftReg { s, .. } | Instr::Mul { s, .. } => Some(s),
        _ => None,
    };
    if let Some(s) = s_of {
        if s != insn.s && insn.base != "NEG" {
            return Err(format!("internal encoder error for '{}' (flag setting mismatch)", insn.text));
        }
    }
    Ok(())
}

fn encode_inner(e: &E) -> EResult<Vec<u8>> {
    let insn = e.insn;
    match insn.base {
        "MOV" => enc_mov(e),
        "MVN" => enc_mvn(e),
        "ADD" | "SUB" => enc_addsub(e),
        "ADDW" | "SUBW" => enc_addw(e),
        "ADC" | "SBC" | "AND" | "ORR" | "EOR" | "BIC" | "ORN" | "RSB" => enc_logic(e),
        "NEG" => {
            nops(insn, &[2])?;
            let mut i2 = insn.clone();
            i2.base = "RSB";
            i2.ops = vec![insn.ops[0].clone(), insn.ops[1].clone(), Op::Imm("0".into())];
            enc_logic(&E { insn: &i2, ctx: e.ctx, env: e.env })
        }
        "CMP" | "CMN" | "TST" | "TEQ" => enc_compare(e),
        "LSL" | "LSR" | "ASR" | "ROR" | "RRX" => enc_shift(e),
        "MUL" => enc_mul(e),
        "MLA" | "MLS" => {
            nops(insn, &[4])?;
            let (d, n, m, a) = (reg_of(&insn.ops[0])?, reg_of(&insn.ops[1])?, reg_of(&insn.ops[2])?, reg_of(&insn.ops[3])?);
            let x = if insn.base == "MLS" { 0x10 } else { 0 };
            Ok(w32(0xFB00 | n as u32, ((a as u32) << 12) | ((d as u32) << 8) | x | m as u32))
        }
        "SMULL" | "UMULL" | "SMLAL" | "UMLAL" => {
            nops(insn, &[4])?;
            let (l, h, n, m) = (reg_of(&insn.ops[0])?, reg_of(&insn.ops[1])?, reg_of(&insn.ops[2])?, reg_of(&insn.ops[3])?);
            if l == h {
                return Err(format!("'{}': RdLo and RdHi must be different registers", insn.text));
            }
            let base = match insn.base {
                "SMULL" => 0xFB80,
                "UMULL" => 0xFBA0,
                "SMLAL" => 0xFBC0,
                _ => 0xFBE0,
            };
            Ok(w32(base | n as u32, ((l as u32) << 12) | ((h as u32) << 8) | m as u32))
        }
        "SDIV" | "UDIV" => {
            nops(insn, &[2, 3])?;
            let (d, n, m) = three_regs(insn)?;
            let base = if insn.base == "SDIV" { 0xFB90 } else { 0xFBB0 };
            Ok(w32(base | n as u32, 0xF0F0 | ((d as u32) << 8) | m as u32))
        }
        "SXTB" | "SXTH" | "UXTB" | "UXTH" => enc_extend(e),
        "REV" | "REV16" | "REVSH" | "RBIT" | "CLZ" => {
            nops(insn, &[2])?;
            let (d, m) = (reg_of(&insn.ops[0])?, reg_of(&insn.ops[1])?);
            let narrow = match insn.base {
                "REV" => Some(0xBA00),
                "REV16" => Some(0xBA40),
                "REVSH" => Some(0xBAC0),
                _ => None,
            };
            if let Some(nb) = narrow {
                if lo(d) && lo(m) && e.may_narrow() {
                    return Ok(n16(nb | ((m as u32) << 3) | d as u32));
                }
            }
            let (h1, h2) = match insn.base {
                "REV" => (0xFA90, 0xF080),
                "REV16" => (0xFA90, 0xF090),
                "RBIT" => (0xFA90, 0xF0A0),
                "REVSH" => (0xFA90, 0xF0B0),
                _ => (0xFAB0, 0xF080),
            };
            Ok(w32(h1 | m as u32, h2 | ((d as u32) << 8) | m as u32))
        }
        "BFC" => {
            nops(insn, &[3])?;
            let d = reg_of(&insn.ops[0])?;
            let lsb = e.small_imm(&insn.ops[1], 0, 31, "lsb")?;
            let w = e.small_imm(&insn.ops[2], 1, 32 - lsb as i64, "width")?;
            Ok(bitfield(0xF36F, d, lsb, lsb + w - 1))
        }
        "BFI" => {
            nops(insn, &[4])?;
            let (d, n) = (reg_of(&insn.ops[0])?, reg_of(&insn.ops[1])?);
            if n == 15 {
                return Err("BFI: Rn cannot be PC (use BFC)".into());
            }
            let lsb = e.small_imm(&insn.ops[2], 0, 31, "lsb")?;
            let w = e.small_imm(&insn.ops[3], 1, 32 - lsb as i64, "width")?;
            Ok(bitfield(0xF360 | n as u32, d, lsb, lsb + w - 1))
        }
        "SBFX" | "UBFX" => {
            nops(insn, &[4])?;
            let (d, n) = (reg_of(&insn.ops[0])?, reg_of(&insn.ops[1])?);
            let lsb = e.small_imm(&insn.ops[2], 0, 31, "lsb")?;
            let w = e.small_imm(&insn.ops[3], 1, 32 - lsb as i64, "width")?;
            let base = if insn.base == "SBFX" { 0xF340 } else { 0xF3C0 };
            Ok(bitfield(base | n as u32, d, lsb, w - 1))
        }
        "SSAT" | "USAT" => {
            nops(insn, &[3, 4])?;
            let d = reg_of(&insn.ops[0])?;
            let sat = if insn.base == "SSAT" {
                e.small_imm(&insn.ops[1], 1, 32, "saturate bit position")? - 1
            } else {
                e.small_imm(&insn.ops[1], 0, 31, "saturate bit position")?
            };
            let n = reg_of(&insn.ops[2])?;
            let (sh, amt) = if insn.ops.len() == 4 {
                match &insn.ops[3] {
                    Op::Shift { t: SRType::Lsl, amt: ShiftAmt::Imm(x) } => (0, e.small_imm(&Op::Imm(x.clone()), 0, 31, "shift")?),
                    Op::Shift { t: SRType::Asr, amt: ShiftAmt::Imm(x) } => (1, e.small_imm(&Op::Imm(x.clone()), 1, 31, "shift")?),
                    _ => return Err(format!("{}: only LSL #n or ASR #n shifts are allowed", insn.base)),
                }
            } else {
                (0, 0)
            };
            let base = if insn.base == "SSAT" { 0xF300 } else { 0xF380 };
            Ok(w32(
                base | (sh << 5) | n as u32,
                ((amt >> 2) << 12) | ((d as u32) << 8) | ((amt & 3) << 6) | sat,
            ))
        }
        "MOVW" | "MOVT" => {
            nops(insn, &[2])?;
            let d = reg_of(&insn.ops[0])?;
            let v = e.imm_of(&insn.ops[1])?;
            if !(0..=0xFFFF).contains(&v) {
                return Err(format!("{}: immediate #{} out of range (0..65535)", insn.base, v));
            }
            let v = v as u32;
            let base = if insn.base == "MOVW" { 0xF240 } else { 0xF2C0 };
            Ok(w32(
                base | (((v >> 11) & 1) << 10) | (v >> 12),
                (((v >> 8) & 7) << 12) | ((d as u32) << 8) | (v & 0xFF),
            ))
        }
        "ADR" => enc_adr(e),
        "LDR" | "LDRB" | "LDRH" | "LDRSB" | "LDRSH" | "STR" | "STRB" | "STRH" | "LDRT" | "LDRBT" | "LDRHT"
        | "LDRSBT" | "LDRSHT" | "STRT" | "STRBT" | "STRHT" => enc_ldst(e),
        "LDRD" | "STRD" => enc_ldst_dual(e),
        "LDREX" | "LDREXB" | "LDREXH" | "STREX" | "STREXB" | "STREXH" => enc_excl(e),
        "LDM" | "LDMIA" | "LDMFD" | "LDMDB" | "LDMEA" | "STM" | "STMIA" | "STMEA" | "STMDB" | "STMFD" => enc_ldm(e),
        "PUSH" | "POP" => enc_pushpop(e),
        "B" => enc_b(e),
        "BL" => {
            nops(insn, &[1])?;
            let t = e.target(&insn.ops[0])?;
            let off = t.wrapping_sub(e.ctx.pc.wrapping_add(4)) as i32;
            if !(-(1 << 24)..(1 << 24)).contains(&off) {
                return Err(format!("BL target out of range (offset {} bytes, limit +/-16 MB)", off));
            }
            let (h1, h2) = b_t4(off);
            Ok(w32(h1, (h2 & !0x1000) | 0xD000))
        }
        "BX" | "BLX" => {
            nops(insn, &[1])?;
            let m = match &insn.ops[0] {
                Op::Reg(r) => *r,
                _ if insn.base == "BLX" => return Err("BLX <label> switches to ARM state, which Cortex-M3 does not have; use BL".into()),
                o => return Err(format!("Expected a register, found {}", describe(o))),
            };
            if insn.base == "BLX" && m == 15 {
                return Err("BLX PC is UNPREDICTABLE".into());
            }
            let base = if insn.base == "BX" { 0x4700 } else { 0x4780 };
            if insn.width == Width::Wide {
                return Err(format!("No 32-bit encoding for '{}'", insn.text));
            }
            Ok(n16(base | ((m as u32) << 3)))
        }
        "CBZ" | "CBNZ" => {
            nops(insn, &[2])?;
            let n = reg_of(&insn.ops[0])?;
            if !lo(n) {
                return Err(format!("{} needs a low register (r0-r7)", insn.base));
            }
            if e.ctx.in_it {
                return Err(format!("{} is not allowed inside an IT block", insn.base));
            }
            let t = e.target(&insn.ops[1])?;
            let off = t.wrapping_sub(e.ctx.pc.wrapping_add(4)) as i32;
            if !(0..=126).contains(&off) {
                return Err(format!(
                    "{} target out of range (offset {} bytes; must be 0..126 bytes forward). Use CMP + B{}",
                    insn.base,
                    off,
                    if insn.base == "CBZ" { "EQ" } else { "NE" }
                ));
            }
            let off = off as u32;
            let nz = (insn.base == "CBNZ") as u32;
            Ok(n16(0xB100 | (nz << 11) | ((off >> 6) << 9) | (((off >> 1) & 0x1F) << 3) | n as u32))
        }
        "TBB" | "TBH" => {
            nops(insn, &[1])?;
            match &insn.ops[0] {
                Op::Mem { rn, off: MemOff::Reg { rm, neg: false, lsl }, wb: false } => {
                    let h = insn.base == "TBH";
                    let sh = match lsl {
                        Some(x) => e.num(x)?,
                        None => 0,
                    };
                    if (h && sh != 1) || (!h && sh != 0) {
                        return Err(if h { "TBH needs [Rn, Rm, LSL #1]".into() } else { "TBB needs [Rn, Rm]".into() });
                    }
                    Ok(w32(0xE8D0 | *rn as u32, 0xF000 | ((h as u32) << 4) | *rm as u32))
                }
                _ => Err(format!("{} needs [Rn, Rm{}]", insn.base, if insn.base == "TBH" { ", LSL #1" } else { "" })),
            }
        }
        "IT" => {
            nops(insn, &[1])?;
            let c = match &insn.ops[0] {
                Op::Expr(x) => parse_cond(&x.to_ascii_uppercase()),
                _ => None,
            }
            .ok_or_else(|| format!("{} needs a condition code (EQ, NE, CS, ...)", insn.text))?;
            let mask = it_mask(c, &insn.it_pattern)?;
            Ok(n16(0xBF00 | ((c as u32) << 4) | mask as u32))
        }
        "NOP" | "YIELD" | "WFE" | "WFI" | "SEV" => {
            nops(insn, &[0])?;
            let h = match insn.base {
                "NOP" => 0,
                "YIELD" => 1,
                "WFE" => 2,
                "WFI" => 3,
                _ => 4,
            };
            if e.wide() {
                Ok(w32(0xF3AF, 0x8000 | h))
            } else {
                Ok(n16(0xBF00 | (h << 4)))
            }
        }
        "SVC" | "BKPT" => {
            nops(insn, &[0, 1])?;
            let v = if insn.ops.is_empty() { 0 } else { e.small_imm(&insn.ops[0], 0, 255, insn.base)? };
            if insn.width == Width::Wide {
                return Err(format!("No 32-bit encoding for {}", insn.base));
            }
            Ok(n16(if insn.base == "SVC" { 0xDF00 } else { 0xBE00 } | v))
        }
        "UDF" => {
            nops(insn, &[0, 1])?;
            let v = if insn.ops.is_empty() { 0 } else { e.small_imm(&insn.ops[0], 0, 65535, "UDF")? };
            if v <= 255 && !e.wide() {
                Ok(n16(0xDE00 | v))
            } else {
                Ok(w32(0xF7F0 | (v >> 12), 0xA000 | (v & 0xFFF)))
            }
        }
        "CPSIE" | "CPSID" => {
            nops(insn, &[1])?;
            if e.ctx.in_it {
                return Err("CPS is not allowed inside an IT block".into());
            }
            let f = match &insn.ops[0] {
                Op::Expr(x) => x.to_ascii_uppercase(),
                _ => String::new(),
            };
            let (i, fl) = match f.as_str() {
                "I" => (1, 0),
                "F" => (0, 1),
                "IF" | "FI" => (1, 1),
                _ => return Err(format!("{} needs 'i', 'f' or 'if'", insn.base)),
            };
            let im = (insn.base == "CPSID") as u32;
            Ok(n16(0xB660 | (im << 4) | (i << 1) | fl))
        }
        "MRS" => {
            nops(insn, &[2])?;
            let d = reg_of(&insn.ops[0])?;
            let (sysm, _) = sysreg(&insn.ops[1], false)?;
            Ok(w32(0xF3EF, 0x8000 | ((d as u32) << 8) | sysm))
        }
        "MSR" => {
            nops(insn, &[2])?;
            let (sysm, mask) = sysreg(&insn.ops[0], true)?;
            let n = reg_of(&insn.ops[1])?;
            Ok(w32(0xF380 | n as u32, 0x8000 | (mask << 10) | sysm))
        }
        "DMB" | "DSB" | "ISB" => {
            nops(insn, &[0, 1])?;
            let opt = if insn.ops.is_empty() {
                15
            } else {
                match &insn.ops[0] {
                    Op::Expr(x) => match x.to_ascii_uppercase().as_str() {
                        "SY" => 15,
                        "ST" => 14,
                        "ISH" => 11,
                        "ISHST" => 10,
                        "NSH" => 7,
                        "NSHST" => 6,
                        "OSH" => 3,
                        "OSHST" => 2,
                        _ => return Err(format!("Unknown barrier option '{}'", x)),
                    },
                    o => e.small_imm(o, 0, 15, "barrier option")?,
                }
            };
            let k = match insn.base {
                "DSB" => 0x8F40,
                "DMB" => 0x8F50,
                _ => 0x8F60,
            };
            Ok(w32(0xF3BF, k | opt))
        }
        "CLREX" => {
            nops(insn, &[0])?;
            Ok(w32(0xF3BF, 0x8F2F))
        }
        "PLD" | "PLI" => enc_preload(e),
        b => Err(format!("Unsupported instruction '{}'", b)),
    }
}

fn three_regs(insn: &Insn) -> EResult<(u8, u8, u8)> {
    if insn.ops.len() == 2 {
        let d = reg_of(&insn.ops[0])?;
        Ok((d, d, reg_of(&insn.ops[1])?))
    } else {
        Ok((reg_of(&insn.ops[0])?, reg_of(&insn.ops[1])?, reg_of(&insn.ops[2])?))
    }
}

fn bitfield(h1: u32, d: u8, lsb: u32, low5: u32) -> Vec<u8> {
    w32(h1, ((lsb >> 2) << 12) | ((d as u32) << 8) | ((lsb & 3) << 6) | low5)
}

pub fn it_mask(firstcond: u8, pattern: &str) -> EResult<u8> {
    let f0 = firstcond & 1;
    if firstcond == 14 && pattern.contains('E') {
        return Err("IT AL cannot have an 'else' (E) slot".into());
    }
    if firstcond == 15 {
        return Err("Invalid IT condition".into());
    }
    let mut mask = 0u8;
    let mut bitpos = 3;
    for ch in pattern.chars() {
        let b = if ch == 'T' { f0 } else { 1 - f0 };
        mask |= b << bitpos;
        bitpos -= 1;
    }
    mask |= 1 << bitpos;
    Ok(mask)
}

fn sysreg(op: &Op, for_msr: bool) -> EResult<(u32, u32)> {
    let name = match op {
        Op::Expr(x) => x.to_ascii_uppercase(),
        o => return Err(format!("Expected a special register name, found {}", describe(o))),
    };
    let (base, suffix) = match name.split_once('_') {
        Some((b, s)) if matches!(b, "APSR" | "IAPSR" | "EAPSR" | "XPSR" | "CPSR") => (b.to_string(), Some(s.to_string())),
        _ => (name.clone(), None),
    };
    let sysm = match base.as_str() {
        "APSR" | "CPSR" => 0,
        "IAPSR" => 1,
        "EAPSR" => 2,
        "XPSR" | "PSR" => 3,
        "IPSR" => 5,
        "EPSR" => 6,
        "IEPSR" => 7,
        "MSP" => 8,
        "PSP" => 9,
        "PRIMASK" => 16,
        "BASEPRI" => 17,
        "BASEPRI_MAX" => 18,
        "FAULTMASK" => 19,
        "CONTROL" => 20,
        _ => return Err(format!("Unknown special register '{}'", name)),
    };
    let mut mask = 2;
    if let Some(sfx) = suffix {
        if !for_msr {
            return Err(format!("MRS does not take a field suffix ('{}')", name));
        }
        mask = match sfx.as_str() {
            "NZCVQ" => 2,
            "G" | "NZCVQG" => return Err("APSR_g requires the DSP extension (not on Cortex-M3)".into()),
            _ => return Err(format!("Unknown PSR field '{}'", name)),
        };
    }
    Ok((sysm, mask))
}

// ---------------------------------------------------------------------------------------
// Data processing
// ---------------------------------------------------------------------------------------

fn enc_mov(e: &E) -> EResult<Vec<u8>> {
    let insn = e.insn;
    nops(insn, &[2, 3])?;
    let d = reg_of(&insn.ops[0])?;
    let s = insn.s;
    if insn.ops.len() == 3 {
        // MOV Rd, Rm, <shift>  ==  <shift> Rd, Rm, ...
        let m = reg_of(&insn.ops[1])?;
        return match &insn.ops[2] {
            Op::Shift { t, amt } => {
                let base = match t {
                    SRType::Lsl => "LSL",
                    SRType::Lsr => "LSR",
                    SRType::Asr => "ASR",
                    SRType::Ror => "ROR",
                    SRType::Rrx => "RRX",
                };
                let ops = match amt {
                    ShiftAmt::Imm(x) => vec![Op::Reg(d), Op::Reg(m), Op::Imm(x.clone())],
                    ShiftAmt::Reg(r) => vec![Op::Reg(d), Op::Reg(m), Op::Reg(*r)],
                    ShiftAmt::Rrx => vec![Op::Reg(d), Op::Reg(m)],
                };
                let i2 = Insn { base, ops, ..insn.clone() };
                enc_shift(&E { insn: &i2, ctx: e.ctx, env: e.env })
            }
            o => Err(format!("Expected a shift, found {}", describe(o))),
        };
    }
    match &insn.ops[1] {
        Op::Reg(m) => {
            let m = *m;
            if s {
                if lo(d) && lo(m) && !e.ctx.in_it && e.may_narrow() {
                    return Ok(n16(((m as u32) << 3) | d as u32)); // LSLS Rd, Rm, #0
                }
                Ok(dp_reg(2, true, 15, d, m, 0, 0))
            } else {
                if e.may_narrow() {
                    return Ok(n16(0x4600 | (((d as u32) >> 3) << 7) | ((m as u32) << 3) | (d as u32 & 7)));
                }
                Ok(dp_reg(2, false, 15, d, m, 0, 0))
            }
        }
        op @ (Op::Imm(_) | Op::Expr(_)) => {
            let v = e.u32_imm(op)?;
            if bad(d) {
                return Err(format!("MOV {}, #imm: destination cannot be SP or PC", reg_name(d)));
            }
            if lo(d) && v <= 255 && e.narrow_s_ok() && e.may_narrow() {
                return Ok(n16(0x2000 | ((d as u32) << 8) | v));
            }
            if s {
                if let Some(i) = modimm(v) {
                    return Ok(dp_imm(2, true, 15, d, i));
                }
                if let Some(i) = modimm(!v) {
                    return Ok(dp_imm(3, true, 15, d, i));
                }
                return Err(format!("MOVS: immediate 0x{:X} cannot be encoded (use LDR {}, =0x{:X})", v, reg_name(d), v));
            }
            if v <= 255 {
                return Ok(dp_imm(2, false, 15, d, v));
            }
            if v <= 0xFFFF && insn.width != Width::Wide {
                return enc_movw(d, v);
            }
            if let Some(i) = modimm(v) {
                return Ok(dp_imm(2, false, 15, d, i));
            }
            if let Some(i) = modimm(!v) {
                return Ok(dp_imm(3, false, 15, d, i));
            }
            if v <= 0xFFFF {
                return enc_movw(d, v);
            }
            Err(format!(
                "Immediate 0x{:X} cannot be encoded in MOV (use LDR {}, =0x{:X} or MOVW/MOVT)",
                v,
                reg_name(d),
                v
            ))
        }
        o => Err(format!("MOV: unexpected operand {}", describe(o))),
    }
}

fn enc_movw(d: u8, v: u32) -> EResult<Vec<u8>> {
    Ok(w32(0xF240 | (((v >> 11) & 1) << 10) | (v >> 12), (((v >> 8) & 7) << 12) | ((d as u32) << 8) | (v & 0xFF)))
}

fn reg_name(r: u8) -> &'static str {
    ["r0", "r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8", "r9", "r10", "r11", "r12", "sp", "lr", "pc"][r as usize & 15]
}

fn enc_mvn(e: &E) -> EResult<Vec<u8>> {
    let insn = e.insn;
    nops(insn, &[2, 3])?;
    let d = reg_of(&insn.ops[0])?;
    match &insn.ops[1] {
        Op::Reg(m) => {
            let m = *m;
            let (ty, imm5) = opt_shift(e, insn.ops.get(2))?;
            if insn.ops.len() == 2 && lo(d) && lo(m) && e.narrow_s_ok() && e.may_narrow() {
                return Ok(n16(0x43C0 | ((m as u32) << 3) | d as u32));
            }
            Ok(dp_reg(3, insn.s, 15, d, m, ty, imm5))
        }
        op => {
            let v = e.u32_imm(op)?;
            if let Some(i) = modimm(v) {
                return Ok(dp_imm(3, insn.s, 15, d, i));
            }
            if let Some(i) = modimm(!v) {
                return Ok(dp_imm(2, insn.s, 15, d, i));
            }
            Err(format!("MVN: immediate 0x{:X} cannot be encoded", v))
        }
    }
}

fn opt_shift(e: &E, op: Option<&Op>) -> EResult<(u32, u32)> {
    match op {
        None => Ok((0, 0)),
        Some(Op::Shift { t, amt: ShiftAmt::Imm(x) }) => {
            let n = e.num(x)?;
            if n < 0 {
                return Err("Negative shift amount".into());
            }
            shift_bits(*t, n as u32)
        }
        Some(Op::Shift { t: SRType::Rrx, amt: ShiftAmt::Rrx }) => Ok((3, 0)),
        Some(Op::Shift { amt: ShiftAmt::Reg(_), .. }) => {
            Err("Register-controlled shifts are only allowed as MOV/LSL/LSR/ASR/ROR Rd, Rm, Rs".into())
        }
        Some(o) => Err(format!("Expected a shift, found {}", describe(o))),
    }
}

/// Split ADD/SUB-like operand lists into (rd, rn, rest, written_with_two_operands).
fn dn_rest(insn: &Insn) -> EResult<(u8, u8, &[Op], bool)> {
    if insn.ops.len() < 2 {
        return Err(format!("'{}' needs at least 2 operands", insn.text));
    }
    let d = reg_of(&insn.ops[0])?;
    match &insn.ops[1] {
        Op::Reg(n) if insn.ops.len() >= 3 && !matches!(insn.ops[2], Op::Shift { .. }) => Ok((d, *n, &insn.ops[2..], false)),
        _ => Ok((d, d, &insn.ops[1..], true)),
    }
}

fn enc_addsub(e: &E) -> EResult<Vec<u8>> {
    let insn = e.insn;
    let (d, n, rest, two_op) = dn_rest(insn)?;
    let sub = insn.base == "SUB";
    let s = insn.s;
    if rest.is_empty() || rest.len() > 2 {
        return Err(format!("'{}': wrong number of operands", insn.text));
    }
    match &rest[0] {
        Op::Reg(m) => {
            let m = *m;
            let (ty, imm5) = opt_shift(e, rest.get(1))?;
            let shifted = rest.len() == 2;
            if !shifted && e.may_narrow() {
                if lo(d) && lo(n) && lo(m) && e.narrow_s_ok() && !(two_op && !sub && !s && !e.ctx.in_it) {
                    return Ok(n16(if sub { 0x1A00 } else { 0x1800 } | ((m as u32) << 6) | ((n as u32) << 3) | d as u32));
                }
                if !sub && !s {
                    // ADD Rdn, Rm (high registers, never sets flags)
                    let pair = if d == n { Some((d, m)) } else if d == m { Some((d, n)) } else { None };
                    if let Some((dn, mm)) = pair {
                        if !(dn == 15 && mm == 15) {
                            return Ok(n16(0x4400 | (((dn as u32) >> 3) << 7) | ((mm as u32) << 3) | (dn as u32 & 7)));
                        }
                    }
                }
            }
            if n == 15 {
                return Err(format!("'{}': PC cannot be the first operand of a 32-bit ADD/SUB", insn.text));
            }
            let op = if sub { 0xD } else { 0x8 };
            Ok(dp_reg(op, s, n, d, m, ty, imm5))
        }
        op @ (Op::Imm(_) | Op::Expr(_)) => {
            if rest.len() != 1 {
                return Err(format!("'{}': unexpected operand after immediate", insn.text));
            }
            let v = e.imm_of(op)?;
            if v < i32::MIN as i64 || v > u32::MAX as i64 {
                return Err(format!("Immediate 0x{:X} does not fit in 32 bits", v));
            }
            let v = v as u32;
            // Try as written, then with the opposite operation and negated immediate.
            let neg = v.wrapping_neg();
            if let Some(b) = addsub_narrow(e, sub, d, n, v, two_op) {
                return Ok(b);
            }
            if v != 0 {
                if let Some(b) = addsub_narrow(e, !sub, d, n, neg, two_op) {
                    return Ok(b);
                }
            }
            if let Some(b) = addsub_wide(e, sub, d, n, v)? {
                return Ok(b);
            }
            if let Some(b) = addsub_wide(e, !sub, d, n, neg)? {
                return Ok(b);
            }
            Err(format!(
                "'{}': immediate #{} cannot be encoded (use a register or LDR =constant)",
                insn.text, v as i32
            ))
        }
        o => Err(format!("'{}': unexpected operand {}", insn.text, describe(o))),
    }
}

fn addsub_narrow(e: &E, sub: bool, d: u8, n: u8, v: u32, two_op: bool) -> Option<Vec<u8>> {
    if !e.may_narrow() {
        return None;
    }
    let s = e.insn.s;
    if lo(d) && lo(n) && e.narrow_s_ok() {
        if !two_op && v <= 7 {
            return Some(n16(if sub { 0x1E00 } else { 0x1C00 } | (v << 6) | ((n as u32) << 3) | d as u32));
        }
        if d == n && v <= 255 {
            return Some(n16(if sub { 0x3800 } else { 0x3000 } | ((d as u32) << 8) | v));
        }
    }
    if !s && v & 3 == 0 {
        if !sub && n == 13 && lo(d) && v <= 1020 {
            return Some(n16(0xA800 | ((d as u32) << 8) | (v >> 2)));
        }
        if d == 13 && n == 13 && v <= 508 {
            return Some(n16(if sub { 0xB080 } else { 0xB000 } | (v >> 2)));
        }
    }
    None
}

fn addsub_wide(e: &E, sub: bool, d: u8, n: u8, v: u32) -> EResult<Option<Vec<u8>>> {
    let s = e.insn.s;
    if n == 15 {
        if s {
            return Err("ADDS/SUBS with PC as operand is not allowed".into());
        }
        // ADR.W
        if v <= 4095 {
            return Ok(Some(plain_imm12(if sub { 0xF2AF } else { 0xF20F } & !0xF, 15, d, v)));
        }
        return Ok(None);
    }
    if let Some(i) = modimm(v) {
        return Ok(Some(dp_imm(if sub { 0xD } else { 0x8 }, s, n, d, i)));
    }
    if !s && v <= 4095 {
        return Ok(Some(plain_imm12(if sub { 0xF2A0 } else { 0xF200 }, n, d, v)));
    }
    Ok(None)
}

fn enc_addw(e: &E) -> EResult<Vec<u8>> {
    let insn = e.insn;
    let (d, n, rest, _) = dn_rest(insn)?;
    if rest.len() != 1 {
        return Err(format!("'{}' needs Rd, Rn, #imm12", insn.text));
    }
    let v = e.small_imm(&rest[0], 0, 4095, insn.base)?;
    Ok(plain_imm12(if insn.base == "SUBW" { 0xF2A0 } else { 0xF200 }, n, d, v))
}

fn enc_logic(e: &E) -> EResult<Vec<u8>> {
    let insn = e.insn;
    let (d, n, rest, _) = dn_rest(insn)?;
    let s = insn.s;
    let (op, alu16, commut) = match insn.base {
        "AND" => (0x0, Some(0x0), true),
        "BIC" => (0x1, Some(0xE), false),
        "ORR" => (0x2, Some(0xC), true),
        "ORN" => (0x3, None, false),
        "EOR" => (0x4, Some(0x1), true),
        "ADC" => (0xA, Some(0x5), true),
        "SBC" => (0xB, Some(0x6), false),
        _ => (0xE, None, false), // RSB
    };
    if rest.is_empty() || rest.len() > 2 {
        return Err(format!("'{}': wrong number of operands", insn.text));
    }
    match &rest[0] {
        Op::Reg(m) => {
            let m = *m;
            let (ty, imm5) = opt_shift(e, rest.get(1))?;
            let plain = rest.len() == 1;
            if let Some(a) = alu16 {
                if plain && e.may_narrow() && lo(d) && lo(n) && lo(m) && e.narrow_s_ok() {
                    if d == n {
                        return Ok(n16(0x4000 | (a << 6) | ((m as u32) << 3) | d as u32));
                    }
                    if commut && d == m {
                        return Ok(n16(0x4000 | (a << 6) | ((n as u32) << 3) | d as u32));
                    }
                }
            }
            Ok(dp_reg(op, s, n, d, m, ty, imm5))
        }
        o => {
            if rest.len() != 1 {
                return Err(format!("'{}': unexpected operand after immediate", insn.text));
            }
            let v = e.u32_imm(o)?;
            if insn.base == "RSB" && v == 0 && lo(d) && lo(n) && e.narrow_s_ok() && e.may_narrow() {
                return Ok(n16(0x4240 | ((n as u32) << 3) | d as u32));
            }
            if let Some(i) = modimm(v) {
                return Ok(dp_imm(op, s, n, d, i));
            }
            let alt = match insn.base {
                "AND" => Some(0x1),
                "BIC" => Some(0x0),
                "ORR" => Some(0x3),
                "ORN" => Some(0x2),
                "ADC" => Some(0xB),
                "SBC" => Some(0xA),
                _ => None,
            };
            if let (Some(a), Some(i)) = (alt, modimm(!v)) {
                return Ok(dp_imm(a, s, n, d, i));
            }
            Err(format!("'{}': immediate 0x{:X} cannot be encoded as a Thumb-2 modified immediate", insn.text, v))
        }
    }
}

fn enc_compare(e: &E) -> EResult<Vec<u8>> {
    let insn = e.insn;
    if insn.s {
        return Err(format!("{} always sets flags; remove the S suffix", insn.base));
    }
    if insn.ops.len() < 2 || insn.ops.len() > 3 {
        return Err(format!("'{}' needs Rn, Op2", insn.text));
    }
    let n = reg_of(&insn.ops[0])?;
    let (op, n16_reg) = match insn.base {
        "CMP" => (0xD, Some(0x4280)),
        "CMN" => (0x8, Some(0x42C0)),
        "TST" => (0x0, Some(0x4200)),
        _ => (0x4, None),
    };
    match &insn.ops[1] {
        Op::Reg(m) => {
            let m = *m;
            let (ty, imm5) = opt_shift(e, insn.ops.get(2))?;
            // an explicitly written shift (even LSL #0) selects the 32-bit form, as in llvm-mc
            let plain = insn.ops.get(2).is_none();
            if plain && e.may_narrow() {
                if let Some(b) = n16_reg {
                    if lo(n) && lo(m) {
                        return Ok(n16(b | ((m as u32) << 3) | n as u32));
                    }
                }
                if insn.base == "CMP" && !(lo(n) && lo(m)) && n != 15 && m != 15 {
                    return Ok(n16(0x4500 | (((n as u32) >> 3) << 7) | ((m as u32) << 3) | (n as u32 & 7)));
                }
            }
            Ok(dp_reg(op, true, n, 15, m, ty, imm5))
        }
        o => {
            if insn.ops.len() != 2 {
                return Err(format!("'{}': unexpected operand", insn.text));
            }
            let v = e.u32_imm(o)?;
            if insn.base == "CMP" && lo(n) && v <= 255 && e.may_narrow() {
                return Ok(n16(0x2800 | ((n as u32) << 8) | v));
            }
            if let Some(i) = modimm(v) {
                return Ok(dp_imm(op, true, n, 15, i));
            }
            let alt = match insn.base {
                "CMP" => Some(0x8),
                "CMN" => Some(0xD),
                _ => None,
            };
            if let Some(a) = alt {
                let nv = v.wrapping_neg();
                if let Some(i) = modimm(nv) {
                    return Ok(dp_imm(a, true, n, 15, i));
                }
            }
            Err(format!("'{}': immediate 0x{:X} cannot be encoded", insn.text, v))
        }
    }
}

fn enc_shift(e: &E) -> EResult<Vec<u8>> {
    let insn = e.insn;
    let t = match insn.base {
        "LSL" => SRType::Lsl,
        "LSR" => SRType::Lsr,
        "ASR" => SRType::Asr,
        "ROR" => SRType::Ror,
        _ => SRType::Rrx,
    };
    let s = insn.s;
    if t == SRType::Rrx {
        nops(insn, &[1, 2])?;
        let d = reg_of(&insn.ops[0])?;
        let m = if insn.ops.len() == 2 { reg_of(&insn.ops[1])? } else { d };
        return Ok(dp_reg(2, s, 15, d, m, 3, 0));
    }
    nops(insn, &[2, 3])?;
    let d = reg_of(&insn.ops[0])?;
    let (m_or_n, last) = if insn.ops.len() == 3 { (reg_of(&insn.ops[1])?, &insn.ops[2]) } else { (d, &insn.ops[1]) };
    match last {
        Op::Reg(rs) => {
            // register shift: Rd = Rn shift Rs
            let (n, m) = (m_or_n, *rs);
            let a = match t {
                SRType::Lsl => 2,
                SRType::Lsr => 3,
                SRType::Asr => 4,
                _ => 7,
            };
            if lo(d) && lo(n) && lo(m) && d == n && e.narrow_s_ok() && e.may_narrow() {
                return Ok(n16(0x4000 | (a << 6) | ((m as u32) << 3) | d as u32));
            }
            Ok(w32(0xFA00 | (t.bits() << 5) | ((s as u32) << 4) | n as u32, 0xF000 | ((d as u32) << 8) | m as u32))
        }
        op => {
            let m = m_or_n;
            let n = e.imm_of(op)?;
            if n < 0 {
                return Err("Negative shift amount".into());
            }
            let n = n as u32;
            let (ty, imm5) = shift_bits(t, n)?;
            // LSL #0 narrow is MOVS Rd, Rm (T2), which is not allowed inside an IT block
            let movs_t2 = ty == 0 && imm5 == 0;
            if t != SRType::Ror && lo(d) && lo(m) && e.narrow_s_ok() && e.may_narrow() && !(movs_t2 && e.ctx.in_it) {
                return Ok(n16((ty << 11) | (imm5 << 6) | ((m as u32) << 3) | d as u32));
            }
            Ok(dp_reg(2, s, 15, d, m, ty, imm5))
        }
    }
}

fn enc_mul(e: &E) -> EResult<Vec<u8>> {
    let insn = e.insn;
    nops(insn, &[2, 3])?;
    let (d, n, m) = three_regs(insn)?;
    let other = if d == m { Some(n) } else if d == n { Some(m) } else { None };
    if let Some(o) = other {
        if lo(d) && lo(o) && e.narrow_s_ok() && e.may_narrow() {
            return Ok(n16(0x4340 | ((o as u32) << 3) | d as u32));
        }
    }
    if insn.s {
        return Err(
            "MULS has only a 16-bit form: Rd must equal one source, all registers r0-r7, and it cannot be inside an IT block (use MUL)"
                .into(),
        );
    }
    Ok(w32(0xFB00 | n as u32, 0xF000 | ((d as u32) << 8) | m as u32))
}

fn enc_extend(e: &E) -> EResult<Vec<u8>> {
    let insn = e.insn;
    nops(insn, &[2, 3])?;
    let (d, m) = (reg_of(&insn.ops[0])?, reg_of(&insn.ops[1])?);
    let rot = match insn.ops.get(2) {
        None => 0,
        Some(Op::Shift { t: SRType::Ror, amt: ShiftAmt::Imm(x) }) => {
            let r = e.num(x)?;
            if ![0, 8, 16, 24].contains(&r) {
                return Err("Rotation must be ROR #0, #8, #16 or #24".into());
            }
            (r / 8) as u32
        }
        Some(o) => return Err(format!("Expected ROR #n, found {}", describe(o))),
    };
    if rot == 0 && lo(d) && lo(m) && e.may_narrow() {
        let b = match insn.base {
            "SXTH" => 0xB200,
            "SXTB" => 0xB240,
            "UXTH" => 0xB280,
            _ => 0xB2C0,
        };
        return Ok(n16(b | ((m as u32) << 3) | d as u32));
    }
    let h1 = match insn.base {
        "SXTH" => 0xFA0F,
        "UXTH" => 0xFA1F,
        "SXTB" => 0xFA4F,
        _ => 0xFA5F,
    };
    Ok(w32(h1, 0xF080 | ((d as u32) << 8) | (rot << 4) | m as u32))
}

// ---------------------------------------------------------------------------------------
// Branches / ADR
// ---------------------------------------------------------------------------------------

fn b_t4(off: i32) -> (u32, u32) {
    let o = off as u32;
    let s = (o >> 24) & 1;
    let i1 = (o >> 23) & 1;
    let i2 = (o >> 22) & 1;
    let j1 = (1 - i1) ^ s;
    let j2 = (1 - i2) ^ s;
    let imm10 = (o >> 12) & 0x3FF;
    let imm11 = (o >> 1) & 0x7FF;
    (0xF000 | (s << 10) | imm10, 0x9000 | (j1 << 13) | (j2 << 11) | imm11)
}

fn enc_b(e: &E) -> EResult<Vec<u8>> {
    let insn = e.insn;
    nops(insn, &[1])?;
    let t = e.target(&insn.ops[0])?;
    let off = t.wrapping_sub(e.ctx.pc.wrapping_add(4)) as i32;
    let cond = insn.cond;
    if cond != 14 && !e.ctx.in_it {
        if (-256..=254).contains(&off) && e.may_narrow() {
            return Ok(n16(0xD000 | ((cond as u32) << 8) | ((off as u32 >> 1) & 0xFF)));
        }
        if !(-(1 << 20)..(1 << 20)).contains(&off) {
            return Err(format!(
                "B{} target out of range (offset {} bytes, limit +/-1 MB)",
                cond_suffix(cond),
                off
            ));
        }
        let o = off as u32;
        let s = (o >> 20) & 1;
        let j2 = (o >> 19) & 1;
        let j1 = (o >> 18) & 1;
        let imm6 = (o >> 12) & 0x3F;
        let imm11 = (o >> 1) & 0x7FF;
        return Ok(w32(0xF000 | (s << 10) | ((cond as u32) << 6) | imm6, 0x8000 | (j1 << 13) | (j2 << 11) | imm11));
    }
    if (-2048..=2046).contains(&off) && e.may_narrow() {
        return Ok(n16(0xE000 | ((off as u32 >> 1) & 0x7FF)));
    }
    if !(-(1 << 24)..(1 << 24)).contains(&off) {
        return Err(format!("B target out of range (offset {} bytes, limit +/-16 MB)", off));
    }
    let (h1, h2) = b_t4(off);
    Ok(w32(h1, h2))
}

fn enc_adr(e: &E) -> EResult<Vec<u8>> {
    let insn = e.insn;
    nops(insn, &[2])?;
    let d = reg_of(&insn.ops[0])?;
    let t = match &insn.ops[1] {
        Op::Expr(x) | Op::Imm(x) => e.val(x)?.v as u32,
        o => return Err(format!("ADR needs a label, found {}", describe(o))),
    };
    let base = e.ctx.pc.wrapping_add(4) & !3;
    let off = t.wrapping_sub(base) as i32;
    if lo(d) && (0..=1020).contains(&off) && off & 3 == 0 && e.may_narrow() {
        return Ok(n16(0xA000 | ((d as u32) << 8) | (off as u32 >> 2)));
    }
    if (0..=4095).contains(&off) {
        return Ok(plain_imm12(0xF200, 15, d, off as u32));
    }
    if (-4095..0).contains(&off) {
        return Ok(plain_imm12(0xF2A0, 15, d, (-off) as u32));
    }
    Err(format!("ADR target out of range (offset {} bytes, limit +/-4095)", off))
}

// ---------------------------------------------------------------------------------------
// Loads / stores
// ---------------------------------------------------------------------------------------

struct LsKind {
    load: bool,
    /// 0 = byte, 1 = halfword, 2 = word
    size: u32,
    signed: bool,
    unpriv: bool,
}

fn ls_kind(base: &str) -> LsKind {
    let load = base.starts_with("LDR");
    let unpriv = base.ends_with('T') && base.len() > 3 && !base.ends_with("ST");
    let core = base.trim_end_matches('T');
    let tail = &core[3..];
    let (size, signed) = match tail {
        "B" => (0, false),
        "H" => (1, false),
        "SB" => (0, true),
        "SH" => (1, true),
        _ => (2, false),
    };
    LsKind { load, size, signed, unpriv }
}

fn enc_ldst(e: &E) -> EResult<Vec<u8>> {
    let insn = e.insn;
    let k = ls_kind(insn.base);
    nops(insn, &[2, 3])?;
    let t = reg_of(&insn.ops[0])?;
    let scale = 1u32 << k.size;
    let bit_s = (k.signed as u32) << 8;
    let lbit = (k.load as u32) << 4;
    let szb = k.size << 5;

    // Literal forms
    match &insn.ops[1] {
        Op::Lit(x) => {
            if !k.load || k.size != 2 || k.signed || k.unpriv {
                return Err("'=expr' is only allowed with LDR".into());
            }
            let v = e.val(x)?;
            if !literal_needs_pool(&v, Some(t)) {
                let u = v.v as u32;
                if let Some(i) = modimm(u) {
                    return Ok(dp_imm(2, false, 15, t, i));
                }
                if let Some(i) = modimm(!u) {
                    return Ok(dp_imm(3, false, 15, t, i));
                }
                return enc_movw(t, u);
            }
            let addr = e.ctx.lit_addr.ok_or("internal: literal pool slot missing")?;
            return ldr_literal(e, t, addr, k.size, k.signed);
        }
        Op::Expr(x) if insn.ops.len() == 2 => {
            if !k.load || k.unpriv {
                return Err("PC-relative stores are not allowed in Thumb".into());
            }
            let addr = e.val(x)?.v as u32;
            return ldr_literal(e, t, addr, k.size, k.signed);
        }
        _ => {}
    }

    let (rn, off, wb) = match &insn.ops[1] {
        Op::Mem { rn, off, wb } => (*rn, off.clone(), *wb),
        o => return Err(format!("Expected a memory operand [Rn, ...], found {}", describe(o))),
    };
    let post = if insn.ops.len() == 3 {
        if wb || off != MemOff::None {
            return Err("Post-indexed addressing must be written [Rn], #offset".into());
        }
        match &insn.ops[2] {
            Op::Imm(x) => Some(e.num(x)?),
            Op::Reg(_) => return Err("Post-indexed register offsets are not available in Thumb".into()),
            o => return Err(format!("Expected #offset, found {}", describe(o))),
        }
    } else {
        None
    };
    if k.load && t == 15 && k.size != 2 {
        return Err(format!("{} PC is not allowed (it encodes PLD/PLI)", insn.base));
    }

    if let Some(p) = post {
        if k.unpriv {
            return Err("Unprivileged loads/stores only support [Rn, #imm]".into());
        }
        if !(-255..=255).contains(&p) {
            return Err(format!("Post-index offset #{} out of range (-255..255)", p));
        }
        if rn == t {
            return Err("Writeback with Rn == Rt is UNPREDICTABLE".into());
        }
        let (u, imm) = if p < 0 { (0, (-p) as u32) } else { (1, p as u32) };
        return Ok(w32(0xF800 | bit_s | szb | lbit | rn as u32, ((t as u32) << 12) | 0x900 | (u << 9) | imm));
    }

    match off {
        MemOff::Reg { rm, neg, lsl } => {
            if neg {
                return Err("Negative register offsets are not available in Thumb".into());
            }
            if wb {
                return Err("Writeback is not allowed with a register offset in Thumb".into());
            }
            if k.unpriv {
                return Err("Unprivileged loads/stores only support [Rn, #imm]".into());
            }
            let sh = match lsl {
                Some(x) => e.num(&x)?,
                None => 0,
            };
            if !(0..=3).contains(&sh) {
                return Err("Register offset shift must be LSL #0-3".into());
            }
            if sh == 0 && lo(t) && lo(rn) && lo(rm) && e.may_narrow() {
                let op = match (k.load, k.size, k.signed) {
                    (false, 2, _) => 0x5000,
                    (false, 1, _) => 0x5200,
                    (false, 0, _) => 0x5400,
                    (true, 0, true) => 0x5600,
                    (true, 2, _) => 0x5800,
                    (true, 1, false) => 0x5A00,
                    (true, 0, false) => 0x5C00,
                    _ => 0x5E00,
                };
                return Ok(n16(op | ((rm as u32) << 6) | ((rn as u32) << 3) | t as u32));
            }
            Ok(w32(0xF800 | bit_s | szb | lbit | rn as u32, ((t as u32) << 12) | ((sh as u32) << 4) | rm as u32))
        }
        MemOff::None | MemOff::Imm(_) => {
            let o = match &off {
                MemOff::Imm(x) => e.num(x)?,
                _ => 0,
            };
            if rn == 15 {
                if wb {
                    return Err("Writeback with PC base is not allowed".into());
                }
                if !k.load {
                    return Err("PC-relative stores are not allowed in Thumb".into());
                }
                let addr = (e.ctx.pc.wrapping_add(4) & !3).wrapping_add(o as u32);
                return ldr_literal(e, t, addr, k.size, k.signed);
            }
            if k.unpriv {
                if wb || !(0..=255).contains(&o) {
                    return Err("Unprivileged loads/stores need [Rn, #0..255] without writeback".into());
                }
                return Ok(w32(0xF800 | bit_s | szb | lbit | rn as u32, ((t as u32) << 12) | 0xE00 | o as u32));
            }
            if wb {
                if !(-255..=255).contains(&o) {
                    return Err(format!("Pre-index offset #{} out of range (-255..255)", o));
                }
                if rn == t {
                    return Err("Writeback with Rn == Rt is UNPREDICTABLE".into());
                }
                let (u, imm) = if o < 0 { (0, (-o) as u32) } else { (1, o as u32) };
                return Ok(w32(0xF800 | bit_s | szb | lbit | rn as u32, ((t as u32) << 12) | 0xD00 | (u << 9) | imm));
            }
            if o >= 0 && e.may_narrow() && !k.signed {
                let o = o as u32;
                if lo(t) && lo(rn) && o.is_multiple_of(scale) && o / scale <= 31 {
                    let op = match (k.load, k.size) {
                        (false, 2) => 0x6000,
                        (true, 2) => 0x6800,
                        (false, 0) => 0x7000,
                        (true, 0) => 0x7800,
                        (false, _) => 0x8000,
                        (true, _) => 0x8800,
                    };
                    return Ok(n16(op | ((o / scale) << 6) | ((rn as u32) << 3) | t as u32));
                }
                if k.size == 2 && rn == 13 && lo(t) && o.is_multiple_of(4) && o <= 1020 {
                    return Ok(n16(if k.load { 0x9800 } else { 0x9000 } | ((t as u32) << 8) | (o >> 2)));
                }
            }
            if (0..=4095).contains(&o) {
                return Ok(w32(0xF880 | bit_s | szb | lbit | rn as u32, ((t as u32) << 12) | o as u32));
            }
            if (-255..0).contains(&o) {
                return Ok(w32(0xF800 | bit_s | szb | lbit | rn as u32, ((t as u32) << 12) | 0xC00 | (-o) as u32));
            }
            Err(format!("Offset #{} out of range (-255..4095)", o))
        }
    }
}

fn ldr_literal(e: &E, t: u8, addr: u32, size: u32, signed: bool) -> EResult<Vec<u8>> {
    let base = e.ctx.pc.wrapping_add(4) & !3;
    let off = addr.wrapping_sub(base) as i32;
    if size == 2 && lo(t) && (0..=1020).contains(&off) && off & 3 == 0 && e.may_narrow() {
        return Ok(n16(0x4800 | ((t as u32) << 8) | (off as u32 >> 2)));
    }
    if !(-4095..=4095).contains(&off) {
        return Err(format!(
            "Literal out of range ({} bytes from the LDR; limit +/-4095). Add an LTORG directive closer to this instruction",
            off
        ));
    }
    let u = (off >= 0) as u32;
    let bit_s = (signed as u32) << 8;
    Ok(w32(0xF81F | bit_s | (size << 5) | (u << 7) | 0x10, ((t as u32) << 12) | off.unsigned_abs()))
}

fn enc_ldst_dual(e: &E) -> EResult<Vec<u8>> {
    let insn = e.insn;
    nops(insn, &[3, 4])?;
    let load = insn.base == "LDRD";
    let (t, t2) = (reg_of(&insn.ops[0])?, reg_of(&insn.ops[1])?);
    let (rn, off, wb) = match &insn.ops[2] {
        Op::Mem { rn, off, wb } => (*rn, off.clone(), *wb),
        Op::Expr(_) => return Err("LDRD with a label is not supported; use [Rn, #imm]".into()),
        o => return Err(format!("Expected a memory operand, found {}", describe(o))),
    };
    let (o, p, w) = if insn.ops.len() == 4 {
        if wb || off != MemOff::None {
            return Err("Post-indexed addressing must be written [Rn], #offset".into());
        }
        (e.imm_of(&insn.ops[3])?, 0, 1)
    } else {
        let o = match off {
            MemOff::None => 0,
            MemOff::Imm(x) => e.num(&x)?,
            MemOff::Reg { .. } => return Err(format!("{} does not support register offsets", insn.base)),
        };
        (o, 1, wb as u32)
    };
    if o % 4 != 0 || !(-1020..=1020).contains(&o) {
        return Err(format!("{} offset #{} must be a multiple of 4 in -1020..1020", insn.base, o));
    }
    if load && t == t2 {
        return Err("LDRD: Rt and Rt2 must be different".into());
    }
    if w == 1 && (rn == t || rn == t2) {
        return Err("Writeback with the base register in the transfer list is UNPREDICTABLE".into());
    }
    let u = (o >= 0) as u32;
    let imm8 = (o.unsigned_abs() as u32) >> 2;
    Ok(w32(
        0xE840 | (p << 8) | (u << 7) | (w << 5) | ((load as u32) << 4) | rn as u32,
        ((t as u32) << 12) | ((t2 as u32) << 8) | imm8,
    ))
}

fn enc_excl(e: &E) -> EResult<Vec<u8>> {
    let insn = e.insn;
    let store = insn.base.starts_with("STR");
    let sz = if insn.base.ends_with('B') {
        0
    } else if insn.base.ends_with('H') {
        1
    } else {
        2
    };
    nops(insn, if store { &[3] } else { &[2] })?;
    let (d, t, mem) = if store {
        (reg_of(&insn.ops[0])?, reg_of(&insn.ops[1])?, &insn.ops[2])
    } else {
        (0, reg_of(&insn.ops[0])?, &insn.ops[1])
    };
    let (rn, off) = match mem {
        Op::Mem { rn, off, wb: false } => (*rn, off.clone()),
        _ => return Err(format!("{} needs [Rn{}]", insn.base, if sz == 2 { ", #imm" } else { "" })),
    };
    let o = match off {
        MemOff::None => 0,
        MemOff::Imm(x) if sz == 2 => e.num(&x)?,
        _ => return Err(format!("{} needs [Rn{}]", insn.base, if sz == 2 { ", #imm" } else { "" })),
    };
    if o % 4 != 0 || !(0..=1020).contains(&o) {
        return Err("Exclusive access offset must be a multiple of 4 in 0..1020".into());
    }
    if store && (d == t || d == rn) {
        return Err("STREX: the status register must differ from Rt and Rn".into());
    }
    let o = o as u32;
    Ok(match (store, sz) {
        (false, 2) => w32(0xE850 | rn as u32, ((t as u32) << 12) | 0xF00 | (o >> 2)),
        (true, 2) => w32(0xE840 | rn as u32, ((t as u32) << 12) | ((d as u32) << 8) | (o >> 2)),
        (false, s) => w32(0xE8D0 | rn as u32, ((t as u32) << 12) | 0xF0F | (if s == 0 { 0x40 } else { 0x50 })),
        (true, s) => w32(0xE8C0 | rn as u32, ((t as u32) << 12) | 0xF00 | (if s == 0 { 0x40 } else { 0x50 }) | d as u32),
    })
}

fn enc_ldm(e: &E) -> EResult<Vec<u8>> {
    let insn = e.insn;
    nops(insn, &[2])?;
    let load = insn.base.starts_with("LDM");
    let db = matches!(insn.base, "LDMDB" | "LDMEA" | "STMDB" | "STMFD");
    let (rn, wb) = match &insn.ops[0] {
        Op::Reg(r) => (*r, false),
        Op::RegWb(r) => (*r, true),
        o => return Err(format!("Expected a base register, found {}", describe(o))),
    };
    let list = match &insn.ops[1] {
        Op::List(l) => *l as u32,
        o => return Err(format!("Expected a register list, found {}", describe(o))),
    };
    let in_list = list & (1 << rn) != 0;
    if !db && lo(rn) && list & !0xFF == 0 && e.may_narrow() {
        if load && (wb != in_list) {
            return Ok(n16(0xC800 | ((rn as u32) << 8) | list));
        }
        if !load && wb {
            if in_list && list & ((1 << rn) - 1) != 0 {
                return Err("STM with writeback: the base register must be the lowest register in the list".into());
            }
            return Ok(n16(0xC000 | ((rn as u32) << 8) | list));
        }
    }
    if list.count_ones() < 2 {
        return Err(format!("{} with a single register is UNPREDICTABLE in its 32-bit form; use LDR/STR", insn.base));
    }
    if rn == 15 {
        return Err("PC cannot be the base register".into());
    }
    if list & (1 << 13) != 0 {
        return Err("SP cannot be in the register list".into());
    }
    if load && list & 0xC000 == 0xC000 {
        return Err("LDM cannot load both LR and PC".into());
    }
    if !load && list & (1 << 15) != 0 {
        return Err("STM cannot store PC".into());
    }
    if wb && in_list {
        return Err("Writeback with the base register in the list is UNPREDICTABLE".into());
    }
    let base = match (load, db) {
        (false, false) => 0xE880,
        (true, false) => 0xE890,
        (false, true) => 0xE900,
        (true, true) => 0xE910,
    };
    Ok(w32(base | ((wb as u32) << 5) | rn as u32, list))
}

fn enc_pushpop(e: &E) -> EResult<Vec<u8>> {
    let insn = e.insn;
    nops(insn, &[1])?;
    let push = insn.base == "PUSH";
    let list = match &insn.ops[0] {
        Op::List(l) => *l as u32,
        o => return Err(format!("{} needs a register list, found {}", insn.base, describe(o))),
    };
    let extra = if push { 1 << 14 } else { 1 << 15 };
    if list & !(0xFF | extra) == 0 && e.may_narrow() {
        let m = ((list & extra != 0) as u32) << 8;
        return Ok(n16(if push { 0xB400 } else { 0xBC00 } | m | (list & 0xFF)));
    }
    if list.count_ones() == 1 {
        let t = list.trailing_zeros();
        if t == 13 || (push && t == 15) {
            return Err(format!("{} {{{}}} is UNPREDICTABLE", insn.base, reg_name(t as u8)));
        }
        return Ok(if push { w32(0xF84D, (t << 12) | 0xD04) } else { w32(0xF85D, (t << 12) | 0xB04) });
    }
    if list & (1 << 13) != 0 {
        return Err("SP cannot be in a PUSH/POP list".into());
    }
    if push && list & (1 << 15) != 0 {
        return Err("PUSH cannot store PC".into());
    }
    if !push && list & 0xC000 == 0xC000 {
        return Err("POP cannot load both LR and PC".into());
    }
    Ok(w32(if push { 0xE92D } else { 0xE8BD }, list))
}

fn enc_preload(e: &E) -> EResult<Vec<u8>> {
    let insn = e.insn;
    nops(insn, &[1])?;
    let s = ((insn.base == "PLI") as u32) << 8;
    match &insn.ops[0] {
        Op::Mem { rn, off, wb: false } => match off {
            MemOff::None | MemOff::Imm(_) => {
                let o = match off {
                    MemOff::Imm(x) => e.num(x)?,
                    _ => 0,
                };
                if (0..=4095).contains(&o) {
                    Ok(w32(0xF890 | s | *rn as u32, 0xF000 | o as u32))
                } else if (-255..0).contains(&o) {
                    Ok(w32(0xF810 | s | *rn as u32, 0xFC00 | (-o) as u32))
                } else {
                    Err("Preload offset out of range (-255..4095)".into())
                }
            }
            MemOff::Reg { rm, neg: false, lsl } => {
                let sh = match lsl {
                    Some(x) => e.num(x)?,
                    None => 0,
                };
                if !(0..=3).contains(&sh) {
                    return Err("Register offset shift must be LSL #0-3".into());
                }
                Ok(w32(0xF810 | s | *rn as u32, 0xF000 | ((sh as u32) << 4) | *rm as u32))
            }
            _ => Err("Negative register offsets are not available in Thumb".into()),
        },
        o => Err(format!("{} needs a memory operand, found {}", insn.base, describe(o))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modified_immediates() {
        assert_eq!(modimm(0xFF), Some(0xFF));
        assert_eq!(modimm(0x00AB_00AB), Some(0x1AB));
        assert_eq!(modimm(0xAB00_AB00), Some(0x2AB));
        assert_eq!(modimm(0xABAB_ABAB), Some(0x3AB));
        assert_eq!(modimm(0xFFFF_FFFF), Some(0x3FF));
        assert_eq!(modimm(0x0000_0100), Some(0xF80));
        assert_eq!(modimm(0x00F0_0000), Some(0x870)); // 15 << 20, as llvm-mc (f421 0170)
        assert_eq!(modimm(0x101), None);
        for imm12 in 0..4096u32 {
            if let Some((v, _)) = crate::thumb::thumb_expand_imm(imm12) {
                let back = modimm(v).unwrap();
                assert_eq!(crate::thumb::thumb_expand_imm(back).unwrap().0, v);
            }
        }
    }

    #[test]
    fn mnemonic_splitting() {
        assert_eq!(split_mnemonic("BLS").map(|m| (m.0, m.2)), Some(("B", 9)));
        assert_eq!(split_mnemonic("BL").map(|m| m.0), Some("BL"));
        assert_eq!(split_mnemonic("BICS").map(|m| (m.0, m.1)), Some(("BIC", true)));
        assert_eq!(split_mnemonic("ADDSEQ").map(|m| (m.0, m.1, m.2)), Some(("ADD", true, 0)));
        assert_eq!(split_mnemonic("ADDEQS").map(|m| (m.0, m.1, m.2)), Some(("ADD", true, 0)));
        assert_eq!(split_mnemonic("LDRSB").map(|m| m.0), Some("LDRSB"));
        assert_eq!(split_mnemonic("ldr.w").map(|m| (m.0, m.3)), Some(("LDR", Width::Wide)));
        assert_eq!(split_mnemonic("ITTE").map(|m| m.4), Some("TE".to_string()));
        assert_eq!(split_mnemonic("TEQ").map(|m| m.0), Some("TEQ"));
        assert_eq!(split_mnemonic("BLXNE").map(|m| (m.0, m.2)), Some(("BLX", 1)));
        assert!(split_mnemonic("stop").is_none());
        assert!(split_mnemonic("_main").is_none());
    }
}
