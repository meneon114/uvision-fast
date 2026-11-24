#[cfg(not(target_arch = "wasm32"))]
use crate::toolchain::ToolchainInfo;
#[cfg(not(target_arch = "wasm32"))]
use anyhow::Context;
use anyhow::{anyhow, Result};
use gimli::{EndianSlice, LittleEndian};
use object::{Object, ObjectSection, ObjectSegment, ObjectSymbol, SymbolKind};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
#[cfg(not(target_arch = "wasm32"))]
use std::process::Command;

#[derive(Debug, Clone)]
pub struct ElfSegment {
    pub vaddr: u32,
    /// Load (physical) address: where the bytes live in the image. Equal to `vaddr`
    /// except for initialised RW data, which runs from SRAM but is stored in flash.
    pub paddr: u32,
    pub memsz: u32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElfSymbol {
    pub name: String,
    pub addr: u32,
    pub size: u32,
    pub is_func: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineEntry {
    pub addr: u32,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisasmLine {
    pub addr: u32,
    pub opcode: String,
    pub mnemonic: String,
    pub label: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct LoadedElf {
    pub path: String,
    pub entry: u32,
    pub segments: Vec<ElfSegment>,
    pub symbols: Vec<ElfSymbol>,
    pub sym_by_name: HashMap<String, u32>,
    pub lines: Vec<LineEntry>,
    pub disassembly: Vec<DisasmLine>,
    /// ELF mapping symbols (`$t` code / `$d` data), sorted by address.
    pub mapping: Vec<(u32, bool)>,
}

impl LoadedElf {
    /// Address of a symbol with the Thumb bit removed from code symbols only. A global
    /// definition wins over a file-local label of the same name.
    pub fn symbol_addr(&self, name: &str) -> Option<u32> {
        let raw = *self.sym_by_name.get(name)?;
        self.symbols
            .iter()
            .find(|s| s.name == name && (s.addr == raw || (s.is_func && s.addr == raw & !1)))
            .map(|s| s.addr)
    }
}

/// (p_vaddr, p_paddr) of each PT_LOAD program header of a little-endian ELF32 file.
fn program_header_paddrs(b: &[u8]) -> Vec<(u32, u32)> {
    let rd16 = |o: usize| b.get(o..o + 2).map(|x| u16::from_le_bytes([x[0], x[1]]) as usize);
    let rd32 = |o: usize| b.get(o..o + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]));
    if b.len() < 52 || &b[..4] != b"\x7fELF" || b[4] != 1 || b[5] != 1 {
        return Vec::new();
    }
    let (Some(phoff), Some(phentsize), Some(phnum)) = (rd32(28), rd16(42), rd16(44)) else {
        return Vec::new();
    };
    (0..phnum)
        .filter_map(|i| {
            let o = phoff as usize + i * phentsize;
            (rd32(o)? == 1).then_some(())?;
            Some((rd32(o + 8)?, rd32(o + 12)?))
        })
        .collect()
}

impl LoadedElf {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load(axf_path: &Path, tc: &ToolchainInfo) -> Result<Self> {
        let bytes = std::fs::read(axf_path)
            .with_context(|| format!("Failed reading ELF/AXF: {}", axf_path.display()))?;
        let mut parsed = Self::parse_raw(&axf_path.to_string_lossy(), &bytes)?;
        let mut disassembly = extract_disassembly(axf_path, tc, &parsed.lines, &parsed.symbols);
        if disassembly.is_empty() {
            disassembly = crate::disasm::disassemble_elf(
                &parsed.segments,
                &parsed.lines,
                &parsed.symbols,
                parsed.entry,
            &parsed.mapping,
            );
        }
        parsed.disassembly = disassembly;
        Ok(parsed)
    }

    pub fn load_from_bytes(name: &str, bytes: &[u8]) -> Result<Self> {
        let mut parsed = Self::parse_raw(name, bytes)?;
        parsed.disassembly = crate::disasm::disassemble_elf(
            &parsed.segments,
            &parsed.lines,
            &parsed.symbols,
            parsed.entry,
            &parsed.mapping,
        );
        Ok(parsed)
    }

    fn parse_raw(label: &str, bytes: &[u8]) -> Result<Self> {
        let obj = object::File::parse(bytes)
            .map_err(|e| anyhow!("Invalid ELF/AXF {}: {}", label, e))?;

        let entry = obj.entry() as u32;
        let paddrs = program_header_paddrs(bytes);
        let mut segments = Vec::new();
        for seg in obj.segments() {
            let vaddr = seg.address() as u32;
            let memsz = seg.size() as u32;
            let data = seg.data().map(|d| d.to_vec()).unwrap_or_default();
            if memsz > 0 {
                let paddr = paddrs.iter().find(|(v, _)| *v == vaddr).map(|(_, p)| *p).unwrap_or(vaddr);
                segments.push(ElfSegment { vaddr, paddr, memsz, data });
            }
        }

        let mut symbols = Vec::new();
        let mut sym_by_name = HashMap::new();
        let mut global_names = std::collections::HashSet::new();
        let mut mapping = Vec::new();
        for sym in obj.symbols() {
            let Ok(name) = sym.name() else { continue };
            if name == "$t" || name.starts_with("$t.") || name == "$d" || name.starts_with("$d.") {
                mapping.push((sym.address() as u32 & !1, name.starts_with("$t")));
                continue;
            }
            if name.is_empty() || name.starts_with('$') {
                continue;
            }
            let raw_addr = sym.address() as u32;
            // Bit 0 is the Thumb bit only on code symbols; a data label (STT_OBJECT) can sit
            // at an odd address, e.g. a DCB right after another DCB.
            let in_code = sym
                .section_index()
                .and_then(|i| obj.section_by_index(i).ok())
                .is_some_and(|s| s.kind() == object::SectionKind::Text);
            let is_func = match sym.kind() {
                SymbolKind::Text => true,
                SymbolKind::Data => false,
                _ => in_code && raw_addr & 1 == 1,
            };
            let addr = if is_func { raw_addr & !1 } else { raw_addr };
            let size = sym.size() as u32;
            if sym.is_global() {
                if global_names.insert(name.to_string()) {
                    sym_by_name.insert(name.to_string(), raw_addr);
                }
            } else if !global_names.contains(name) {
                sym_by_name.entry(name.to_string()).or_insert(raw_addr);
            }
            symbols.push(ElfSymbol {
                name: name.to_string(),
                addr,
                size,
                is_func,
            });
        }
        symbols.sort_by_key(|s| (s.addr, s.name.clone()));
        mapping.sort();
        mapping.dedup_by_key(|m| m.0);

        let lines = extract_dwarf_lines(&obj).unwrap_or_default();

        Ok(Self {
            path: label.to_string(),
            entry,
            segments,
            symbols,
            sym_by_name,
            lines,
            disassembly: Vec::new(),
            mapping,
        })
    }

    pub fn line_for_addr(&self, addr: u32) -> Option<&LineEntry> {
        let clean = addr & !1;
        if self.lines.is_empty() {
            return None;
        }
        let idx = self.lines.partition_point(|e| e.addr <= clean);
        if idx == 0 {
            return None;
        }
        let cand = &self.lines[idx - 1];
        if clean.saturating_sub(cand.addr) <= 64 {
            Some(cand)
        } else {
            None
        }
    }

    pub fn addr_for_line(&self, file_query: &str, line: u32) -> Option<u32> {
        let qnorm = normalize_path_cmp(file_query);
        let qname = Path::new(&qnorm)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&qnorm)
            .to_string();

        // Exact path match first, then filename match
        let mut exact: Option<u32> = None;
        let mut by_name: Option<u32> = None;
        for e in &self.lines {
            let enorm = normalize_path_cmp(&e.file);
            if e.line == line {
                if enorm == qnorm {
                    exact = Some(e.addr);
                    break;
                }
                let ename = Path::new(&enorm)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&enorm);
                if ename == qname && by_name.is_none() {
                    by_name = Some(e.addr);
                }
            }
        }
        if exact.is_some() {
            return exact;
        }
        if by_name.is_some() {
            return by_name;
        }

        // Nearest line >= requested within same file (within 10 lines)
        let mut best: Option<(u32, u32)> = None;
        for e in &self.lines {
            let enorm = normalize_path_cmp(&e.file);
            let ename = Path::new(&enorm)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(&enorm);
            if (enorm == qnorm || ename == qname) && e.line >= line && e.line - line <= 8
                && best.is_none_or(|(bl, _)| e.line < bl) {
                    best = Some((e.line, e.addr));
                }
        }
        best.map(|(_, a)| a)
    }

