//! ELF32 executable writer (ARM EABI5) with a DWARF v2 line table.
//!
//! Layout mirrors what armlink produces for a simple scatter file:
//! * PT_LOAD #0: the ROM load region (vectors, code, RO data, RW initialisation image);
//! * PT_LOAD #1: the RW execution region in RAM. Its `p_paddr` is the load address inside
//!   the ROM image (LMA) and `p_memsz` includes the zero-initialised (ZI) part, so a loader
//!   that honours physical addresses programs only flash, exactly like a real download.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sec {
    Text,
    Data,
    Bss,
}

pub struct Sym {
    pub name: String,
    pub value: u32,
    pub size: u32,
    pub global: bool,
    pub func: bool,
    /// Mapping symbols ($t / $d) are STT_NOTYPE locals.
    pub notype: bool,
    pub sec: Sec,
}

pub struct LineRow {
    pub addr: u32,
    pub file: usize,
    pub line: u32,
}

pub struct Image<'a> {
    pub entry: u32,
    pub rom_base: u32,
    pub rom: &'a [u8],
    /// (VMA, LMA, initialised bytes)
    pub rw: Option<(u32, u32, &'a [u8])>,
    /// (VMA, size)
    pub zi: Option<(u32, u32)>,
    pub syms: Vec<Sym>,
    pub files: &'a [String],
    pub rows: &'a [LineRow],
}

