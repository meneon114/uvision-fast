//! Thumb / Thumb-2 disassembler (Keil-style text) built on the shared decoder in
//! [`crate::thumb`], so the debugger shows exactly what the simulator executes.
//!
//! Output is UAL that the built-in assembler accepts back unchanged at the same address:
//! 32-bit encodings that have a 16-bit counterpart carry `.W`, branch / ADR / literal
//! targets are absolute addresses (with the symbol name in a comment), and IT blocks are
//! tracked so conditional suffixes and flag setting are shown correctly. ELF mapping
//! symbols (`$t` / `$d`) separate code from data such as literal pools.

use crate::elf::{DisasmLine, ElfSegment, ElfSymbol, LineEntry};
use crate::thumb::{self, Addr, Alu, Barrier, ExtKind, HintKind, Instr, LongMul, RevKind, SRType, Shift, Size};

/// Disassemble the flash image. `mapping` holds ELF mapping symbols (address, is_code),
/// sorted by address; without them everything before the first function symbol is data.
pub fn disassemble_elf(
    segments: &[ElfSegment],
    lines: &[LineEntry],
    symbols: &[ElfSymbol],
    entry: u32,
    mapping: &[(u32, bool)],
) -> Vec<DisasmLine> {
    let mut out = Vec::new();
    let Some(seg) = segments
        .iter()
        .find(|s| s.paddr >= 0x0800_0000 && s.paddr < 0x1000_0000 && !s.data.is_empty())
        .or_else(|| segments.iter().find(|s| !s.data.is_empty()))
    else {
        return out;
    };
    let base = seg.paddr;
    let bytes = &seg.data;
    let end = base + bytes.len() as u32;

    let local_map: Vec<(u32, bool)> = mapping.iter().copied().filter(|(a, _)| *a >= base && *a < end).collect();
    let code_start = symbols
        .iter()
        .filter(|s| s.is_func && s.addr >= base && s.addr < end && s.name != "__Vectors")
        .map(|s| s.addr)
        .min()
        .unwrap_or_else(|| {
            let e = entry & !1;
            if e >= base && e < end {
                e
            } else {
                base + 8.min(bytes.len() as u32)
            }
        });
    let is_code_at = |addr: u32| -> bool {
        if local_map.is_empty() {
            return addr >= code_start;
        }
        let i = local_map.partition_point(|(a, _)| *a <= addr);
        if i == 0 {
            false
        } else {
            local_map[i - 1].1
        }
    };
    let next_boundary = |addr: u32| -> u32 {
        local_map.iter().map(|(a, _)| *a).find(|&a| a > addr).unwrap_or(end).min(end)
    };

    let mut off = 0usize;
    let mut it: u8 = 0;
    while off < bytes.len() {
        let addr = base + off as u32;
        let label = symbols.iter().find(|s| s.addr == addr).map(|s| s.name.clone());
        let le = lookup_line(lines, addr);
        let mk = |opcode: String, text: String, label: Option<String>| DisasmLine {
            addr,
            opcode,
            mnemonic: text,
            label,
            file: le.map(|l| l.file.clone()),
            line: le.map(|l| l.line),
        };

        if !is_code_at(addr) {
            it = 0;
            let limit = next_boundary(addr);
            let left = limit.saturating_sub(addr) as usize;
            if addr & 3 == 0 && left >= 4 && off + 4 <= bytes.len() {
                let w = u32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]]);
                let comment = symbols
                    .iter()
                    .find(|s| s.addr == (w & !1) && w != 0 && s.addr >= 0x0800_0000)
                    .map(|s| format!(" ; {}", s.name))
                    .unwrap_or_default();
                out.push(mk(format!("{:08x}", w), format!("{:<9}0x{:08X}{}", "DCD", w, comment), label));
                off += 4;
            } else if addr & 1 == 0 && left >= 2 && off + 2 <= bytes.len() {
                let h = u16::from_le_bytes([bytes[off], bytes[off + 1]]);
                out.push(mk(format!("{:04x}", h), format!("{:<9}0x{:04X}", "DCW", h), label));
                off += 2;
            } else {
                out.push(mk(format!("{:02x}", bytes[off]), format!("{:<9}0x{:02X}", "DCB", bytes[off]), label));
                off += 1;
            }
            continue;
        }
        if off + 2 > bytes.len() {
            out.push(mk(format!("{:02x}", bytes[off]), format!("{:<9}0x{:02X}", "DCB", bytes[off]), label));
            break;
        }
        let hw1 = u16::from_le_bytes([bytes[off], bytes[off + 1]]);
        let wide = thumb::is_thumb32(hw1);
        if wide && off + 4 > bytes.len() {
            out.push(mk(format!("{:04x}", hw1), format!("{:<9}0x{:04X}", "DCW", hw1), label));
            break;
        }
        let hw2 = if wide { u16::from_le_bytes([bytes[off + 2], bytes[off + 3]]) } else { 0 };
        let in_it = it != 0;
        let last = it & 0xF == 0x8;
        let cond = if in_it { it >> 4 } else { 14 };
        let d = thumb::decode(hw1, hw2, in_it, last);
        let text = format_instr(&d.instr, addr, hw1, hw2, wide, cond, symbols);
        let text = if d.unpredictable { format!("{} ; UNPREDICTABLE", text) } else { text };
        let opcode = if wide { format!("{:04x}{:04x}", hw1, hw2) } else { format!("{:04x}", hw1) };
        out.push(mk(opcode, text, label));
        it = match d.instr {
            Instr::It { firstcond, mask } if !in_it => (firstcond << 4) | mask,
            _ if in_it => thumb::it_advance(it),
            _ => 0,
        };
        off += d.len as usize;
    }
    out
}