    pub fn symbol_for_addr(&self, addr: u32) -> Option<(&ElfSymbol, u32)> {
        let clean = addr & !1;
        let mut best: Option<&ElfSymbol> = None;
        for s in &self.symbols {
            if s.addr <= clean && s.addr >= 0x0800_0000
                && best.is_none_or(|b| s.addr >= b.addr) {
                    best = Some(s);
                }
        }
        best.map(|s| (s, clean - s.addr))
    }

    pub fn executable_lines_for_file(&self, file_query: &str) -> Vec<u32> {
        let qnorm = normalize_path_cmp(file_query);
        let qname = Path::new(&qnorm)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&qnorm)
            .to_string();
        let mut out: Vec<u32> = self
            .lines
            .iter()
            .filter(|e| {
                let enorm = normalize_path_cmp(&e.file);
                let ename = Path::new(&enorm)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&enorm);
                enorm == qnorm || ename == qname
            })
            .map(|e| e.line)
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }
}

fn normalize_path_cmp(s: &str) -> String {
    s.replace('\\', "/").to_ascii_lowercase()
}

fn extract_dwarf_lines<'a>(obj: &object::File<'a>) -> Result<Vec<LineEntry>> {
    let load_section = |id: gimli::SectionId| -> std::result::Result<Cow<'a, [u8]>, gimli::Error> {
        match obj.section_by_name(id.name()) {
            Some(sec) => Ok(sec.uncompressed_data().unwrap_or(Cow::Borrowed(&[]))),
            None => Ok(Cow::Borrowed(&[])),
        }
    };

    let dwarf_sections = gimli::DwarfSections::load(load_section)?;
    let dwarf = dwarf_sections.borrow(|sec| EndianSlice::new(sec, LittleEndian));

    let mut entries = Vec::new();
    let mut iter = dwarf.units();
    while let Some(header) = iter.next()? {
        let unit = dwarf.unit(header)?;
        let Some(program) = unit.line_program.clone() else { continue };
        let comp_dir = unit
            .comp_dir
            .as_ref()
            .map(|s| String::from_utf8_lossy(s.slice()).into_owned())
            .unwrap_or_default();

        let mut rows = program.rows();
        while let Some((header, row)) = rows.next_row()? {
            if row.end_sequence() {
                continue;
            }
            let Some(line_nz) = row.line() else { continue };
            let addr = (row.address() as u32) & !1;
            let file_path = if let Some(file) = row.file(header) {
                let mut p = PathBuf::new();
                if let Some(dir_attr) = file.directory(header) {
                    if let Ok(d_str) = dwarf.attr_string(&unit, dir_attr) {
                        let ds = String::from_utf8_lossy(d_str.slice()).into_owned();
                        let dp = PathBuf::from(&ds);
                        if dp.is_relative() && !comp_dir.is_empty() {
                            p.push(&comp_dir);
                        }
                        p.push(dp);
                    }
                } else if !comp_dir.is_empty() {
                    p.push(&comp_dir);
                }
                if let Ok(f_str) = dwarf.attr_string(&unit, file.path_name()) {
                    p.push(String::from_utf8_lossy(f_str.slice()).as_ref());
                }
                p.to_string_lossy().into_owned()
            } else {
                String::new()
            };

            entries.push(LineEntry {
                addr,
                file: file_path,
                line: line_nz.get() as u32,
            });
        }
    }

    entries.sort_by_key(|e| (e.addr, e.line));
    entries.dedup_by(|a, b| a.addr == b.addr && a.file == b.file && a.line == b.line);
    Ok(entries)
}

