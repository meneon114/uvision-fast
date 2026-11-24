use crate::elf::{DisasmLine, ElfSegment, ElfSymbol, LineEntry};
use crate::sim::cpu::{is_thumb32, thumb_expand_imm_c};

pub fn disassemble_elf(
    segments: &[ElfSegment],
    lines: &[LineEntry],
    symbols: &[ElfSymbol],
    entry: u32,
) -> Vec<DisasmLine> {
    let mut out = Vec::new();
    let Some(flash_seg) = segments
        .iter()
        .find(|s| s.vaddr >= 0x0800_0000 && s.vaddr < 0x1000_0000 && !s.data.is_empty())
        .or_else(|| segments.iter().find(|s| !s.data.is_empty()))
    else {
        return out;
    };

    let base = flash_seg.vaddr;
    let bytes = &flash_seg.data;
    let code_start = symbols
        .iter()
        .filter(|s| s.is_func && s.addr >= base && s.name != "__Vectors")
        .map(|s| s.addr)
        .min()
        .unwrap_or_else(|| {
            let e = entry & !1;
            if e >= base && e < base + bytes.len() as u32 {
                e
            } else if bytes.len() >= 8 {
                base + 8
            } else {
                base
            }
        });

    let mut off = 0usize;
    while off < bytes.len() {
        let addr = base + off as u32;
        let label = symbols
            .iter()
            .find(|s| s.addr == addr && !s.name.starts_with('$'))
            .map(|s| s.name.clone());
        let le = lookup_line(lines, addr);

        if addr < code_start && off + 4 <= bytes.len() {
            let w = u32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]]);
            out.push(DisasmLine {
                addr,
                opcode: format!("{:08x}", w),
                mnemonic: format!("DCD      0x{:08X}", w),
                label,
                file: le.map(|l| l.file.clone()),
                line: le.map(|l| l.line),
            });
            off += 4;
            continue;
        }

        if off + 2 > bytes.len() {
            break;
        }
        let hw1 = u16::from_le_bytes([bytes[off], bytes[off + 1]]);
        if is_thumb32(hw1) && off + 4 <= bytes.len() {
            let hw2 = u16::from_le_bytes([bytes[off + 2], bytes[off + 3]]);
            let mnemonic = decode_thumb32(addr, hw1, hw2, symbols);
            out.push(DisasmLine {
                addr,
                opcode: format!("{:04x}{:04x}", hw1, hw2),
                mnemonic,
                label,
                file: le.map(|l| l.file.clone()),
                line: le.map(|l| l.line),
            });
            off += 4;
        } else {
            let mnemonic = decode_thumb16(addr, hw1, symbols);
            out.push(DisasmLine {
                addr,
                opcode: format!("{:04x}", hw1),
                mnemonic,
                label,
                file: le.map(|l| l.file.clone()),
                line: le.map(|l| l.line),
            });
            off += 2;
        }
    }

    out
}

fn lookup_line(lines: &[LineEntry], addr: u32) -> Option<&LineEntry> {
    let idx = lines.partition_point(|e| e.addr <= addr);
    if idx == 0 {
        None
    } else {
        let c = &lines[idx - 1];
        if addr.saturating_sub(c.addr) <= 16 {
            Some(c)
        } else {
            None
        }
    }
}

fn reg(r: usize) -> &'static str {
    match r {
        0 => "r0",
        1 => "r1",
        2 => "r2",
        3 => "r3",
        4 => "r4",
        5 => "r5",
        6 => "r6",
        7 => "r7",
        8 => "r8",
        9 => "r9",
        10 => "r10",
        11 => "r11",
        12 => "r12",
        13 => "sp",
        14 => "lr",
        15 => "pc",
        _ => "r?",
    }
}

fn cond_name(c: u8) -> &'static str {
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