pub fn build(img: &Image) -> Vec<u8> {
    let debug_line = build_dwarf_v2_line(img.files, img.rows);

    let has_data = img.rw.map(|(_, _, b)| !b.is_empty()).unwrap_or(false);
    let has_bss = img.zi.map(|(_, s)| s > 0).unwrap_or(false);
    let data_idx: u16 = 2;
    let bss_idx: u16 = if has_data { 3 } else { 2 };
    let sym_idx: u16 = 2 + has_data as u16 + has_bss as u16;
    let str_idx = sym_idx + 1;
    let dline_idx = str_idx + 1;
    let dinfo_idx = dline_idx + 1;
    let dabbrev_idx = dinfo_idx + 1;
    let shstr_idx = dabbrev_idx + 1;
    let shnum = shstr_idx + 1;

    // Symbol table: locals first (ELF requirement), sh_info = first global index.
    let mut strtab = vec![0u8];
    let mut symtab = vec![0u8; 16];
    let mut ordered: Vec<&Sym> = img.syms.iter().filter(|s| !s.global).collect();
    let n_local = ordered.len() as u32 + 1;
    ordered.extend(img.syms.iter().filter(|s| s.global));
    for s in ordered {
        let name_off = strtab.len() as u32;
        strtab.extend_from_slice(s.name.as_bytes());
        strtab.push(0);
        let bind = if s.global { 1u8 } else { 0 };
        let ty = if s.notype {
            0u8
        } else if s.func {
            2
        } else {
            1
        };
        let shndx: u16 = match s.sec {
            Sec::Text => 1,
            Sec::Data if has_data => data_idx,
            Sec::Bss if has_bss => bss_idx,
            Sec::Data | Sec::Bss => {
                if has_bss {
                    bss_idx
                } else if has_data {
                    data_idx
                } else {
                    0xFFF1
                }
            }
        };
        symtab.extend_from_slice(&name_off.to_le_bytes());
        symtab.extend_from_slice(&s.value.to_le_bytes());
        symtab.extend_from_slice(&s.size.to_le_bytes());
        symtab.push((bind << 4) | ty);
        symtab.push(0);
        symtab.extend_from_slice(&shndx.to_le_bytes());
    }

    let debug_abbrev: [u8; 8] = [1, 0x11, 0, 0x10, 0x06, 0, 0, 0];
    let mut debug_info = Vec::with_capacity(16);
    debug_info.extend_from_slice(&8u32.to_le_bytes()); // unit_length
    debug_info.extend_from_slice(&2u16.to_le_bytes()); // version
    debug_info.extend_from_slice(&0u32.to_le_bytes()); // abbrev offset
    debug_info.push(4); // address size
    debug_info.push(1); // DW_TAG_compile_unit
    debug_info.extend_from_slice(&0u32.to_le_bytes()); // DW_AT_stmt_list
    let ul = (debug_info.len() - 4) as u32;
    debug_info[0..4].copy_from_slice(&ul.to_le_bytes());

    let mut shstrtab = vec![0u8];
    let sh_text = push_cstr(&mut shstrtab, ".text");
    let sh_data = push_cstr(&mut shstrtab, ".data");
    let sh_bss = push_cstr(&mut shstrtab, ".bss");
    let sh_symtab = push_cstr(&mut shstrtab, ".symtab");
    let sh_strtab = push_cstr(&mut shstrtab, ".strtab");
    let sh_dline = push_cstr(&mut shstrtab, ".debug_line");
    let sh_dinfo = push_cstr(&mut shstrtab, ".debug_info");
    let sh_dabbrev = push_cstr(&mut shstrtab, ".debug_abbrev");
    let sh_shstrtab = push_cstr(&mut shstrtab, ".shstrtab");

    let has_rw_seg = has_data || has_bss;
    let phnum: u16 = 1 + has_rw_seg as u16;
    let phoff: u32 = 52;
    let text_off = align4(phoff + phnum as u32 * 32);
    let mut cur = text_off + img.rom.len() as u32;
    let rw_bytes: &[u8] = img.rw.map(|(_, _, b)| b).unwrap_or(&[]);
    let data_off = align4(cur);
    cur = data_off + rw_bytes.len() as u32;
    let sym_off = align4(cur);
    cur = sym_off + symtab.len() as u32;
    let str_off = cur;
    cur = str_off + strtab.len() as u32;
    let dline_off = cur;
    cur += debug_line.len() as u32;
    let dinfo_off = cur;
    cur += debug_info.len() as u32;
    let dabbrev_off = cur;
    cur += debug_abbrev.len() as u32;
    let shstr_off = cur;
    cur += shstrtab.len() as u32;
    let shoff = align4(cur);

    let mut elf = Vec::with_capacity((shoff + shnum as u32 * 40) as usize);
    elf.extend_from_slice(b"\x7FELF\x01\x01\x01\x00");
    elf.extend_from_slice(&[0u8; 8]);
    elf.extend_from_slice(&2u16.to_le_bytes()); // ET_EXEC
    elf.extend_from_slice(&40u16.to_le_bytes()); // EM_ARM
    elf.extend_from_slice(&1u32.to_le_bytes());
    elf.extend_from_slice(&img.entry.to_le_bytes());
    elf.extend_from_slice(&phoff.to_le_bytes());
    elf.extend_from_slice(&shoff.to_le_bytes());
    elf.extend_from_slice(&0x0500_0200u32.to_le_bytes()); // EABI5, HASENTRY
    elf.extend_from_slice(&52u16.to_le_bytes());
    elf.extend_from_slice(&32u16.to_le_bytes());
    elf.extend_from_slice(&phnum.to_le_bytes());
    elf.extend_from_slice(&40u16.to_le_bytes());
    elf.extend_from_slice(&shnum.to_le_bytes());
    elf.extend_from_slice(&shstr_idx.to_le_bytes());

    let rom_len = img.rom.len() as u32;
    push_phdr(&mut elf, text_off, img.rom_base, img.rom_base, rom_len, rom_len, 5);
    let (rw_vma, rw_lma) = img.rw.map(|(v, l, _)| (v, l)).unwrap_or((img.zi.map(|z| z.0).unwrap_or(0), 0));
    let zi_size = img.zi.map(|z| z.1).unwrap_or(0);
    if has_rw_seg {
        let lma = if has_data { rw_lma } else { rw_vma };
        push_phdr(&mut elf, data_off, rw_vma, lma, rw_bytes.len() as u32, rw_bytes.len() as u32 + zi_size, 6);
    }

    pad_to(&mut elf, text_off);
    elf.extend_from_slice(img.rom);
    pad_to(&mut elf, data_off);
    elf.extend_from_slice(rw_bytes);
    pad_to(&mut elf, sym_off);
    elf.extend_from_slice(&symtab);
    elf.extend_from_slice(&strtab);
    elf.extend_from_slice(&debug_line);
    elf.extend_from_slice(&debug_info);
    elf.extend_from_slice(&debug_abbrev);
    elf.extend_from_slice(&shstrtab);
    pad_to(&mut elf, shoff);

    elf.extend_from_slice(&[0u8; 40]);
    push_shdr(&mut elf, [sh_text, 1, 6, img.rom_base, text_off, rom_len, 0, 0, 4, 0]);
    if has_data {
        push_shdr(&mut elf, [sh_data, 1, 3, rw_vma, data_off, rw_bytes.len() as u32, 0, 0, 4, 0]);
    }
    if has_bss {
        let zi_vma = img.zi.map(|z| z.0).unwrap_or(0);
        push_shdr(&mut elf, [sh_bss, 8, 3, zi_vma, data_off + rw_bytes.len() as u32, zi_size, 0, 0, 8, 0]);
    }
    push_shdr(&mut elf, [sh_symtab, 2, 0, 0, sym_off, symtab.len() as u32, str_idx as u32, n_local, 4, 16]);
    push_shdr(&mut elf, [sh_strtab, 3, 0, 0, str_off, strtab.len() as u32, 0, 0, 1, 0]);
    push_shdr(&mut elf, [sh_dline, 1, 0, 0, dline_off, debug_line.len() as u32, 0, 0, 1, 0]);
    push_shdr(&mut elf, [sh_dinfo, 1, 0, 0, dinfo_off, debug_info.len() as u32, 0, 0, 1, 0]);
    push_shdr(&mut elf, [sh_dabbrev, 1, 0, 0, dabbrev_off, debug_abbrev.len() as u32, 0, 0, 1, 0]);
    push_shdr(&mut elf, [sh_shstrtab, 3, 0, 0, shstr_off, shstrtab.len() as u32, 0, 0, 1, 0]);
    elf
}