fn lookup_line(lines: &[LineEntry], addr: u32) -> Option<&LineEntry> {
    let idx = lines.partition_point(|e| e.addr <= addr);
    if idx == 0 {
        return None;
    }
    let c = &lines[idx - 1];
    (c.addr == addr).then_some(c)
}

fn r(n: u8) -> &'static str {
    ["r0", "r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8", "r9", "r10", "r11", "r12", "sp", "lr", "pc"][n as usize & 15]
}

fn hex(v: u32) -> String {
    format!("#0x{:X}", v)
}

fn target(t: u32, symbols: &[ElfSymbol]) -> String {
    match symbols.iter().find(|s| s.addr == (t & !1)) {
        Some(s) => format!("0x{:08X} ; {}", t & !1, s.name),
        None => format!("0x{:08X}", t & !1),
    }
}

fn shift_str(s: Shift) -> String {
    if s.is_none() {
        return String::new();
    }
    match s.t {
        SRType::Rrx => ",RRX".into(),
        t => format!(",{} #{}", t.name().to_ascii_uppercase(), s.n),
    }
}

fn reg_list(mask: u16) -> String {
    let mut items = Vec::new();
    let mut i = 0u8;
    while i < 16 {
        if mask & (1 << i) != 0 {
            let start = i;
            while i + 1 < 16 && mask & (1 << (i + 1)) != 0 && i < 12 && start <= 12 {
                i += 1;
            }
            if i > start + 1 {
                items.push(format!("{}-{}", r(start), r(i)));
            } else if i == start + 1 {
                items.push(r(start).to_string());
                items.push(r(i).to_string());
            } else {
                items.push(r(start).to_string());
            }
        }
        i += 1;
    }
    format!("{{{}}}", items.join(","))
}

fn mem(a: &Addr, pc: u32) -> String {
    match *a {
        Addr::Imm { rn, imm, add, index, wback } => {
            let sign = if add { "" } else { "-" };
            if !index {
                format!("[{}],#{}0x{:X}", r(rn), sign, imm)
            } else if wback {
                format!("[{},#{}0x{:X}]!", r(rn), sign, imm)
            } else if imm == 0 && add {
                format!("[{}]", r(rn))
            } else {
                format!("[{},#{}0x{:X}]", r(rn), sign, imm)
            }
        }
        Addr::Reg { rn, rm, lsl } => {
            if lsl == 0 {
                format!("[{},{}]", r(rn), r(rm))
            } else {
                format!("[{},{},LSL #{}]", r(rn), r(rm), lsl)
            }
        }
        Addr::Lit { imm, add } => {
            let base = (pc + 4) & !3;
            let t = if add { base.wrapping_add(imm) } else { base.wrapping_sub(imm) };
            format!("[pc,#{}0x{:X}] ; @0x{:08X}", if add { "" } else { "-" }, imm, t)
        }
    }
}