fn format_target(target: u32, symbols: &[ElfSymbol]) -> String {
    if let Some(s) = symbols.iter().find(|s| s.addr == (target & !1)) {
        format!("0x{:08X} ({})", target & !1, s.name)
    } else {
        format!("0x{:08X}", target & !1)
    }
}

fn sign_ext(v: u32, bits: u32) -> i32 {
    let sh = 32 - bits;
    ((v << sh) as i32) >> sh
}

fn format_reg_list(mask: u16, extra: Option<&str>) -> String {
    let mut items = Vec::new();
    for i in 0..16 {
        if (mask & (1 << i)) != 0 {
            items.push(reg(i).to_string());
        }
    }
    if let Some(e) = extra {
        items.push(e.to_string());
    }
    format!("{{{}}}", items.join(","))
}

fn decode_thumb16(pc: u32, op: u16, symbols: &[ElfSymbol]) -> String {
    let top3 = (op >> 13) & 0x7;
    match top3 {
        0b000 => {
            let sub = (op >> 11) & 0x3;
            let rd = (op & 0x7) as usize;
            let rn = ((op >> 3) & 0x7) as usize;
            let imm5 = ((op >> 6) & 0x1F) as u32;
            match sub {
                0b00 if imm5 == 0 => format!("MOVS     {},{}", reg(rd), reg(rn)),
                0b00 => format!("LSLS     {},{},#{}", reg(rd), reg(rn), imm5),
                0b01 => format!("LSRS     {},{},#{}", reg(rd), reg(rn), if imm5 == 0 { 32 } else { imm5 }),
                0b10 => format!("ASRS     {},{},#{}", reg(rd), reg(rn), if imm5 == 0 { 32 } else { imm5 }),
                _ => {
                    let subop = (op >> 9) & 0x3;
                    let rm_imm = ((op >> 6) & 0x7) as usize;
                    match subop {
                        0 => format!("ADDS     {},{},{}", reg(rd), reg(rn), reg(rm_imm)),
                        1 => format!("SUBS     {},{},{}", reg(rd), reg(rn), reg(rm_imm)),
                        2 => format!("ADDS     {},{},#{}", reg(rd), reg(rn), rm_imm),
                        _ => format!("SUBS     {},{},#{}", reg(rd), reg(rn), rm_imm),
                    }
                }
            }
        }
        0b001 => {
            let sub = (op >> 11) & 0x3;
            let rdn = ((op >> 8) & 0x7) as usize;
            let imm8 = op & 0xFF;
            let mn = match sub {
                0 => "MOVS",
                1 => "CMP",
                2 => "ADDS",
                _ => "SUBS",
            };
            format!("{:<9}{},#0x{:X}", mn, reg(rdn), imm8)
        }
        0b010 => {
            if (op & 0xFC00) == 0x4000 {
                let alu = (op >> 6) & 0xF;
                let rm = ((op >> 3) & 0x7) as usize;
                let rdn = (op & 0x7) as usize;
                let mn = match alu {
                    0x0 => "ANDS",
                    0x1 => "EORS",
                    0x2 => "LSLS",
                    0x3 => "LSRS",
                    0x4 => "ASRS",
                    0x5 => "ADCS",
                    0x6 => "SBCS",
                    0x7 => "RORS",
                    0x8 => "TST",
                    0x9 => "RSBS",
                    0xA => "CMP",
                    0xB => "CMN",
                    0xC => "ORRS",
                    0xD => "MULS",
                    0xE => "BICS",
                    _ => "MVNS",
                };
                if alu == 0x9 {
                    format!("{:<9}{},{},#0", mn, reg(rdn), reg(rm))
                } else {
                    format!("{:<9}{},{}", mn, reg(rdn), reg(rm))
                }
            } else if (op & 0xFC00) == 0x4400 {
                let sub = (op >> 8) & 0x3;
                let rm = ((op >> 3) & 0xF) as usize;
                let rdn = (((op >> 7) & 1) << 3 | (op & 0x7)) as usize;
                match sub {
                    0 => format!("ADD      {},{}", reg(rdn), reg(rm)),
                    1 => format!("CMP      {},{}", reg(rdn), reg(rm)),
                    2 => format!("MOV      {},{}", reg(rdn), reg(rm)),
                    _ => {
                        if (op & 0x80) != 0 {
                            format!("BLX      {}", reg(rm))
                        } else {
                            format!("BX       {}", reg(rm))
                        }
                    }
                }
            } else if (op & 0xF800) == 0x4800 {
                let rt = ((op >> 8) & 0x7) as usize;
                let imm = ((op & 0xFF) as u32) << 2;
                let target = ((pc + 4) & !3) + imm;
                format!("LDR      {},[pc,#{}] ; [0x{:08X}]", reg(rt), imm, target)
            } else {
                let sub = (op >> 9) & 0x7;
                let rm = ((op >> 6) & 0x7) as usize;
                let rn = ((op >> 3) & 0x7) as usize;
                let rt = (op & 0x7) as usize;
                let mn = ["STR", "STRH", "STRB", "LDRSB", "LDR", "LDRH", "LDRB", "LDRSH"][sub as usize];
                format!("{:<9}{},[{},{}]", mn, reg(rt), reg(rn), reg(rm))
            }
        }
        0b011 => {
            let is_b = (op & (1 << 12)) != 0;
            let is_l = (op & (1 << 11)) != 0;
            let imm5 = ((op >> 6) & 0x1F) as u32;
            let rn = ((op >> 3) & 0x7) as usize;
            let rt = (op & 0x7) as usize;
            let mn = match (is_b, is_l) {
                (false, false) => "STR",
                (false, true) => "LDR",
                (true, false) => "STRB",
                (true, true) => "LDRB",
            };
            let off = if is_b { imm5 } else { imm5 << 2 };
            format!("{:<9}{},[{},#0x{:X}]", mn, reg(rt), reg(rn), off)
        }
        0b100 => {
            if (op & (1 << 12)) == 0 {
                let mn = if (op & (1 << 11)) != 0 { "LDRH" } else { "STRH" };
                let off = (((op >> 6) & 0x1F) as u32) << 1;
                let rn = ((op >> 3) & 0x7) as usize;
                let rt = (op & 0x7) as usize;
                format!("{:<9}{},[{},#0x{:X}]", mn, reg(rt), reg(rn), off)
            } else {
                let mn = if (op & (1 << 11)) != 0 { "LDR" } else { "STR" };
                let rt = ((op >> 8) & 0x7) as usize;
                let off = ((op & 0xFF) as u32) << 2;
                format!("{:<9}{},[sp,#0x{:X}]", mn, reg(rt), off)
            }
        }
        0b101 => {
            if (op & (1 << 12)) == 0 {
                let rd = ((op >> 8) & 0x7) as usize;
                let off = ((op & 0xFF) as u32) << 2;
                if (op & (1 << 11)) == 0 {
                    let target = ((pc + 4) & !3) + off;
                    format!("ADR      {},{}", reg(rd), format_target(target, symbols))
                } else {
                    format!("ADD      {},sp,#0x{:X}", reg(rd), off)
                }
            } else {
                decode_thumb16_misc(pc, op, symbols)
            }
        }
        0b110 => {
            if (op & (1 << 12)) == 0 {
                let mn = if (op & (1 << 11)) != 0 { "LDMIA" } else { "STMIA" };
                let rn = ((op >> 8) & 0x7) as usize;
                format!("{:<9}{}!,{}", mn, reg(rn), format_reg_list(op & 0xFF, None))
            } else {
                let cond = ((op >> 8) & 0xF) as u8;
                if cond == 0xF {
                    format!("SVC      #0x{:02X}", op & 0xFF)
                } else if cond == 0xE {
                    format!("UDF      #0x{:02X}", op & 0xFF)
                } else {
                    let off = (op & 0xFF) as i8 as i32;
                    let target = (pc.wrapping_add(4) as i32).wrapping_add(off << 1) as u32;
                    format!("B{:<8}{}", cond_name(cond), format_target(target, symbols))
                }
            }
        }
        _ => {
            let off = sign_ext((op & 0x7FF) as u32, 11) << 1;
            let target = (pc.wrapping_add(4) as i32).wrapping_add(off) as u32;
            format!("B        {}", format_target(target, symbols))
        }
    }
}

