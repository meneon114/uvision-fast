use super::*;

pub(crate) fn build(src: &str) -> InMemoryBuild {
    let opts = TargetOptions { device: "STM32F103C8".into(), ..Default::default() };
    assemble_sources(&opts, "t.axf", None, None, &[("t.s".to_string(), src.to_string())])
}

pub(crate) const LAB3_EXP1: &str = r#"GPIOA_BASE          EQU 0x40010800
RCC_BASE            EQU 0x40021000
RCC_APB2ENR         EQU 0x18
GPIO_CRL            EQU 0x00
GPIO_ODR            EQU 0x0C

     AREA main, CODE, READONLY
     EXPORT _main
     ENTRY

_main PROC
    LDR r2, =RCC_BASE
    LDR r1, [r2, #RCC_APB2ENR]
    ORR r1, r1, #(1 << 2)
    STR r1, [r2, #RCC_APB2ENR]

     LDR r3, =GPIOA_BASE
     LDR r1, [r3, #GPIO_CRL]
     BIC r1, r1, #(15 << 20)
     ORR r1, r1, #(3 << 20)
     STR r1, [r3, #GPIO_CRL]

     LDR r1, [r3, #GPIO_ODR]
     ORR r1, r1, #(1 << 5)
     STR r1, [r3, #GPIO_ODR]

stop
    B stop
    ENDP
    END
"#;

#[test]
fn lab3_builds() {
    let b = build(LAB3_EXP1);
    assert!(b.result.success, "{}", b.result.log);
    println!("{}", b.result.log);
}

// ---------------------------------------------------------------------------------------
// Golden encodings (llvm-mc-18, committed) and disassemble -> reassemble round trip
// ---------------------------------------------------------------------------------------

const GOLDEN: &str = include_str!("golden/thumb.txt");

struct GoldenLine {
    lineno: usize,
    text: String,
    bytes: Vec<u8>,
}

fn golden_lines() -> Vec<GoldenLine> {
    GOLDEN
        .lines()
        .enumerate()
        .filter_map(|(i, l)| {
            let (text, enc) = l.split_once(';')?;
            if text.trim().is_empty() || !l.starts_with(' ') {
                return None;
            }
            let mut bytes = Vec::new();
            for hw in enc.split_whitespace() {
                bytes.extend_from_slice(&u16::from_str_radix(hw, 16).unwrap().to_le_bytes());
            }
            Some(GoldenLine { lineno: i + 1, text: text.trim().to_string(), bytes })
        })
        .collect()
}

/// Wrap instruction text in a minimal program; returns (elf, address of `golden_start`).
fn build_block(body: &str) -> (crate::elf::LoadedElf, u32) {
    let src = format!(
        "    AREA golden, CODE, READONLY\n    ENTRY\ngolden_start\n{}\n    END\n",
        body
    );
    let b = build(&src);
    assert!(b.result.success, "build failed:\n{}", b.result.log);
    let elf = crate::elf::LoadedElf::load_from_bytes("g.axf", b.elf_bytes.as_ref().unwrap()).unwrap();
    let start = elf.symbols.iter().find(|s| s.name == "golden_start").expect("golden_start").addr;
    (elf, start)
}

fn rom_bytes(elf: &crate::elf::LoadedElf, addr: u32, len: usize) -> Vec<u8> {
    let seg = elf.segments.iter().find(|s| addr >= s.paddr && addr < s.paddr + s.data.len() as u32).unwrap();
    let o = (addr - seg.paddr) as usize;
    seg.data[o..(o + len).min(seg.data.len())].to_vec()
}

fn hw_str(b: &[u8]) -> String {
    b.chunks(2).map(|c| format!("{:04x}", u16::from_le_bytes([c[0], *c.get(1).unwrap_or(&0)]))).collect::<Vec<_>>().join(" ")
}

#[test]
fn golden_encodings_match_llvm_mc() {
    let lines = golden_lines();
    assert!(lines.len() > 250);
    let body: String = GOLDEN.lines().filter(|l| !l.trim_start().starts_with(';')).collect::<Vec<_>>().join("\n");
    let (elf, start) = build_block(&body);
    assert_eq!(start % 4, 0, "golden block must start word-aligned like llvm-mc's .text");
    let total: usize = lines.iter().map(|l| l.bytes.len()).sum();
    let got = rom_bytes(&elf, start, total);
    let mut off = 0;
    let mut bad = Vec::new();
    for l in &lines {
        let g = &got[off..(off + l.bytes.len()).min(got.len())];
        if g != l.bytes.as_slice() {
            bad.push(format!("golden.txt:{}: {:<32} expected {} got {}", l.lineno, l.text, hw_str(&l.bytes), hw_str(g)));
            if g.len() != l.bytes.len() || bad.len() > 40 {
                break;
            }
        }
        off += l.bytes.len();
    }
    assert!(bad.is_empty(), "{} mismatches (first ones):\n{}", bad.len(), bad.join("\n"));
}

/// Every golden instruction one at a time: catches errors that a single block could mask.
#[test]
fn golden_narrow_sizes_individually() {
    let mut it_left = 0usize;
    for l in golden_lines() {
        let mnem = l.text.split_whitespace().next().unwrap_or("");
        if it_left > 0 {
            // instructions inside IT blocks are covered by the block test
            it_left -= 1;
            continue;
        }
        if mnem.starts_with("it") && mnem.len() <= 5 {
            it_left = mnem.len() - 1;
            continue;
        }
        if l.text.contains("back") || l.text.contains("fwd") || l.text.contains("adrtgt") {
            continue;
        }
        let src = format!(
            "    AREA golden, CODE, READONLY\n    ENTRY\ngolden_start\n    {}\n    END\n",
            l.text
        );
        let b = build(&src);
        assert!(b.result.success, "golden.txt:{} `{}` failed:\n{}", l.lineno, l.text, b.result.log);
        let elf = crate::elf::LoadedElf::load_from_bytes("g.axf", b.elf_bytes.as_ref().unwrap()).unwrap();
        let start = elf.symbols.iter().find(|s| s.name == "golden_start").unwrap().addr;
        let got = rom_bytes(&elf, start, l.bytes.len());
        assert_eq!(hw_str(&got), hw_str(&l.bytes), "golden.txt:{} `{}`", l.lineno, l.text);
    }
}

#[test]
fn golden_disassembly_reassembles_identically() {
    let lines = golden_lines();
    let total: u32 = lines.iter().map(|l| l.bytes.len() as u32).sum();
    let body: String = GOLDEN.lines().filter(|l| !l.trim_start().starts_with(';')).collect::<Vec<_>>().join("\n");
    let (elf, start) = build_block(&body);
    let dis: Vec<_> = elf.disassembly.iter().filter(|d| d.addr >= start && d.addr < start + total).collect();
    assert!(!dis.is_empty());
    let re_body: String = dis.iter().map(|d| format!("    {}", d.mnemonic)).collect::<Vec<_>>().join("\n");
    if std::env::var("GOLDEN_DUMP").is_ok() {
        for d in &dis {
            println!("{:08X} {:<10} {}", d.addr, d.opcode, d.mnemonic);
        }
    }
    let (elf2, start2) = build_block(&re_body);
    assert_eq!(start, start2);
    let a = rom_bytes(&elf, start, total as usize);
    let b = rom_bytes(&elf2, start2, total as usize);
    if a != b {
        let mut msg = String::new();
        for d in &dis {
            let o = (d.addr - start) as usize;
            let n = d.opcode.len() / 2;
            if a[o..o + n] != b[o..(o + n).min(b.len())] {
                msg.push_str(&format!("0x{:08X} {:<40} orig {} reasm {}\n", d.addr, d.mnemonic, hw_str(&a[o..o + n]), hw_str(&b[o..(o + n).min(b.len())])));
            }
        }
        panic!("round trip differs:\n{}", msg);
    }
}

#[test]
fn vector_table_and_data_keep_their_size() {
    let b = build(LAB3_EXP1);
    assert!(b.result.success, "{}", b.result.log);
    let elf = crate::elf::LoadedElf::load_from_bytes("t.axf", b.elf_bytes.as_ref().unwrap()).unwrap();
    let rom = &elf.segments[0];
    assert_eq!(rom.paddr, 0x0800_0000);
    let word = |i: usize| u32::from_le_bytes(rom.data[i * 4..i * 4 + 4].try_into().unwrap());
    assert_eq!(word(0), 0x2000_5000, "initial SP = top of the 20 KiB SRAM");
    let reset = elf.sym_by_name["Reset_Handler"];
    assert_eq!(word(1), reset, "reset vector -> Reset_Handler (Thumb bit set)");
    assert_eq!(reset & 1, 1);
    assert_eq!(elf.entry, reset);
    assert!(rom.data.len() >= 76 * 4 + 32);
}