fn sysreg_name(sysm: u8, mask: u8, msr: bool) -> String {
    let base = match sysm {
        0 => "APSR",
        1 => "IAPSR",
        2 => "EAPSR",
        3 => "XPSR",
        5 => "IPSR",
        6 => "EPSR",
        7 => "IEPSR",
        8 => "MSP",
        9 => "PSP",
        16 => "PRIMASK",
        17 => "BASEPRI",
        18 => "BASEPRI_MAX",
        19 => "FAULTMASK",
        20 => "CONTROL",
        _ => return format!("SYSm_{}", sysm),
    };
    if msr && sysm <= 3 && mask == 2 {
        format!("{}_nzcvq", base)
    } else {
        base.to_string()
    }
}

/// Format one instruction. `cond` is the IT condition for instructions inside an IT block.
fn format_instr(ins: &Instr, pc: u32, hw1: u16, hw2: u16, wide: bool, cond: u8, symbols: &[ElfSymbol]) -> String {
    let c = if cond == 14 { "" } else { thumb::cond_name(cond) };
    let w = if wide { ".W" } else { "" };
    let s_ = |s: bool| if s { "S" } else { "" };
    let m = |name: String, ops: String| format!("{:<9}{}", name, ops);
    match ins {
        Instr::AluImm { op, s, rd, rn, imm, .. } => {
            let narrow_form = matches!(op, Alu::Add | Alu::Sub | Alu::Mov | Alu::Cmp);
            let wsfx = if wide && narrow_form { ".W" } else { "" };
            match op {
                Alu::Mov | Alu::Mvn => m(format!("{}{}{}{}", op.name().to_ascii_uppercase(), s_(*s), c, wsfx), format!("{},{}", r(*rd), hex(*imm))),
                o if o.is_compare() => m(format!("{}{}{}", o.name().to_ascii_uppercase(), c, wsfx), format!("{},{}", r(*rn), hex(*imm))),
                Alu::Rsb if !wide => m(format!("RSB{}{}", s_(*s), c), format!("{},{},#0", r(*rd), r(*rn))),
                o => {
                    let name = format!("{}{}{}{}", o.name().to_ascii_uppercase(), s_(*s), c, wsfx);
                    // 16-bit ADDS/SUBS Rdn,#imm8 is the two-operand form
                    if !wide && (hw1 & 0xF000) == 0x3000 {
                        m(name, format!("{},{}", r(*rd), hex(*imm)))
                    } else {
                        m(name, format!("{},{},{}", r(*rd), r(*rn), hex(*imm)))
                    }
                }
            }
        }
        Instr::AluReg { op, s, rd, rn, rm, shift } => {
            let narrow_form = !matches!(op, Alu::Orn | Alu::Teq | Alu::Rsb);
            let wsfx = if wide && narrow_form { ".W" } else { "" };
            match op {
                Alu::Mov if !shift.is_none() || (wide && shift.t == SRType::Rrx) => {
                    let (name, ops) = if shift.t == SRType::Rrx {
                        ("RRX", format!("{},{}", r(*rd), r(*rm)))
                    } else {
                        (
                            match shift.t {
                                SRType::Lsl => "LSL",
                                SRType::Lsr => "LSR",
                                SRType::Asr => "ASR",
                                _ => "ROR",
                            },
                            format!("{},{},#{}", r(*rd), r(*rm), shift.n),
                        )
                    };
                    let wsfx = if wide && !matches!(shift.t, SRType::Ror | SRType::Rrx) { ".W" } else { "" };
                    m(format!("{}{}{}{}", name, s_(*s), c, wsfx), ops)
                }
                Alu::Mov | Alu::Mvn => m(
                    format!("{}{}{}{}", op.name().to_ascii_uppercase(), s_(*s), c, wsfx),
                    format!("{},{}{}", r(*rd), r(*rm), shift_str(*shift)),
                ),
                o if o.is_compare() => m(
                    format!("{}{}{}", o.name().to_ascii_uppercase(), c, wsfx),
                    format!("{},{}{}", r(*rn), r(*rm), shift_str(*shift)),
                ),
                o => {
                    let name = format!("{}{}{}{}", o.name().to_ascii_uppercase(), s_(*s), c, wsfx);
                    if !wide && (hw1 & 0xFF00) == 0x4400 {
                        m(name, format!("{},{}", r(*rd), r(*rm)))
                    } else {
                        m(name, format!("{},{},{}{}", r(*rd), r(*rn), r(*rm), shift_str(*shift)))
                    }
                }
            }
        }
        Instr::ShiftReg { t, s, rd, rn, rm } => m(
            format!("{}{}{}{}", t.name().to_ascii_uppercase(), s_(*s), c, w),
            format!("{},{},{}", r(*rd), r(*rn), r(*rm)),
        ),
        // the 16-bit SP-relative forms decode to AddW too: ADD rd,sp,#i / ADD sp,sp,#i / SUB sp,sp,#i
        Instr::AddW { sub, rd, rn, imm } if !wide => m(format!("{}{}", if *sub { "SUB" } else { "ADD" }, c), format!("{},{},{}", r(*rd), r(*rn), hex(*imm))),
        Instr::AddW { sub, rd, rn, imm } => m(format!("{}W{}", if *sub { "SUB" } else { "ADD" }, c), format!("{},{},{}", r(*rd), r(*rn), hex(*imm))),
        Instr::Adr { rd, imm, add } => {
            let base = (pc + 4) & !3;
            let t = if *add { base.wrapping_add(*imm) } else { base.wrapping_sub(*imm) };
            m(format!("ADR{}{}", c, w), format!("{},0x{:08X}", r(*rd), t))
        }
        Instr::Movw { rd, imm } => m(format!("MOVW{}", c), format!("{},{}", r(*rd), hex(*imm as u32))),
        Instr::Movt { rd, imm } => m(format!("MOVT{}", c), format!("{},{}", r(*rd), hex(*imm as u32))),
        Instr::Mul { s, rd, rn, rm } => m(format!("MUL{}{}{}", s_(*s), c, w), format!("{},{},{}", r(*rd), r(*rn), r(*rm))),
        Instr::Mla { rd, rn, rm, ra } => m(format!("MLA{}", c), format!("{},{},{},{}", r(*rd), r(*rn), r(*rm), r(*ra))),
        Instr::Mls { rd, rn, rm, ra } => m(format!("MLS{}", c), format!("{},{},{},{}", r(*rd), r(*rn), r(*rm), r(*ra))),
        Instr::LongMul { kind, rdlo, rdhi, rn, rm } => {
            let n = match kind {
                LongMul::Smull => "SMULL",
                LongMul::Umull => "UMULL",
                LongMul::Smlal => "SMLAL",
                LongMul::Umlal => "UMLAL",
            };
            m(format!("{}{}", n, c), format!("{},{},{},{}", r(*rdlo), r(*rdhi), r(*rn), r(*rm)))
        }
        Instr::Div { signed, rd, rn, rm } => m(format!("{}DIV{}", if *signed { "S" } else { "U" }, c), format!("{},{},{}", r(*rd), r(*rn), r(*rm))),
        Instr::Extend { kind, rd, rm, rot } => {
            let n = match kind {
                ExtKind::Sxtb => "SXTB",
                ExtKind::Sxth => "SXTH",
                ExtKind::Uxtb => "UXTB",
                ExtKind::Uxth => "UXTH",
            };
            let ws = if wide && *rot == 0 { ".W" } else { "" };
            let rs = if *rot != 0 { format!(",ROR #{}", rot) } else { String::new() };
            m(format!("{}{}{}", n, c, ws), format!("{},{}{}", r(*rd), r(*rm), rs))
        }
        Instr::Rev { kind, rd, rm } => {
            let (n, nf) = match kind {
                RevKind::Rev => ("REV", true),
                RevKind::Rev16 => ("REV16", true),
                RevKind::Revsh => ("REVSH", true),
                RevKind::Rbit => ("RBIT", false),
            };
            m(format!("{}{}{}", n, c, if wide && nf { ".W" } else { "" }), format!("{},{}", r(*rd), r(*rm)))
        }
        Instr::Clz { rd, rm } => m(format!("CLZ{}", c), format!("{},{}", r(*rd), r(*rm))),
        Instr::Bfx { signed, rd, rn, lsb, width } => m(
            format!("{}BFX{}", if *signed { "S" } else { "U" }, c),
            format!("{},{},#{},#{}", r(*rd), r(*rn), lsb, width),
        ),
        Instr::Bfi { rd, rn, lsb, msb } => {
            let width = (*msb as i32 - *lsb as i32 + 1).max(0);
            if *rn == 15 {
                m(format!("BFC{}", c), format!("{},#{},#{}", r(*rd), lsb, width))
            } else {
                m(format!("BFI{}", c), format!("{},{},#{},#{}", r(*rd), r(*rn), lsb, width))
            }
        }
        Instr::Sat { unsigned, rd, sat, rn, shift } => m(
            format!("{}SAT{}", if *unsigned { "U" } else { "S" }, c),
            format!("{},#{},{}{}", r(*rd), sat, r(*rn), shift_str(*shift)),
        ),
        Instr::Str { sz: Size::W, rt, addr: Addr::Imm { rn: 13, imm: 4, add: false, index: true, wback: true }, unpriv: false } => {
            m(format!("PUSH{}.W", c), format!("{{{}}}", r(*rt)))
        }
        Instr::Ldr { sz: Size::W, signed: false, rt, addr: Addr::Imm { rn: 13, imm: 4, add: true, index: false, wback: true }, unpriv: false } => {
            m(format!("POP{}.W", c), format!("{{{}}}", r(*rt)))
        }
        Instr::Ldr { sz, signed, rt, addr, unpriv } => {
            let n = format!(
                "LDR{}{}{}",
                match (sz, signed) {
                    (Size::B, false) => "B",
                    (Size::B, true) => "SB",
                    (Size::H, false) => "H",
                    (Size::H, true) => "SH",
                    _ => "",
                },
                if *unpriv { "T" } else { "" },
                c
            );
            let ws = if wide && !*unpriv { ".W" } else { "" };
            m(format!("{}{}", n, ws), format!("{},{}", r(*rt), mem(addr, pc)))
        }
        Instr::Str { sz, rt, addr, unpriv } => {
            let n = format!(
                "STR{}{}{}",
                match sz {
                    Size::B => "B",
                    Size::H => "H",
                    _ => "",
                },
                if *unpriv { "T" } else { "" },
                c
            );
            let ws = if wide && !*unpriv { ".W" } else { "" };
            m(format!("{}{}", n, ws), format!("{},{}", r(*rt), mem(addr, pc)))
        }
        Instr::Ldrd { rt, rt2, addr } => m(format!("LDRD{}", c), format!("{},{},{}", r(*rt), r(*rt2), mem(addr, pc))),
        Instr::Strd { rt, rt2, addr } => m(format!("STRD{}", c), format!("{},{},{}", r(*rt), r(*rt2), mem(addr, pc))),
        Instr::Ldrex { sz, rt, rn, imm } => match sz {
            Size::W if *imm != 0 => m(format!("LDREX{}", c), format!("{},[{},{}]", r(*rt), r(*rn), hex(*imm))),
            Size::W => m(format!("LDREX{}", c), format!("{},[{}]", r(*rt), r(*rn))),
            Size::B => m(format!("LDREXB{}", c), format!("{},[{}]", r(*rt), r(*rn))),
            Size::H => m(format!("LDREXH{}", c), format!("{},[{}]", r(*rt), r(*rn))),
        },
        Instr::Strex { sz, rd, rt, rn, imm } => match sz {
            Size::W if *imm != 0 => m(format!("STREX{}", c), format!("{},{},[{},{}]", r(*rd), r(*rt), r(*rn), hex(*imm))),
            Size::W => m(format!("STREX{}", c), format!("{},{},[{}]", r(*rd), r(*rt), r(*rn))),
            Size::B => m(format!("STREXB{}", c), format!("{},{},[{}]", r(*rd), r(*rt), r(*rn))),
            Size::H => m(format!("STREXH{}", c), format!("{},{},[{}]", r(*rd), r(*rt), r(*rn))),
        },
        Instr::Clrex => m(format!("CLREX{}", c), String::new()),
        Instr::Ldm { rn, regs, wback, db } => {
            if *rn == 13 && *wback && !*db {
                m(format!("POP{}{}", c, w), reg_list(*regs))
            } else {
                let ws = if wide && !*db { ".W" } else { "" };
                let bang = if *wback { "!" } else { "" };
                m(format!("LDM{}{}{}", if *db { "DB" } else { "" }, c, ws), format!("{}{},{}", r(*rn), bang, reg_list(*regs)))
            }
        }
        Instr::Stm { rn, regs, wback, db } => {
            if *rn == 13 && *wback && *db {
                m(format!("PUSH{}{}", c, w), reg_list(*regs))
            } else {
                let ws = if wide && !*db { ".W" } else { "" };
                let bang = if *wback { "!" } else { "" };
                m(format!("STM{}{}{}", if *db { "DB" } else { "" }, c, ws), format!("{}{},{}", r(*rn), bang, reg_list(*regs)))
            }
        }
        Instr::Preload { data, addr } => {
            let a = match addr {
                Addr::Lit { .. } => mem(addr, pc),
                _ => mem(addr, pc),
            };
            m(format!("{}{}", if *data { "PLD" } else { "PLI" }, c), a)
        }
        Instr::Tb { half, rn, rm } => {
            if *half {
                m(format!("TBH{}", c), format!("[{},{},LSL #1]", r(*rn), r(*rm)))
            } else {
                m(format!("TBB{}", c), format!("[{},{}]", r(*rn), r(*rm)))
            }
        }
        Instr::B { cond: bc, off } => {
            let t = pc.wrapping_add(4).wrapping_add(*off as u32);
            let cc = if *bc != 14 { thumb::cond_name(*bc) } else { c };
            m(format!("B{}{}", cc, w), target(t, symbols))
        }
        Instr::Bl { off } => m(format!("BL{}.W", c), target(pc.wrapping_add(4).wrapping_add(*off as u32), symbols)),
        Instr::Bx { rm } => m(format!("BX{}", c), r(*rm).to_string()),
        Instr::Blx { rm } => m(format!("BLX{}", c), r(*rm).to_string()),
        Instr::Cbz { nz, rn, off } => m(
            if *nz { "CBNZ".into() } else { "CBZ".into() },
            format!("{},{}", r(*rn), target(pc.wrapping_add(4).wrapping_add(*off), symbols)),
        ),
        Instr::It { firstcond, mask } => {
            let mut pat = String::new();
            let low = mask.trailing_zeros();
            let mut b = 3i32;
            while b > low as i32 {
                pat.push(if (mask >> b) & 1 == firstcond & 1 { 'T' } else { 'E' });
                b -= 1;
            }
            m(format!("IT{}", pat), thumb::cond_name(*firstcond).to_string())
        }
        Instr::Hint(h) => {
            let n = match h {
                HintKind::Nop => "NOP",
                HintKind::Yield => "YIELD",
                HintKind::Wfe => "WFE",
                HintKind::Wfi => "WFI",
                HintKind::Sev => "SEV",
                HintKind::Dbg(o) => return m(format!("DBG{}", c), format!("#{}", o)),
                HintKind::Other(o) => return m("NOP".into(), format!("; hint #{}", o)),
            };
            m(format!("{}{}{}", n, c, w), String::new())
        }
        Instr::Svc(v) => m(format!("SVC{}", c), hex(*v as u32)),
        Instr::Bkpt(v) => m("BKPT".into(), hex(*v as u32)),
        Instr::Cps { disable, i, f } => {
            let fl = format!("{}{}", if *i { "i" } else { "" }, if *f { "f" } else { "" });
            m(if *disable { "CPSID".into() } else { "CPSIE".into() }, fl)
        }
        Instr::Msr { sysm, mask, rn } => m(format!("MSR{}", c), format!("{},{}", sysreg_name(*sysm, *mask, true), r(*rn))),
        Instr::Mrs { rd, sysm } => m(format!("MRS{}", c), format!("{},{}", r(*rd), sysreg_name(*sysm, 0, false))),
        Instr::Barrier { kind, opt } => {
            let n = match kind {
                Barrier::Dsb => "DSB",
                Barrier::Dmb => "DMB",
                Barrier::Isb => "ISB",
            };
            let o = match opt {
                15 => "SY".to_string(),
                14 => "ST".into(),
                11 => "ISH".into(),
                10 => "ISHST".into(),
                7 => "NSH".into(),
                6 => "NSHST".into(),
                3 => "OSH".into(),
                2 => "OSHST".into(),
                v => format!("#{}", v),
            };
            m(format!("{}{}", n, c), o)
        }
        Instr::Udf { imm } => m(format!("UDF{}", w), hex(*imm as u32)),
        Instr::Undefined | Instr::Coproc => {
            if wide {
                m("DCW".into(), format!("0x{:04X},0x{:04X} ; undefined", hw1, hw2))
            } else {
                m("DCW".into(), format!("0x{:04X} ; undefined", hw1))
            }
        }
    }
}