fn decode_thumb16_misc(pc: u32, op: u16, symbols: &[ElfSymbol]) -> String {
    let sub = (op >> 8) & 0xF;
    match sub {
        0x0 => {
            let imm = ((op & 0x7F) as u32) << 2;
            if (op & 0x80) == 0 {
                format!("ADD      sp,sp,#0x{:X}", imm)
            } else {
                format!("SUB      sp,sp,#0x{:X}", imm)
            }
        }
        0x1 | 0x3 | 0x9 | 0xB => {
            let nz = (op & (1 << 11)) != 0;
            let i = ((op >> 9) & 1) as u32;
            let imm5 = ((op >> 3) & 0x1F) as u32;
            let rn = (op & 0x7) as usize;
            let target = pc.wrapping_add(4).wrapping_add(((i << 5) | imm5) << 1);
            format!("{:<9}{},{}", if nz { "CBNZ" } else { "CBZ" }, reg(rn), format_target(target, symbols))
        }
        0x2 => {
            let kind = ((op >> 6) & 0x3) as usize;
            let mn = ["SXTH", "SXTB", "UXTH", "UXTB"][kind];
            format!("{:<9}{},{}", mn, reg((op & 7) as usize), reg(((op >> 3) & 7) as usize))
        }
        0x4 | 0x5 => format!("PUSH     {}", format_reg_list(op & 0xFF, if (op & 0x100) != 0 { Some("lr") } else { None })),
        0x6 => {
            let dis = (op & 0x10) != 0;
            format!("{}   i", if dis { "CPSID" } else { "CPSIE" })
        }
        0xA => {
            let kind = ((op >> 6) & 0x3) as usize;
            let mn = ["REV", "REV16", "REV", "REVSH"][kind];
            format!("{:<9}{},{}", mn, reg((op & 7) as usize), reg(((op >> 3) & 7) as usize))
        }
        0xC | 0xD => format!("POP      {}", format_reg_list(op & 0xFF, if (op & 0x100) != 0 { Some("pc") } else { None })),
        0xE => format!("BKPT     #0x{:02X}", op & 0xFF),
        0xF => {
            let mask = op & 0xF;
            if mask == 0 {
                match (op >> 4) & 0xF {
                    0 => "NOP".to_string(),
                    2 => "WFE".to_string(),
                    3 => "WFI".to_string(),
                    4 => "SEV".to_string(),
                    _ => "HINT".to_string(),
                }
            } else {
                format!("IT       {}", cond_name(((op >> 4) & 0xF) as u8))
            }
        }
        _ => format!("DCW      0x{:04X}", op),
    }
}