fn build_dwarf_v2_line(files: &[String], rows: &[LineRow]) -> Vec<u8> {
    let mut header = vec![
        2,           // minimum_instruction_length
        1,           // default_is_stmt
        (-5i8) as u8, // line_base
        14,          // line_range
        10,          // opcode_base
    ];
    header.extend_from_slice(&[0, 1, 1, 1, 1, 0, 0, 0, 1]);
    header.push(0); // no include directories
    for f in files {
        header.extend_from_slice(f.as_bytes());
        header.extend_from_slice(&[0, 0, 0, 0]);
    }
    header.push(0);

    let mut prog = Vec::new();
    if let Some(first) = rows.first() {
        prog.extend_from_slice(&[0, 5, 2]);
        prog.extend_from_slice(&first.addr.to_le_bytes());
        let mut cur_addr = first.addr;
        let mut cur_line: i64 = 1;
        let mut cur_file = 0usize;
        for r in rows {
            if r.addr < cur_addr {
                // new sequence for a lower address
                prog.extend_from_slice(&[0, 1, 1]);
                prog.extend_from_slice(&[0, 5, 2]);
                prog.extend_from_slice(&r.addr.to_le_bytes());
                cur_addr = r.addr;
                cur_line = 1;
                cur_file = 0;
            }
            if r.file != cur_file {
                prog.push(4);
                write_uleb128(&mut prog, (r.file + 1) as u64);
                cur_file = r.file;
            }
            let dpc = (r.addr - cur_addr) / 2;
            if dpc > 0 {
                prog.push(2);
                write_uleb128(&mut prog, dpc as u64);
                cur_addr = r.addr;
            }
            let dl = r.line as i64 - cur_line;
            if dl != 0 {
                prog.push(3);
                write_sleb128(&mut prog, dl);
                cur_line = r.line as i64;
            }
            prog.push(1);
        }
        prog.push(2);
        write_uleb128(&mut prog, 1);
        prog.extend_from_slice(&[0, 1, 1]);
    }

    let header_len = header.len() as u32;
    let unit_len = 2 + 4 + header_len + prog.len() as u32;
    let mut out = Vec::new();
    out.extend_from_slice(&unit_len.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&header_len.to_le_bytes());
    out.extend_from_slice(&header);
    out.extend_from_slice(&prog);
    out
}