#[cfg(not(target_arch = "wasm32"))]
fn extract_disassembly(
    axf_path: &Path,
    tc: &ToolchainInfo,
    lines: &[LineEntry],
    symbols: &[ElfSymbol],
) -> Vec<DisasmLine> {
    let mut out = Vec::new();
    let Ok(fromelf) = tc.require_fromelf() else {
        return out;
    };
    let Ok(output) = Command::new(fromelf).args(["-c", "-d"]).arg(axf_path).output() else {
        return out;
    };
    let text = String::from_utf8_lossy(&output.stdout);

    let mut pending_label: Option<String> = None;
    let mut in_code_section = false;

    for raw in text.lines() {
        let t = raw.trim();
        if t.starts_with("** Section") {
            in_code_section = t.contains("SHF_EXECINSTR") || t.contains("ER_RO");
            pending_label = None;
            continue;
        }
        if !in_code_section || t.is_empty() {
            continue;
        }
        if t.starts_with("0x") {
            if let Some(colon) = t.find(':') {
                let addr_str = t[2..colon].trim();
                if let Ok(addr) = u32::from_str_radix(addr_str, 16) {
                    let rest = t[colon + 1..].trim();
                    let opcode_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
                    let opcode = rest[..opcode_end].to_string();
                    let after_op = rest[opcode_end..].trim_start();
                    let ascii_chars = if opcode.len() >= 8 { 4 } else { 2 };
                    let mnemonic = if after_op.len() > ascii_chars {
                        after_op[ascii_chars..].trim().to_string()
                    } else {
                        String::new()
                    };
                    let sym_label = pending_label.take().or_else(|| {
                        symbols
                            .iter()
                            .find(|s| s.addr == addr && !s.name.starts_with('$'))
                            .map(|s| s.name.clone())
                    });
                    let le = line_lookup(lines, addr);
                    out.push(DisasmLine {
                        addr,
                        opcode,
                        mnemonic,
                        label: sym_label,
                        file: le.map(|l| l.file.clone()),
                        line: le.map(|l| l.line),
                    });
                }
            }
            continue;
        }
        if !t.starts_with('$') && !t.starts_with("Size") && !t.starts_with("Address") && !t.contains(' ') {
            pending_label = Some(t.to_string());
        }
    }
    out
}

#[cfg(not(target_arch = "wasm32"))]
fn line_lookup(lines: &[LineEntry], addr: u32) -> Option<&LineEntry> {
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