fn decode_thumb32(pc: u32, hw1: u16, hw2: u16, symbols: &[ElfSymbol]) -> String {
    let op1 = (hw1 >> 11) & 0x3;
    let op2 = (hw1 >> 4) & 0x7F;
    let op = (hw2 >> 15) & 1;

    match (op1, op2, op) {
        (0b10, _, 1) => {
            let s = ((hw1 >> 10) & 1) as u32;
            let j1 = ((hw2 >> 13) & 1) as u32;
            let j2 = ((hw2 >> 11) & 1) as u32;
            let b_op = (hw2 >> 12) & 0x7;
            if (b_op & 0x5) == 0 {
                let cond = ((hw1 >> 6) & 0xF) as u8;
                if (cond & 0xE) != 0xE {
                    let raw = (s << 20) | (j2 << 19) | (j1 << 18) | (((hw1 & 0x3F) as u32) << 12) | (((hw2 & 0x7FF) as u32) << 1);
                    let target = (pc.wrapping_add(4) as i32).wrapping_add(sign_ext(raw, 21)) as u32;
                    return format!("B{:<8}{}", format!("{}.W", cond_name(cond)), format_target(target, symbols));
                }
                return "SYS.W".to_string();
            }
            let i1 = !(j1 ^ s) & 1;
            let i2 = !(j2 ^ s) & 1;
            let raw = (s << 24) | (i1 << 23) | (i2 << 22) | (((hw1 & 0x3FF) as u32) << 12) | (((hw2 & 0x7FF) as u32) << 1);
            let target = (pc.wrapping_add(4) as i32).wrapping_add(sign_ext(raw, 25)) as u32;
            let mn = if (b_op & 4) != 0 { "BL" } else { "B.W" };
            format!("{:<9}{}", mn, format_target(target, symbols))
        }
        (0b10, x, 0) if (x & 0x20) == 0 => {
            let sub = ((hw1 >> 5) & 0xF) as u8;
            let s_bit = if (hw1 & 0x10) != 0 { "S" } else { "" };
            let rn = (hw1 & 0xF) as usize;
            let rd = ((hw2 >> 8) & 0xF) as usize;
            let imm12 = ((((hw1 >> 10) & 1) as u32) << 11) | ((((hw2 >> 12) & 7) as u32) << 8) | (hw2 & 0xFF) as u32;
            let (imm32, _) = thumb_expand_imm_c(imm12, false);
            match sub {
                0x0 if rd == 15 => format!("TST      {},#0x{:X}", reg(rn), imm32),
                0x0 => format!("AND{:<6}{},{},#0x{:X}", s_bit, reg(rd), reg(rn), imm32),
                0x1 => format!("BIC{:<6}{},{},#0x{:X}", s_bit, reg(rd), reg(rn), imm32),
                0x2 if rn == 15 => format!("MOV{:<6}{},#0x{:X}", s_bit, reg(rd), imm32),
                0x2 => format!("ORR{:<6}{},{},#0x{:X}", s_bit, reg(rd), reg(rn), imm32),
                0x3 if rn == 15 => format!("MVN{:<6}{},#0x{:X}", s_bit, reg(rd), imm32),
                0x3 => format!("ORN{:<6}{},{},#0x{:X}", s_bit, reg(rd), reg(rn), imm32),
                0x4 if rd == 15 => format!("TEQ      {},#0x{:X}", reg(rn), imm32),
                0x4 => format!("EOR{:<6}{},{},#0x{:X}", s_bit, reg(rd), reg(rn), imm32),
                0x8 if rd == 15 => format!("CMN      {},#0x{:X}", reg(rn), imm32),
                0x8 => format!("ADD{:<6}{},{},#0x{:X}", s_bit, reg(rd), reg(rn), imm32),
                0xA => format!("ADC{:<6}{},{},#0x{:X}", s_bit, reg(rd), reg(rn), imm32),
                0xB => format!("SBC{:<6}{},{},#0x{:X}", s_bit, reg(rd), reg(rn), imm32),
                0xD if rd == 15 => format!("CMP      {},#0x{:X}", reg(rn), imm32),
                0xD => format!("SUB{:<6}{},{},#0x{:X}", s_bit, reg(rd), reg(rn), imm32),
                0xE => format!("RSB{:<6}{},{},#0x{:X}", s_bit, reg(rd), reg(rn), imm32),
                _ => format!("DCI.W    0x{:04X}{:04X}", hw1, hw2),
            }
        }
        (0b10, x, 0) if (x & 0x20) != 0 => {
            let sub = (hw1 >> 4) & 0x1F;
            let rn = (hw1 & 0xF) as usize;
            let rd = ((hw2 >> 8) & 0xF) as usize;
            let imm12 = ((((hw1 >> 10) & 1) as u32) << 11) | ((((hw2 >> 12) & 7) as u32) << 8) | (hw2 & 0xFF) as u32;
            let imm16 = (((hw1 & 0xF) as u32) << 12) | imm12;
            match sub {
                0x00 => format!("ADDW     {},{},#0x{:X}", reg(rd), reg(rn), imm12),
                0x04 => format!("MOVW     {},#0x{:04X}", reg(rd), imm16),
                0x0A => format!("SUBW     {},{},#0x{:X}", reg(rd), reg(rn), imm12),
                0x0C => format!("MOVT     {},#0x{:04X}", reg(rd), imm16),
                _ => format!("DCI.W    0x{:04X}{:04X}", hw1, hw2),
            }
        }
        (0b01, x, _) if (x & 0x60) == 0x20 => {
            let sub = ((hw1 >> 5) & 0xF) as u8;
            let s_bit = if (hw1 & 0x10) != 0 { "S" } else { "" };
            let rn = (hw1 & 0xF) as usize;
            let rd = ((hw2 >> 8) & 0xF) as usize;
            let rm = (hw2 & 0xF) as usize;
            let stype = ((hw2 >> 4) & 0x3) as usize;
            let imm5 = ((((hw2 >> 12) & 7) as u32) << 2) | (((hw2 >> 6) & 3) as u32);
            let sname = ["LSL", "LSR", "ASR", "ROR"][stype];
            let sh_suffix = if imm5 == 0 {
                if stype == 3 { ",RRX".to_string() } else { String::new() }
            } else {
                format!(",{} #{}", sname, imm5)
            };
            match sub {
                0x2 if rn == 15 && imm5 > 0 => format!("{}{:<5}{},{},#{}", sname, s_bit, reg(rd), reg(rm), imm5),
                0x2 if rn == 15 => format!("MOV{:<6}{},{}{}", s_bit, reg(rd), reg(rm), sh_suffix),
                0x0 => format!("AND{:<6}{},{},{}{}", s_bit, reg(rd), reg(rn), reg(rm), sh_suffix),
                0x1 => format!("BIC{:<6}{},{},{}{}", s_bit, reg(rd), reg(rn), reg(rm), sh_suffix),
                0x2 => format!("ORR{:<6}{},{},{}{}", s_bit, reg(rd), reg(rn), reg(rm), sh_suffix),
                0x4 => format!("EOR{:<6}{},{},{}{}", s_bit, reg(rd), reg(rn), reg(rm), sh_suffix),
                0x8 => format!("ADD{:<6}{},{},{}{}", s_bit, reg(rd), reg(rn), reg(rm), sh_suffix),
                0xD => format!("SUB{:<6}{},{},{}{}", s_bit, reg(rd), reg(rn), reg(rm), sh_suffix),
                _ => format!("DP.W     {},{}", reg(rd), reg(rn)),
            }
        }
        (0b01, x, _) if (x & 0x64) == 0x00 => {
            let op = (hw1 >> 7) & 0x3;
            let is_load = (hw1 & 0x10) != 0;
            let rn = (hw1 & 0xF) as usize;
            if rn == 13 && op == 2 && !is_load {
                format!("PUSH.W   {}", format_reg_list(hw2, None))
            } else if rn == 13 && op == 1 && is_load {
                format!("POP.W    {}", format_reg_list(hw2, None))
            } else {
                format!("{}{:<5}{},{}", if is_load { "LDM" } else { "STM" }, if op == 2 { "DB.W" } else { "IA.W" }, reg(rn), format_reg_list(hw2, None))
            }
        }
        (0b11, x, _) if (x & 0x60) == 0x00 => {
            let size = (hw1 >> 5) & 0x3;
            let is_s = (hw1 & (1 << 8)) != 0;
            let is_l = (hw1 & (1 << 4)) != 0;
            let rn = (hw1 & 0xF) as usize;
            let rt = ((hw2 >> 12) & 0xF) as usize;
            let mn = match (is_l, size, is_s) {
                (true, 0, false) => "LDRB.W",
                (true, 0, true) => "LDRSB.W",
                (true, 1, false) => "LDRH.W",
                (true, 1, true) => "LDRSH.W",
                (true, _, _) => "LDR.W",
                (false, 0, _) => "STRB.W",
                (false, 1, _) => "STRH.W",
                (false, _, _) => "STR.W",
            };
            if rn == 15 {
                let imm12 = hw2 & 0xFFF;
                let base = (pc + 4) & !3;
                let addr = if (hw1 & 0x80) != 0 { base + imm12 as u32 } else { base - imm12 as u32 };
                format!("{:<9}{},[pc,#0x{:X}] ; [0x{:08X}]", mn, reg(rt), imm12, addr)
            } else if (hw1 & 0x80) != 0 {
                format!("{:<9}{},[{},#0x{:X}]", mn, reg(rt), reg(rn), hw2 & 0xFFF)
            } else if (hw2 & 0x800) != 0 {
                let sign = if (hw2 & 0x200) != 0 { "" } else { "-" };
                format!("{:<9}{},[{},#{}{}]", mn, reg(rt), reg(rn), sign, hw2 & 0xFF)
            } else {
                format!("{:<9}{},[{},{},LSL #{}]", mn, reg(rt), reg(rn), reg((hw2 & 0xF) as usize), (hw2 >> 4) & 3)
            }
        }
        (0b11, x, _) if (x & 0x70) == 0x20 => {
            let sub = (hw1 >> 4) & 0xF;
            let rn = (hw1 & 0xF) as usize;
            let rd = ((hw2 >> 8) & 0xF) as usize;
            let rm = (hw2 & 0xF) as usize;
            if ((hw2 >> 4) & 0x8) == 0 && sub <= 7 {
                let mn = ["LSL", "LSR", "ASR", "ROR"][((sub >> 1) & 3) as usize];
                let s = if (sub & 1) != 0 { "S.W" } else { ".W" };
                format!("{}{:<5}{},{},{}", mn, s, reg(rd), reg(rn), reg(rm))
            } else if sub <= 5 {
                let mn = match sub { 0 => "SXTH", 1 => "UXTH", 4 => "SXTB", _ => "UXTB" };
                format!("{:<9}{},{}", mn, reg(rd), reg(rm))
            } else if (hw1 & 0x0FF0) == 0x0AB0 {
                format!("CLZ      {},{}", reg(rd), reg(rm))
            } else if (hw1 & 0x0FF0) == 0x0A90 {
                let mn = ["REV.W", "REV16.W", "RBIT", "REVSH.W"][((hw2 >> 4) & 3) as usize];
                format!("{:<9}{},{}", mn, reg(rd), reg(rm))
            } else {
                format!("DCI.W    0x{:04X}{:04X}", hw1, hw2)
            }
        }
        (0b11, x, _) if (x & 0x70) == 0x30 => {
            let rn = (hw1 & 0xF) as usize;
            let ra = ((hw2 >> 12) & 0xF) as usize;
            let rd = ((hw2 >> 8) & 0xF) as usize;
            let rm = (hw2 & 0xF) as usize;
            if (hw1 & 0x0080) == 0 && ra == 15 {
                format!("MUL      {},{},{}", reg(rd), reg(rn), reg(rm))
            } else if (hw1 & 0x00F0) == 0x0090 {
                format!("SDIV     {},{},{}", reg(rd), reg(rn), reg(rm))
            } else if (hw1 & 0x00F0) == 0x00B0 {
                format!("UDIV     {},{},{}", reg(rd), reg(rn), reg(rm))
            } else {
                format!("MUL.W    {},{},{}", reg(rd), reg(rn), reg(rm))
            }
        }
        _ => format!("DCI.W    0x{:04X}{:04X}", hw1, hw2),
    }
}