/// Intel HEX with extended linear address records.
pub fn format_intel_hex(base_addr: u32, bytes: &[u8]) -> String {
    let mut lines = Vec::new();
    let mut cur_hi: Option<u16> = None;
    let mut off = 0usize;
    while off < bytes.len() {
        let addr = base_addr.wrapping_add(off as u32);
        let hi = (addr >> 16) as u16;
        if cur_hi != Some(hi) {
            cur_hi = Some(hi);
            lines.push(hex_record(&[0x02, 0x00, 0x00, 0x04, (hi >> 8) as u8, hi as u8]));
        }
        // never let a record cross a 64 KiB boundary
        let room = 0x1_0000 - (addr & 0xFFFF) as usize;
        let n = 16.min(bytes.len() - off).min(room);
        let lo = addr as u16;
        let mut rec = vec![n as u8, (lo >> 8) as u8, lo as u8, 0x00];
        rec.extend_from_slice(&bytes[off..off + n]);
        lines.push(hex_record(&rec));
        off += n;
    }
    lines.push(":00000001FF".to_string());
    lines.join("\r\n") + "\r\n"
}

fn hex_record(body: &[u8]) -> String {
    let sum: u8 = body.iter().fold(0u8, |a, &b| a.wrapping_add(b));
    let mut s = String::from(":");
    for &b in body {
        s.push_str(&format!("{:02X}", b));
    }
    s.push_str(&format!("{:02X}", sum.wrapping_neg()));
    s
}

fn write_uleb128(out: &mut Vec<u8>, mut val: u64) {
    loop {
        let mut b = (val & 0x7F) as u8;
        val >>= 7;
        if val != 0 {
            b |= 0x80;
        }
        out.push(b);
        if val == 0 {
            break;
        }
    }
}

fn write_sleb128(out: &mut Vec<u8>, mut val: i64) {
    loop {
        let b = (val & 0x7F) as u8;
        val >>= 7;
        let done = (val == 0 && b & 0x40 == 0) || (val == -1 && b & 0x40 != 0);
        out.push(if done { b } else { b | 0x80 });
        if done {
            break;
        }
    }
}

fn push_cstr(buf: &mut Vec<u8>, s: &str) -> u32 {
    let off = buf.len() as u32;
    buf.extend_from_slice(s.as_bytes());
    buf.push(0);
    off
}

fn align4(v: u32) -> u32 {
    (v + 3) & !3
}

fn pad_to(buf: &mut Vec<u8>, target: u32) {
    if (buf.len() as u32) < target {
        buf.resize(target as usize, 0);
    }
}

fn push_phdr(buf: &mut Vec<u8>, off: u32, vaddr: u32, paddr: u32, filesz: u32, memsz: u32, flags: u32) {
    for v in [1, off, vaddr, paddr, filesz, memsz, flags, 4] {
        buf.extend_from_slice(&v.to_le_bytes());
    }
}

fn push_shdr(buf: &mut Vec<u8>, f: [u32; 10]) {
    for v in f {
        buf.extend_from_slice(&v.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_records_split_at_64k() {
        let h = format_intel_hex(0x0800_FFF8, &[0xAA; 16]);
        let lines: Vec<&str> = h.trim_end().split("\r\n").collect();
        assert_eq!(lines[0], ":020000040800F2");
        assert!(lines[1].starts_with(":08FFF800"));
        assert_eq!(lines[2], ":020000040801F1");
        assert!(lines[3].starts_with(":08000000"));
        assert_eq!(*lines.last().unwrap(), ":00000001FF");
    }
}
