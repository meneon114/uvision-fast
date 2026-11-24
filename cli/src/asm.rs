//! Built-in assembler + linker for Keil armasm syntax (Cortex-M3, STM32F1).
//!
//! Pipeline: parse every source into AREAs of statements -> synthesise the startup code a
//! Keil project would link in (vector table, Reset_Handler, SystemInit, `__main`) unless
//! the sources provide it -> iterative layout where instruction / literal-pool sizes only
//! grow (so it always converges) -> encode -> ELF32 + DWARF line table, Intel HEX and BIN.
//!
//! Memory layout follows a default Keil scatter file: `RESET` first in flash, then the
//! code and RO data, then the RW initialisation image; RW then ZI data in SRAM. RW data
//! reaches RAM only through the scatter-load in `__main`, as on the chip.

mod elfout;
mod encode;
mod expr;
mod startup;

pub use elfout::format_intel_hex;

use crate::project::TargetOptions;
#[cfg(not(target_arch = "wasm32"))]
use crate::project::UvProject;
use crate::sim::memory::DeviceMap;
use crate::target::{BuildResult, Diagnostic};
#[cfg(not(target_arch = "wasm32"))]
use anyhow::Result;
use encode::{Ctx, Env, Insn, Op};
use expr::{ExprError, Resolver, Val};
use std::cell::Cell;
use std::collections::{HashMap, HashSet, VecDeque};
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

pub struct InMemoryBuild {
    pub result: BuildResult,
    pub elf_bytes: Option<Vec<u8>>,
    pub hex_text: Option<String>,
    pub bin_bytes: Option<Vec<u8>>,
}

/// Encode a 32-bit constant as a Thumb-2 modified immediate (i:imm3:imm8), if possible.
pub fn encode_thumb_expand_imm(val: u32) -> Option<u16> {
    encode::modimm(val).map(|v| v as u16)
}

// ---------------------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum DataItem {
    Expr(String),
    Bytes(Vec<u8>),
}

#[derive(Clone, Debug)]
enum Body {
    /// Zero-size anchor for labels at the end of an area.
    Label,
    Insn { insn: Box<Insn>, in_it: bool, last_in_it: bool, pool: Option<usize> },
    Data { unit: u32, aligned: bool, items: Vec<DataItem> },
    Space { size: String, fill: Option<String>, unit: u32 },
    Align { align: u32 },
    Pool(usize),
}

#[derive(Clone, Debug)]
struct Stmt {
    file: usize,
    line: u32,
    area: usize,
    body: Body,
    pad: u32,
    addr: u32,
    size: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AreaKind {
    Code,
    RoData,
    RwData,
    Zi,
}

#[derive(Clone, Debug)]
struct Area {
    name: String,
    file: usize,
    kind: AreaKind,
    align: u32,
    reset: bool,
    stmts: Vec<usize>,
    base: u32,
    size: u32,
}

#[derive(Clone, Debug, Default)]
struct Pool {
    slots: Vec<u32>,
    reserved: u32,
}

#[derive(Clone, Copy, Debug)]
enum LinkSym {
    RwLoad,
    RwBase,
    RwLimit,
    ZiLimit,
}

#[derive(Clone, Debug)]
enum SymDef {
    Label(usize),
    Equ(String),
    Linker(LinkSym),
    /// Resolve `name` as seen from file `file` (used by the startup code to reach the
    /// user's entry point even when it is not EXPORTed).
    Ref(usize, String),
}

#[derive(Debug, Default)]
struct SrcFile {
    path: String,
    virt: bool,
    syms: HashMap<String, SymDef>,
    sym_line: HashMap<String, u32>,
    exports: HashSet<String>,
    weak: HashSet<String>,
    imports: HashSet<String>,
    regs: HashMap<String, u8>,
    entry: Option<usize>,
}

struct Proc {
    file: usize,
    name: String,
    start: usize,
    end: usize,
}

const HIDDEN_ENTRY: &str = "__uvf_entry_directive";

struct Asm {
    files: Vec<SrcFile>,
    areas: Vec<Area>,
    stmts: Vec<Stmt>,
    pools: Vec<Pool>,
    procs: Vec<Proc>,
    diags: Vec<Diagnostic>,
    strong: HashMap<String, usize>,
    weak_glob: HashMap<String, usize>,
    link: [u32; 4],
    defines: Vec<(String, String)>,
}

// ---------------------------------------------------------------------------------------
// Expression environment
// ---------------------------------------------------------------------------------------

struct R<'a> {
    asm: &'a Asm,
    file: usize,
    pc: Option<u32>,
    depth: Cell<u32>,
}

impl Resolver for R<'_> {
    fn symbol(&self, name: &str) -> Option<Result<Val, ExprError>> {
        self.asm.lookup(self.file, name, self.depth.get())
    }
    fn here(&self) -> Option<i64> {
        self.pc.map(|p| p as i64)
    }
}

struct FileEnv<'a> {
    asm: &'a Asm,
    file: usize,
}

impl Env for FileEnv<'_> {
    fn eval(&self, e: &str, pc: u32) -> Result<Val, ExprError> {
        self.asm.eval(self.file, e, Some(pc))
    }
}

impl Asm {
    fn new() -> Self {
        Asm {
            files: Vec::new(),
            areas: Vec::new(),
            stmts: Vec::new(),
            pools: Vec::new(),
            procs: Vec::new(),
            diags: Vec::new(),
            strong: HashMap::new(),
            weak_glob: HashMap::new(),
            link: [0; 4],
            defines: Vec::new(),
        }
    }

    fn eval(&self, file: usize, e: &str, pc: Option<u32>) -> Result<Val, ExprError> {
        expr::eval(e, &R { asm: self, file, pc, depth: Cell::new(0) })
    }

    fn lookup(&self, file: usize, name: &str, depth: u32) -> Option<Result<Val, ExprError>> {
        if depth > 40 {
            return Some(Err(ExprError::Syntax(format!("Circular definition involving '{}'", name))));
        }
        let f = &self.files[file];
        let local = f.syms.get(name);
        if let Some(d) = local {
            let overridden = f.weak.contains(name) && self.strong.get(name).is_some_and(|&g| g != file);
            if !overridden {
                return Some(self.def_val(file, d, depth));
            }
        }
        for map in [&self.strong, &self.weak_glob] {
            if let Some(&g) = map.get(name) {
                if let Some(d) = self.files[g].syms.get(name) {
                    return Some(self.def_val(g, d, depth));
                }
            }
        }
        None
    }

    fn def_val(&self, file: usize, d: &SymDef, depth: u32) -> Result<Val, ExprError> {
        match d {
            SymDef::Label(s) => {
                let st = &self.stmts[*s];
                Ok(Val { v: st.addr as i64, reloc: 1, thumb: matches!(st.body, Body::Insn { .. }) })
            }
            SymDef::Equ(e) => expr::eval(e, &R { asm: self, file, pc: None, depth: Cell::new(depth + 1) }),
            SymDef::Linker(k) => Ok(Val { v: self.link[*k as usize] as i64, reloc: 1, thumb: false }),
            SymDef::Ref(f2, n) => self.lookup(*f2, n, depth + 1).unwrap_or_else(|| Err(ExprError::Undefined(n.clone()))),
        }
    }

    fn diag(&mut self, file: usize, line: u32, sev: &str, msg: impl Into<String>) {
        self.diags.push(Diagnostic {
            file: self.files[file].path.clone(),
            line,
            column: 1,
            severity: sev.to_string(),
            code: if sev == "error" { "ASM".into() } else { "ASMW".into() },
            message: msg.into(),
        });
    }

    fn errors(&self) -> usize {
        self.diags.iter().filter(|d| d.severity == "error").count()
    }
}

// ---------------------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------------------

#[derive(Default)]
struct PState {
    file: usize,
    area: Option<usize>,
    pending_labels: Vec<(String, u32)>,
    pending_entry: bool,
    it: VecDeque<u8>,
    it_line: u32,
    lits: HashMap<usize, Vec<usize>>,
    proc_open: Option<(String, usize, u32)>,
    /// IF stack: (this branch active, some branch already taken, parent active)
    conds: Vec<(bool, bool, bool)>,
    ended: bool,
}

const DIRECTIVES: &[&str] = &[
    "AREA", "EXPORT", "GLOBAL", "IMPORT", "EXTERN", "ENTRY", "END", "PROC", "FUNCTION", "ENDP", "ENDFUNC", "THUMB",
    "CODE16", "ARM", "CODE32", "PRESERVE8", "REQUIRE8", "ALIGN", "LTORG", "DCB", "DCW", "DCWU", "DCD", "DCDU", "DCQ",
    "DCQU", "SPACE", "FILL", "EQU", "RN", "IF", "ELSE", "ELIF", "ENDIF", "INCLUDE", "GET", "INCBIN", "MACRO", "MEND",
    "KEEP", "REQUIRE", "INFO", "ASSERT", "OPT", "TTL", "SUBT", "ATTR", "ROUT", "GBLA", "GBLL", "GBLS", "LCLA", "LCLL",
    "LCLS", "SETA", "SETL", "SETS", "%",
];

fn is_directive(w: &str) -> bool {
    let u = w.to_ascii_uppercase();
    DIRECTIVES.contains(&u.as_str())
}

fn is_keyword(w: &str) -> bool {
    is_directive(w) || encode::is_mnemonic(w) || w.starts_with('.')
}

fn split_first_word(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    match s.find(|c: char| c.is_whitespace()) {
        Some(i) => (&s[..i], s[i..].trim()),
        None => (s, ""),
    }
}

fn strip_comment(line: &str) -> &str {
    let b = line.as_bytes();
    let mut in_str = false;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if in_str {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
        } else {
            match c {
                b'"' => in_str = true,
                b'\'' if i + 2 < b.len() && b[i + 2] == b'\'' => {
                    i += 3;
                    continue;
                }
                b';' | b'@' => return &line[..i],
                b'/' if i + 1 < b.len() && b[i + 1] == b'/' => return &line[..i],
                _ => {}
            }
        }
        i += 1;
    }
    line
}

fn valid_ident(s: &str) -> bool {
    let mut ch = s.chars();
    match ch.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    ch.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

fn unbar(s: &str) -> String {
    let t = s.trim();
    t.strip_prefix('|').and_then(|x| x.strip_suffix('|')).unwrap_or(t).to_string()
}

fn parse_string(s: &str) -> Result<Vec<u8>, String> {
    let inner = s
        .trim()
        .strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .ok_or_else(|| format!("Malformed string {}", s))?;
    let mut out = Vec::new();
    let mut it = inner.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push(10),
                Some('r') => out.push(13),
                Some('t') => out.push(9),
                Some('0') => out.push(0),
                Some('\\') => out.push(b'\\'),
                Some('"') => out.push(b'"'),
                Some('\'') => out.push(b'\''),
                Some(o) => {
                    out.push(b'\\');
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(o.encode_utf8(&mut buf).as_bytes());
                }
                None => out.push(b'\\'),
            }
        } else if c == '"' && it.peek() == Some(&'"') {
            it.next();
            out.push(b'"');
        } else {
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
    }
    Ok(out)
}

impl Asm {
    fn add_file(&mut self, path: &str, virt: bool) -> usize {
        let mut f = SrcFile { path: path.to_string(), virt, ..Default::default() };
        if !virt {
            for (n, v) in &self.defines {
                f.syms.insert(n.clone(), SymDef::Equ(v.clone()));
            }
        }
        self.files.push(f);
        self.files.len() - 1
    }

    fn parse_file(&mut self, path: &str, text: &str, virt: bool) -> usize {
        let file = self.add_file(path, virt);
        let mut st = PState { file, ..Default::default() };
        let mut last_line = 0;
        for (i, raw) in text.lines().enumerate() {
            last_line = i as u32 + 1;
            if let Err(msg) = self.parse_line(&mut st, raw, last_line) {
                self.diag(file, last_line, "error", msg);
            }
            if st.ended {
                break;
            }
        }
        self.finish_file(&mut st, last_line.max(1));
        if !st.ended && !virt {
            self.diag(file, last_line.max(1), "warning", "Missing END directive at the end of the file");
        }
        file
    }

    fn finish_file(&mut self, st: &mut PState, line: u32) {
        if let Err(m) = self.leave_area(st, line) {
            self.diag(st.file, line, "error", m);
        }
        if let Some((name, _, l)) = st.proc_open.take() {
            self.diag(st.file, l, "error", format!("PROC '{}' has no matching ENDP", name));
        }
        if !st.conds.is_empty() {
            self.diag(st.file, line, "error", "IF without matching ENDIF");
        }
        let mut areas: Vec<usize> = st.lits.keys().copied().collect();
        areas.sort();
        for a in areas {
            let lits = st.lits.remove(&a).unwrap_or_default();
            if !lits.is_empty() {
                self.add_pool(st.file, line, a, &lits);
            }
        }
    }

    fn add_pool(&mut self, file: usize, line: u32, area: usize, lits: &[usize]) {
        let p = self.pools.len();
        self.pools.push(Pool::default());
        let idx = self.raw_stmt(file, line, area, Body::Pool(p), 0);
        let _ = idx;
        for &l in lits {
            if let Body::Insn { pool, .. } = &mut self.stmts[l].body {
                *pool = Some(p);
            }
        }
    }

    fn raw_stmt(&mut self, file: usize, line: u32, area: usize, body: Body, size: u32) -> usize {
        let idx = self.stmts.len();
        self.stmts.push(Stmt { file, line, area, body, pad: 0, addr: 0, size });
        self.areas[area].stmts.push(idx);
        idx
    }

    /// Close the current area: bind trailing labels, check the IT block is complete.
    fn leave_area(&mut self, st: &mut PState, line: u32) -> Result<(), String> {
        let Some(a) = st.area else {
            if let Some((l, ln)) = st.pending_labels.first() {
                let (l, ln) = (l.clone(), *ln);
                st.pending_labels.clear();
                self.diag(st.file, ln, "error", format!("Label '{}' is outside any AREA", l));
            }
            return Ok(());
        };
        if !st.pending_labels.is_empty() || st.pending_entry {
            let line_of = st.pending_labels.first().map(|x| x.1).unwrap_or(line);
            let idx = self.raw_stmt(st.file, line_of, a, Body::Label, 0);
            self.bind_pending(st, idx);
        }
        if !st.it.is_empty() {
            st.it.clear();
            return Err(format!("IT block starting at line {} is not complete", st.it_line));
        }
        Ok(())
    }

    fn bind_pending(&mut self, st: &mut PState, idx: usize) {
        for (name, line) in std::mem::take(&mut st.pending_labels) {
            self.define(st.file, &name, SymDef::Label(idx), line);
        }
        if st.pending_entry {
            st.pending_entry = false;
            if self.files[st.file].entry.is_none() {
                self.files[st.file].entry = Some(idx);
            }
        }
    }

    fn define(&mut self, file: usize, name: &str, d: SymDef, line: u32) {
        if let Some(prev) = self.files[file].sym_line.get(name) {
            let prev = *prev;
            self.diag(file, line, "error", format!("Symbol '{}' is already defined at line {}", name, prev));
            return;
        }
        if encode::parse_reg(name).is_some() {
            self.diag(file, line, "error", format!("'{}' is a register name and cannot be used as a symbol", name));
            return;
        }
        self.files[file].syms.insert(name.to_string(), d);
        self.files[file].sym_line.insert(name.to_string(), line);
    }

    fn push_stmt(&mut self, st: &mut PState, line: u32, body: Body, size: u32) -> Result<usize, String> {
        let a = st.area.ok_or("No AREA directive before this statement")?;
        let is_insn = matches!(body, Body::Insn { .. });
        if !st.it.is_empty() && !is_insn {
            return Err("Only instructions are allowed inside an IT block".into());
        }
        if is_insn && self.areas[a].kind != AreaKind::Code {
            return Err(format!(
                "Instruction in data AREA '{}'; instructions must be in an AREA declared with CODE",
                self.areas[a].name
            ));
        }
        if !is_insn && self.areas[a].kind == AreaKind::Zi {
            if let Body::Data { items, .. } = &body {
                let nonzero = items.iter().any(|i| match i {
                    DataItem::Bytes(b) => b.iter().any(|&x| x != 0),
                    DataItem::Expr(e) => e.trim() != "0",
                });
                if nonzero {
                    self.diag(st.file, line, "warning", "Initialised data in a NOINIT area is ignored (zero-initialised at startup)");
                }
            }
        }
        let idx = self.raw_stmt(st.file, line, a, body, size);
        self.bind_pending(st, idx);
        Ok(idx)
    }

    fn regs_of(&self, file: usize) -> impl Fn(&str) -> Option<u8> + '_ {
        move |s: &str| encode::parse_reg(s).or_else(|| self.files[file].regs.get(s.trim()).copied())
    }

    fn parse_line(&mut self, st: &mut PState, raw: &str, line: u32) -> Result<(), String> {
        let code = strip_comment(raw);
        let code = code.trim_end();
        if code.trim().is_empty() {
            return Ok(());
        }
        let col0 = !code.starts_with(|c: char| c.is_whitespace());

        // Conditional assembly is evaluated before anything else.
        let (fw, frest) = split_first_word(code);
        let fwu = fw.to_ascii_uppercase();
        match fwu.as_str() {
            "IF" | "[" => {
                let parent = st.conds.last().map(|c| c.0).unwrap_or(true);
                let v = if parent { self.parse_time_cond(st.file, frest)? } else { false };
                st.conds.push((parent && v, v, parent));
                return Ok(());
            }
            "ELIF" => {
                let Some(top) = st.conds.last_mut() else { return Err("ELIF without IF".into()) };
                let (_, taken, parent) = *top;
                if taken || !parent {
                    top.0 = false;
                } else {
                    let v = self.parse_time_cond(st.file, frest)?;
                    let top = st.conds.last_mut().unwrap();
                    top.0 = v;
                    top.1 = v;
                }
                return Ok(());
            }
            "ELSE" | "|" => {
                let Some(top) = st.conds.last_mut() else { return Err("ELSE without IF".into()) };
                top.0 = top.2 && !top.1;
                top.1 = true;
                return Ok(());
            }
            "ENDIF" | "]" => {
                if st.conds.pop().is_none() {
                    return Err("ENDIF without IF".into());
                }
                return Ok(());
            }
            _ => {}
        }
        if st.conds.last().is_some_and(|c| !c.0) {
            return Ok(());
        }

        // Label field
        let (mut label, mut rest): (Option<String>, &str) = (None, code.trim());
        let (w1, r1) = split_first_word(code);
        let (w2, _) = split_first_word(r1);
        if let Some(l) = w1.strip_suffix(':') {
            label = Some(l.to_string());
            rest = r1;
        } else if col0 {
            if w1.starts_with('.') && is_keyword(w1) {
                // GNU directive in column 0
            } else if is_directive(w1) && !w2.eq_ignore_ascii_case("EQU") && !w2.eq_ignore_ascii_case("RN") && !is_keyword(w2) {
                self.diag(st.file, line, "warning", format!("Directive '{}' in column 1 is treated as a label by armasm; indent it", w1));
            } else if encode::is_mnemonic(w1) && !r1.is_empty() && !is_keyword(w2) {
                return Err(format!(
                    "'{}' starts in column 1, so it is read as a label and '{}' as the instruction. Indent instructions with a space or tab",
                    w1, w2
                ));
            } else {
                label = Some(w1.to_string());
                rest = r1;
            }
        } else if !is_keyword(w1) && !w2.is_empty() && is_keyword(w2) {
            self.diag(st.file, line, "warning", format!("Label '{}' should start in column 1 (armasm treats an indented word as an instruction)", w1));
            label = Some(w1.to_string());
            rest = r1;
        }
        if let Some(l) = &label {
            let l2 = unbar(l);
            if !valid_ident(&l2) {
                if l2.chars().all(|c| c.is_ascii_digit()) {
                    return Err(format!("Numeric local label '{}' is not supported; use a named label", l2));
                }
                return Err(format!("Invalid label '{}'", l));
            }
            label = Some(l2);
        }

        let (op, args) = split_first_word(rest);
        let opu = op.to_ascii_uppercase();

        if op.is_empty() {
            if let Some(l) = label {
                st.pending_labels.push((l, line));
            }
            return Ok(());
        }

        // Directives that consume the label
        match opu.as_str() {
            "EQU" | "*" | ".EQU" | ".SET" if label.is_some() || opu.starts_with('.') => {
                let (name, val) = if let Some(l) = label {
                    (l, args.to_string())
                } else {
                    let parts = encode::split_operands(args);
                    if parts.len() != 2 {
                        return Err(format!("{} needs name, value", op));
                    }
                    (parts[0].clone(), parts[1].clone())
                };
                if val.trim().is_empty() {
                    return Err(format!("EQU '{}' has no value", name));
                }
                let v = val.split(',').next().unwrap_or("").trim().to_string();
                self.define(st.file, &name, SymDef::Equ(v), line);
                return Ok(());
            }
            "EQU" | "*" => return Err(format!("{} needs a name in column 1", opu)),
            _ => {}
        }
        if opu == "RN" {
            let name = label.ok_or("RN needs a name in column 1")?;
            let r = encode::parse_reg(args).ok_or_else(|| format!("RN: '{}' is not a register", args))?;
            self.files[st.file].regs.insert(name, r);
            return Ok(());
        }
        if matches!(opu.as_str(), "PROC" | "FUNCTION") {
            let name = label.ok_or("PROC needs a label in column 1 (e.g. 'main PROC')")?;
            if let Some((open, _, l)) = &st.proc_open {
                return Err(format!("PROC '{}' inside PROC '{}' (line {}); add ENDP first", name, open, l));
            }
            st.proc_open = Some((name.clone(), self.stmts.len(), line));
            st.pending_labels.push((name, line));
            return Ok(());
        }
        if let Some(l) = label.take() {
            st.pending_labels.push((l, line));
        }
        self.directive_or_insn(st, line, op, &opu, args)
    }

    fn parse_time_cond(&self, file: usize, e: &str) -> Result<bool, String> {
        // Replace :DEF:sym with 1/0 (defined so far), then evaluate.
        let mut s = e.to_string();
        loop {
            let up = s.to_ascii_uppercase();
            let Some(i) = up.find(":DEF:") else { break };
            let after = &s[i + 5..];
            let after_t = after.trim_start();
            let skip = after.len() - after_t.len();
            let end = after_t.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$')).unwrap_or(after_t.len());
            let name = &after_t[..end];
            let defined = self.files[file].syms.contains_key(name)
                || self.files[file].regs.contains_key(name)
                || self.strong.contains_key(name);
            s = format!("{}{}{}", &s[..i], if defined { "1" } else { "0" }, &after_t[end..]);
            let _ = skip;
        }
        let v = self.eval(file, &s, None).map_err(|e| format!("IF condition: {}", e))?;
        Ok(v.v != 0)
    }

    fn area_directive(&mut self, st: &mut PState, line: u32, args: &str) -> Result<(), String> {
        let parts = encode::split_operands(args);
        if parts.is_empty() || parts[0].is_empty() {
            return Err("AREA needs a name".into());
        }
        let name = unbar(&parts[0]);
        let mut code = false;
        let mut ro: Option<bool> = None;
        let mut noinit = false;
        let mut align = 4u32;
        for p in &parts[1..] {
            let up = p.trim().to_ascii_uppercase();
            match up.as_str() {
                "CODE" => code = true,
                "DATA" => code = false,
                "READONLY" => ro = Some(true),
                "READWRITE" => ro = Some(false),
                "NOINIT" => noinit = true,
                "NOALLOC" | "COMDEF" | "COMMON" | "MERGE" | "STRINGS" => {}
                _ => {
                    if let Some(n) = up.strip_prefix("ALIGN") {
                        let n = n.trim().trim_start_matches('=').trim();
                        let v: u32 = n.parse().map_err(|_| format!("Bad AREA ALIGN value '{}'", p.trim()))?;
                        if v > 31 {
                            return Err("AREA ALIGN must be 0..31".into());
                        }
                        align = 1u32 << v;
                    } else if !up.starts_with("ASSOC") && !up.starts_with("GROUP") && !up.starts_with("LINKORDER") {
                        return Err(format!("Unknown AREA attribute '{}'", p.trim()));
                    }
                }
            }
        }
        self.switch_area(st, line, &name, code, ro, noinit, align)
    }

    #[allow(clippy::too_many_arguments)]
    fn switch_area(&mut self, st: &mut PState, line: u32, name: &str, code: bool, ro: Option<bool>, noinit: bool, align: u32) -> Result<(), String> {
        self.leave_area(st, line)?;
        let kind = if code {
            if ro == Some(false) {
                self.diag(st.file, line, "warning", "READWRITE code areas are placed in flash like READONLY ones");
            }
            AreaKind::Code
        } else if noinit {
            AreaKind::Zi
        } else if ro == Some(true) {
            AreaKind::RoData
        } else {
            AreaKind::RwData
        };
        let existing = self.areas.iter().position(|a| a.file == st.file && a.name == name);
        let idx = match existing {
            Some(i) => i,
            None => {
                self.areas.push(Area {
                    name: name.to_string(),
                    file: st.file,
                    kind,
                    align,
                    reset: name == "RESET" || name == ".isr_vector",
                    stmts: Vec::new(),
                    base: 0,
                    size: 0,
                });
                self.areas.len() - 1
            }
        };
        st.area = Some(idx);
        Ok(())
    }

    fn directive_or_insn(&mut self, st: &mut PState, line: u32, op: &str, opu: &str, args: &str) -> Result<(), String> {
        let file = st.file;
        let names = |args: &str| -> Vec<(String, bool)> {
            encode::split_operands(args)
                .into_iter()
                .filter(|s| !s.is_empty())
                .map(|s| {
                    let up = s.to_ascii_uppercase();
                    let weak = up.contains("[WEAK") || up.ends_with(" WEAK");
                    let n = s.split(|c: char| c.is_whitespace() || c == '[').next().unwrap_or("").to_string();
                    (unbar(&n), weak)
                })
                .collect()
        };
        match opu {
            "AREA" => return self.area_directive(st, line, args),
            "EXPORT" | "GLOBAL" | ".GLOBAL" | ".GLOBL" | ".WEAK" => {
                let list = names(args);
                if list.is_empty() {
                    return Err(format!("{} needs a symbol name", op));
                }
                for (n, weak) in list {
                    self.files[file].exports.insert(n.clone());
                    if weak || opu == ".WEAK" {
                        self.files[file].weak.insert(n);
                    }
                }
                return Ok(());
            }
            "IMPORT" | "EXTERN" | ".EXTERN" => {
                for (n, _) in names(args) {
                    self.files[file].imports.insert(n);
                }
                return Ok(());
            }
            "ENTRY" => {
                st.pending_entry = true;
                return Ok(());
            }
            "END" | ".END" => {
                st.ended = true;
                return Ok(());
            }
            "ENDP" | "ENDFUNC" => {
                let Some((name, start, _)) = st.proc_open.take() else {
                    return Err("ENDP without a matching PROC".into());
                };
                self.procs.push(Proc { file, name, start, end: self.stmts.len() });
                return Ok(());
            }
            "THUMB" | "CODE16" | "PRESERVE8" | "REQUIRE8" | "KEEP" | "REQUIRE" | "INFO" | "ASSERT" | "OPT" | "TTL"
            | "SUBT" | "ATTR" | "ROUT" | ".SYNTAX" | ".THUMB" | ".CPU" | ".FPU" | ".ARCH" | ".EABI_ATTRIBUTE"
            | ".FILE" | ".TYPE" | ".SIZE" | ".THUMB_FUNC" | ".CODE" | ".FNSTART" | ".FNEND" | ".CANTUNWIND"
            | ".IDENT" | ".PRESERVE8" => return Ok(()),
            "ARM" | "CODE32" | ".ARM" => return Err("Cortex-M3 only executes Thumb code; remove ARM / CODE32".into()),
            "INCLUDE" | "GET" | "INCBIN" | ".INCLUDE" => {
                return Err(format!("{} is not supported by the built-in assembler; add the file to the project instead", op))
            }
            "MACRO" | "MEND" | "MEXIT" | ".MACRO" => return Err("Macros are not supported by the built-in assembler".into()),
            "GBLA" | "GBLL" | "GBLS" | "LCLA" | "LCLL" | "LCLS" | "SETA" | "SETL" | "SETS" => {
                return Err(format!("{} (assembly-time variables) is not supported; use EQU", op))
            }
            ".TEXT" => return self.switch_area(st, line, ".text", true, Some(true), false, 4),
            ".DATA" => return self.switch_area(st, line, ".data", false, Some(false), false, 4),
            ".BSS" => return self.switch_area(st, line, ".bss", false, Some(false), true, 4),
            ".SECTION" => {
                let n = unbar(encode::split_operands(args).first().map(|s| s.as_str()).unwrap_or(""));
                let flags = args.to_ascii_lowercase();
                let code = n.starts_with(".text") || flags.contains("\"ax\"") || flags.contains("\"xa\"");
                let ro = n.starts_with(".rodata") || n == ".isr_vector" || (flags.contains("\"a\"") && !code);
                let zi = n.starts_with(".bss") || n.starts_with(".noinit");
                return self.switch_area(st, line, &n, code, Some(ro || code), zi, 4);
            }
            "ALIGN" | ".ALIGN" | ".BALIGN" | ".P2ALIGN" => {
                let parts = encode::split_operands(args);
                let a = if parts.is_empty() || parts[0].is_empty() {
                    4
                } else {
                    let v = self.eval(file, &parts[0], None).map_err(|e| e.to_string())?.v;
                    match opu {
                        ".ALIGN" | ".P2ALIGN" => {
                            if !(0..=16).contains(&v) {
                                return Err("Alignment out of range".into());
                            }
                            1i64 << v
                        }
                        _ => v,
                    }
                };
                if a <= 0 || a & (a - 1) != 0 || a > 65536 {
                    return Err(format!("ALIGN value {} must be a power of two", a));
                }
                if parts.len() > 1 {
                    return Err("ALIGN with an offset or padding value is not supported".into());
                }
                self.push_stmt(st, line, Body::Align { align: a as u32 }, 0)?;
                return Ok(());
            }
            "LTORG" | ".LTORG" | ".POOL" => {
                let a = st.area.ok_or("LTORG outside an AREA")?;
                if !st.it.is_empty() {
                    return Err("LTORG inside an IT block".into());
                }
                // bind labels before the pool to the pool start
                let lits = st.lits.remove(&a).unwrap_or_default();
                let p = self.pools.len();
                self.pools.push(Pool::default());
                self.push_stmt(st, line, Body::Pool(p), 0)?;
                for l in lits {
                    if let Body::Insn { pool, .. } = &mut self.stmts[l].body {
                        *pool = Some(p);
                    }
                }
                return Ok(());
            }
            "DCB" | "DCW" | "DCWU" | "DCD" | "DCDU" | "DCQ" | "DCQU" | ".BYTE" | ".HWORD" | ".SHORT" | ".2BYTE" | ".WORD"
            | ".LONG" | ".4BYTE" | ".INT" | ".QUAD" | ".8BYTE" | ".ASCII" | ".ASCIZ" | ".STRING" => {
                let (unit, aligned) = match opu {
                    "DCB" | ".BYTE" | ".ASCII" | ".ASCIZ" | ".STRING" => (1, false),
                    "DCW" => (2, true),
                    "DCWU" | ".HWORD" | ".SHORT" | ".2BYTE" => (2, false),
                    "DCD" => (4, true),
                    "DCDU" | ".WORD" | ".LONG" | ".4BYTE" | ".INT" => (4, false),
                    "DCQ" => (8, true),
                    _ => (8, false),
                };
                let parts = encode::split_operands(args);
                if parts.is_empty() || parts.iter().any(|p| p.is_empty()) {
                    return Err(format!("{} needs at least one value", op));
                }
                let mut items = Vec::new();
                let mut size = 0u32;
                for p in parts {
                    if p.starts_with('"') {
                        if unit != 1 {
                            return Err("Strings are only allowed in DCB".into());
                        }
                        let mut b = parse_string(&p)?;
                        if matches!(opu, ".ASCIZ" | ".STRING") {
                            b.push(0);
                        }
                        size += b.len() as u32;
                        items.push(DataItem::Bytes(b));
                    } else {
                        size += unit;
                        items.push(DataItem::Expr(p));
                    }
                }
                self.push_stmt(st, line, Body::Data { unit, aligned, items }, size)?;
                return Ok(());
            }
            "SPACE" | "%" | ".SPACE" | ".SKIP" | ".ZERO" => {
                if args.trim().is_empty() {
                    return Err("SPACE needs a size".into());
                }
                let parts = encode::split_operands(args);
                let fill = parts.get(1).cloned();
                self.push_stmt(st, line, Body::Space { size: parts[0].clone(), fill, unit: 1 }, 0)?;
                return Ok(());
            }
            "FILL" | ".FILL" => {
                let parts = encode::split_operands(args);
                if parts.is_empty() || parts[0].is_empty() {
                    return Err("FILL needs a size".into());
                }
                let unit = match parts.get(2) {
                    Some(u) => {
                        let v = self.eval(file, u, None).map_err(|e| e.to_string())?.v;
                        if ![1, 2, 4].contains(&v) {
                            return Err("FILL value size must be 1, 2 or 4".into());
                        }
                        v as u32
                    }
                    None => 1,
                };
                let size = if opu == ".FILL" {
                    // GNU: .fill repeat, size, value
                    return Err(".fill is not supported; use .space".into());
                } else {
                    parts[0].clone()
                };
                self.push_stmt(st, line, Body::Space { size, fill: parts.get(1).cloned(), unit }, 0)?;
                return Ok(());
            }
            _ => {}
        }
        if op.starts_with('.') {
            return Err(format!("Unknown directive '{}'", op));
        }
        let Some((base, s, cond, width, pattern)) = encode::split_mnemonic(op) else {
            return Err(format!("Unknown opcode '{}' (labels must start in column 1; instructions must be indented)", op));
        };
        let regs = self.regs_of(file);
        let mut ops = Vec::new();
        if !args.trim().is_empty() {
            for o in encode::split_operands(args) {
                ops.push(encode::parse_operand(&o, &regs)?);
            }
        }
        drop(regs);
        let insn = Insn { base, s, cond, width, ops, it_pattern: pattern, text: op.to_string() };

        // IT block bookkeeping
        let (mut in_it, mut last_in_it) = (false, false);
        if base == "IT" {
            if !st.it.is_empty() {
                return Err("IT instruction inside an IT block".into());
            }
            if cond != 14 {
                return Err("IT cannot be conditional".into());
            }
            let c = match insn.ops.first() {
                Some(Op::Expr(x)) => encode::parse_cond(&x.to_ascii_uppercase()),
                _ => None,
            }
            .ok_or_else(|| format!("{} needs a condition (EQ, NE, CS, CC, MI, PL, VS, VC, HI, LS, GE, LT, GT, LE)", op))?;
            encode::it_mask(c, &insn.it_pattern)?;
            st.it.push_back(c);
            for ch in insn.it_pattern.chars() {
                st.it.push_back(if ch == 'T' { c } else { c ^ 1 });
            }
            st.it_line = line;
        } else if let Some(c) = st.it.pop_front() {
            in_it = true;
            last_in_it = st.it.is_empty();
            let c_eff = if c == 14 { 14 } else { c };
            if insn.cond != c_eff {
                st.it.clear();
                if insn.cond == 14 {
                    return Err(format!(
                        "Instruction in an IT block must carry the block's condition: write {}{}",
                        op.to_ascii_uppercase(),
                        crate::thumb::cond_name(c).to_ascii_uppercase()
                    ));
                }
                return Err(format!(
                    "Condition {} does not match the IT block (expected {})",
                    crate::thumb::cond_name(insn.cond).to_ascii_uppercase(),
                    crate::thumb::cond_name(c).to_ascii_uppercase()
                ));
            }
        } else if cond != 14 && base != "B" {
            return Err(format!(
                "Conditional instruction '{}' outside an IT block. Thumb-2 needs 'IT {}' on the line before it",
                op,
                crate::thumb::cond_name(cond).to_ascii_uppercase()
            ));
        }
        let is_lit = base == "LDR" && matches!(insn.ops.get(1), Some(Op::Lit(_)));
        let idx = self.push_stmt(
            st,
            line,
            Body::Insn { insn: Box::new(insn), in_it, last_in_it, pool: None },
            2,
        )?;
        if is_lit {
            let a = st.area.unwrap_or(0);
            st.lits.entry(a).or_default().push(idx);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------
// Linking: startup synthesis, layout, emission
// ---------------------------------------------------------------------------------------

struct Layout {
    rom_order: Vec<usize>,
    rw_order: Vec<usize>,
    zi_order: Vec<usize>,
    rom_end: u32,
    rw_load: u32,
    rw_base: u32,
    rw_limit: u32,
    zi_base: u32,
    zi_limit: u32,
}

fn align_up(v: u32, a: u32) -> u32 {
    if a <= 1 {
        v
    } else {
        v.wrapping_add(a - 1) & !(a - 1)
    }
}

impl Asm {
    fn compute_globals(&mut self) {
        self.strong.clear();
        self.weak_glob.clear();
        let mut dup = Vec::new();
        for (fi, f) in self.files.iter().enumerate() {
            for name in &f.exports {
                if !f.syms.contains_key(name) {
                    continue;
                }
                if f.weak.contains(name) {
                    self.weak_glob.entry(name.clone()).or_insert(fi);
                } else if let Some(&prev) = self.strong.get(name) {
                    dup.push((fi, name.clone(), prev));
                } else {
                    self.strong.insert(name.clone(), fi);
                }
            }
        }
        for (fi, name, prev) in dup {
            let line = self.files[fi].sym_line.get(&name).copied().unwrap_or(1);
            let other = self.files[prev].path.clone();
            self.diag(fi, line, "error", format!("Symbol '{}' is EXPORTed by more than one file (also in {})", name, other));
        }
    }

    fn check_exports(&mut self) {
        let mut errs = Vec::new();
        for (fi, f) in self.files.iter().enumerate() {
            for name in &f.exports {
                if !f.syms.contains_key(name) && !f.weak.contains(name) {
                    errs.push((fi, name.clone()));
                }
            }
        }
        for (fi, n) in errs {
            self.diag(fi, 1, "error", format!("EXPORTed symbol '{}' is not defined in this file", n));
        }
    }

    /// Find the user entry point the way Keil's runtime reaches it.
    fn find_entry(&self) -> Option<(usize, String)> {
        for name in ["__main", "main", "_main"] {
            for (fi, f) in self.files.iter().enumerate() {
                if !f.virt && matches!(f.syms.get(name), Some(SymDef::Label(_))) {
                    return Some((fi, name.to_string()));
                }
            }
        }
        for (fi, f) in self.files.iter().enumerate() {
            if f.entry.is_some() && !f.virt {
                return Some((fi, HIDDEN_ENTRY.to_string()));
            }
        }
        None
    }

    fn referenced_undefined(&self, name: &str) -> bool {
        if self.strong.contains_key(name) || self.files.iter().any(|f| !f.virt && f.syms.contains_key(name) && !f.exports.contains(name)) {
            return false;
        }
        self.files.iter().any(|f| f.imports.contains(name))
    }

    fn synthesize_startup(&mut self, options: &TargetOptions, map: &DeviceMap) -> Option<startup::Plan> {
        // ENTRY directive -> hidden label
        for fi in 0..self.files.len() {
            if let Some(s) = self.files[fi].entry {
                self.files[fi].syms.insert(HIDDEN_ENTRY.into(), SymDef::Label(s));
            }
        }
        self.compute_globals();
        let has_reset = self.areas.iter().any(|a| a.reset);
        let entry = self.find_entry();
        let plan = if !has_reset {
            let Some((_, ename)) = &entry else {
                if self.stmts.is_empty() {
                    return None;
                }
                let f = self.files.iter().position(|f| !f.virt).unwrap_or(0);
                self.diag(f, 1, "error", "No entry point: define __main, main or _main, or put ENTRY before the first instruction");
                return None;
            };
            let to_user_main = ename == "__main";
            startup::Plan {
                vectors: true,
                initial_sp: options.rw_base.max(0x2000_0000) + map.sram_size,
                system_init: true,
                main: !to_user_main,
                reset_to_user_main: to_user_main,
            }
        } else {
            let si = self.referenced_undefined("SystemInit");
            let mn = self.referenced_undefined("__main");
            if !si && !mn {
                return None;
            }
            if mn && entry.is_none() {
                let f = self.files.iter().position(|f| f.imports.contains("__main")).unwrap_or(0);
                self.diag(f, 1, "error", "__main is IMPORTed but there is no main / ENTRY to call");
                return None;
            }
            startup::Plan { vectors: false, initial_sp: 0, system_init: si, main: mn, reset_to_user_main: false }
        };
        let text = startup::generate(&plan);
        let f = self.parse_file("<startup>", &text, true);
        if let Some((ef, en)) = entry {
            self.files[f].syms.insert("__uvf_user_entry".into(), SymDef::Ref(ef, en));
        }
        for (n, k) in [
            ("__uvf_rw_load", LinkSym::RwLoad),
            ("__uvf_rw_base", LinkSym::RwBase),
            ("__uvf_rw_limit", LinkSym::RwLimit),
            ("__uvf_zi_limit", LinkSym::ZiLimit),
        ] {
            self.files[f].syms.insert(n.into(), SymDef::Linker(k));
        }
        self.compute_globals();
        Some(plan)
    }

    fn handler_warnings(&mut self) {
        let handlers: HashSet<String> = startup::handler_names().into_iter().collect();
        let mut warn = Vec::new();
        for (fi, f) in self.files.iter().enumerate() {
            if f.virt {
                continue;
            }
            for (name, d) in &f.syms {
                if matches!(d, SymDef::Label(_)) && handlers.contains(name) && !f.exports.contains(name) && self.weak_glob.contains_key(name) {
                    warn.push((fi, name.clone(), f.sym_line.get(name).copied().unwrap_or(1)));
                }
            }
        }
        for (fi, n, l) in warn {
            self.diag(
                fi,
                l,
                "warning",
                format!("'{}' is not EXPORTed, so the vector table uses the default handler (an endless loop). Add 'EXPORT {}'", n, n),
            );
        }
    }

    fn orders(&self) -> (Vec<usize>, Vec<usize>, Vec<usize>) {
        let virt = |a: &Area| self.files[a.file].virt;
        let mut rom: Vec<usize> = (0..self.areas.len()).filter(|&i| self.areas[i].reset).collect();
        rom.extend((0..self.areas.len()).filter(|&i| {
            let a = &self.areas[i];
            !a.reset && virt(a) && matches!(a.kind, AreaKind::Code | AreaKind::RoData)
        }));
        rom.extend((0..self.areas.len()).filter(|&i| {
            let a = &self.areas[i];
            !a.reset && !virt(a) && matches!(a.kind, AreaKind::Code | AreaKind::RoData)
        }));
        let rw = (0..self.areas.len()).filter(|&i| !self.areas[i].reset && self.areas[i].kind == AreaKind::RwData).collect();
        let zi = (0..self.areas.len()).filter(|&i| !self.areas[i].reset && self.areas[i].kind == AreaKind::Zi).collect();
        (rom, rw, zi)
    }

    fn place_area(&mut self, a: usize, start: u32) -> u32 {
        let mut pc = align_up(start, self.areas[a].align);
        self.areas[a].base = pc;
        let ids = self.areas[a].stmts.clone();
        for s in ids {
            let st = &self.stmts[s];
            let al = match &st.body {
                Body::Insn { .. } => 2,
                Body::Data { unit, aligned: true, .. } => (*unit).min(4),
                Body::Align { align } => *align,
                Body::Pool(_) => 4,
                _ => 1,
            };
            let size = match &st.body {
                Body::Pool(p) => self.pools[*p].reserved * 4,
                _ => st.size,
            };
            let addr = align_up(pc, al);
            let st = &mut self.stmts[s];
            st.pad = addr - pc;
            st.addr = addr;
            st.size = size;
            pc = addr + size;
        }
        self.areas[a].size = pc - self.areas[a].base;
        pc
    }

    fn place(&mut self, ro_base: u32, rw_base: u32, lay: &mut Layout) {
        let mut pc = ro_base;
        for &a in &lay.rom_order.clone() {
            pc = self.place_area(a, pc);
        }
        let rw_load = align_up(pc, 4);
        let mut ram = rw_base;
        for &a in &lay.rw_order.clone() {
            ram = self.place_area(a, ram);
        }
        let rw_limit = align_up(ram, 4);
        let zi_base = rw_limit;
        let mut z = zi_base;
        for &a in &lay.zi_order.clone() {
            z = self.place_area(a, z);
        }
        let zi_limit = align_up(z, 4);
        lay.rw_load = rw_load;
        lay.rw_base = rw_base;
        lay.rw_limit = rw_limit;
        lay.zi_base = zi_base;
        lay.zi_limit = zi_limit;
        lay.rom_end = rw_load + (rw_limit - rw_base);
        self.link = [rw_load, rw_base, rw_limit, zi_limit];
    }

    fn lit_expr(&self, s: usize) -> Option<&str> {
        match &self.stmts[s].body {
            Body::Insn { insn, .. } if insn.base == "LDR" => match insn.ops.get(1) {
                Some(Op::Lit(e)) => Some(e.as_str()),
                _ => None,
            },
            _ => None,
        }
    }

    fn lit_addr(&self, s: usize) -> Option<u32> {
        let Body::Insn { pool: Some(p), .. } = &self.stmts[s].body else { return None };
        let e = self.lit_expr(s)?;
        let v = self.eval(self.stmts[s].file, e, Some(self.stmts[s].addr)).ok()?;
        let dv = v.data_value();
        let slot = self.pools[*p].slots.iter().position(|&x| x == dv)?;
        let pool_stmt = self.stmts.iter().position(|st| matches!(st.body, Body::Pool(q) if q == *p))?;
        Some(self.stmts[pool_stmt].addr + 4 * slot as u32)
    }

    fn encode_stmt(&self, s: usize, force_wide: bool) -> Result<Vec<u8>, String> {
        let st = &self.stmts[s];
        let Body::Insn { insn, in_it, last_in_it, .. } = &st.body else { return Ok(Vec::new()) };
        let ctx = Ctx { pc: st.addr, in_it: *in_it, last_in_it: *last_in_it, force_wide, lit_addr: self.lit_addr(s) };
        encode::encode(insn, ctx, &FileEnv { asm: self, file: st.file })
    }

    /// One measurement pass; returns true if any reservation grew.
    /// Re-size every statement for the current placement. The first passes size exactly
    /// (a size may shrink once addresses settle, e.g. a branch to an absolute address
    /// measured while everything before it was still zero-sized); later passes only grow,
    /// which guarantees termination.
    fn measure(&mut self, exact: bool) -> bool {
        let mut changed = false;
        // literal pools
        let mut slots: Vec<Vec<u32>> = vec![Vec::new(); self.pools.len()];
        for s in 0..self.stmts.len() {
            let Body::Insn { pool: Some(p), .. } = &self.stmts[s].body else { continue };
            let p = *p;
            let Some(e) = self.lit_expr(s) else { continue };
            if let Ok(v) = self.eval(self.stmts[s].file, e, Some(self.stmts[s].addr)) {
                if encode::literal_needs_pool(&v) {
                    let dv = v.data_value();
                    if !slots[p].contains(&dv) {
                        slots[p].push(dv);
                    }
                }
            }
        }
        for (p, sl) in slots.into_iter().enumerate() {
            let n = sl.len() as u32;
            if n > self.pools[p].reserved || (exact && n != self.pools[p].reserved) {
                self.pools[p].reserved = n;
                changed = true;
            }
            self.pools[p].slots = sl;
        }
        for s in 0..self.stmts.len() {
            let need = match &self.stmts[s].body {
                Body::Insn { .. } => match self.encode_stmt(s, false) {
                    Ok(b) => b.len() as u32,
                    Err(_) => 0,
                },
                Body::Space { size, .. } => match self.eval(self.stmts[s].file, size, Some(self.stmts[s].addr)) {
                    Ok(v) if v.is_const() && (0..=0x0100_0000).contains(&v.v) => v.v as u32,
                    _ => 0,
                },
                _ => 0,
            };
            if need > self.stmts[s].size || (exact && need != self.stmts[s].size) {
                self.stmts[s].size = need;
                changed = true;
            }
        }
        changed
    }

    fn emit_stmt(&self, s: usize) -> Result<Vec<u8>, String> {
        let st = &self.stmts[s];
        let file = st.file;
        match &st.body {
            Body::Label | Body::Align { .. } => Ok(Vec::new()),
            Body::Insn { in_it, .. } => {
                let mut b = self.encode_stmt(s, st.size == 4)?;
                if b.len() == 2 && st.size == 4 {
                    if *in_it {
                        return Err("internal: relaxation needs a 32-bit form inside an IT block; add .W".into());
                    }
                    b.extend_from_slice(&[0x00, 0xBF]);
                }
                if b.len() as u32 != st.size {
                    return Err("internal: instruction size changed during layout".into());
                }
                Ok(b)
            }
            Body::Data { unit, items, .. } => {
                let mut out = Vec::new();
                for it in items {
                    match it {
                        DataItem::Bytes(b) => out.extend_from_slice(b),
                        DataItem::Expr(e) => {
                            let pc = st.addr + out.len() as u32;
                            let v = self.eval(file, e, Some(pc)).map_err(|x| x.to_string())?;
                            match unit {
                                1 => {
                                    if !v.is_const() {
                                        return Err(format!("DCB value '{}' is an address; use DCD", e));
                                    }
                                    if !(-128..=255).contains(&v.v) {
                                        return Err(format!("DCB value {} out of range (-128..255)", v.v));
                                    }
                                    out.push(v.v as u8);
                                }
                                2 => {
                                    if !(-32768..=65535).contains(&v.v) {
                                        return Err(format!("DCW value {} out of range (-32768..65535)", v.v));
                                    }
                                    out.extend_from_slice(&(v.data_value() as u16).to_le_bytes());
                                }
                                4 => {
                                    if v.v < i32::MIN as i64 || v.v > u32::MAX as i64 {
                                        return Err(format!("DCD value 0x{:X} does not fit in 32 bits", v.v));
                                    }
                                    out.extend_from_slice(&v.data_value().to_le_bytes());
                                }
                                _ => out.extend_from_slice(&(v.v as u64).to_le_bytes()),
                            }
                        }
                    }
                }
                Ok(out)
            }
            Body::Space { fill, unit, .. } => {
                let fv = match fill {
                    Some(f) => {
                        let v = self.eval(file, f, Some(st.addr)).map_err(|x| x.to_string())?;
                        v.v as u32
                    }
                    None => 0,
                };
                let mut out = Vec::with_capacity(st.size as usize);
                let bytes = fv.to_le_bytes();
                for i in 0..st.size as usize {
                    out.push(bytes[i % *unit as usize]);
                }
                Ok(out)
            }
            Body::Pool(p) => {
                let pool = &self.pools[*p];
                let mut out = Vec::new();
                for v in &pool.slots {
                    out.extend_from_slice(&v.to_le_bytes());
                }
                out.resize(st.size as usize, 0);
                Ok(out)
            }
        }
    }

    fn size_of_area_bytes(&self, kinds: &[AreaKind], virt: Option<bool>, insn_only: Option<bool>) -> u32 {
        let mut n = 0;
        for a in &self.areas {
            if !kinds.contains(&a.kind) || virt.is_some_and(|v| self.files[a.file].virt != v) {
                continue;
            }
            for &s in &a.stmts {
                let st = &self.stmts[s];
                let is_insn = matches!(st.body, Body::Insn { .. });
                if insn_only.is_none_or(|i| i == is_insn) {
                    n += st.size + st.pad;
                }
            }
        }
        n
    }
}

fn fill_pad(buf: &mut [u8], start_addr: u32, code: bool) {
    if !code {
        return;
    }
    let mut i = 0;
    if start_addr & 1 == 1 && !buf.is_empty() {
        buf[0] = 0;
        i = 1;
    }
    while i + 2 <= buf.len() {
        buf[i] = 0x00;
        buf[i + 1] = 0xBF;
        i += 2;
    }
}

// ---------------------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
pub fn assemble_project(project: &UvProject) -> Result<BuildResult> {
    let t0 = Instant::now();
    let build_dir = project.build_dir();
    std::fs::create_dir_all(&build_dir)?;

    let mut missing: Vec<Diagnostic> = Vec::new();
    let mut sources: Vec<(String, String)> = Vec::new();
    for src in project.source_files() {
        if src.file_type != 2 {
            continue;
        }
        let p = Path::new(&src.abs_path);
        match std::fs::read(p) {
            Ok(bytes) => sources.push((src.abs_path.clone(), String::from_utf8_lossy(&bytes).into_owned())),
            Err(_) => missing.push(Diagnostic {
                file: src.abs_path.clone(),
                line: 1,
                column: 1,
                severity: "error".to_string(),
                code: "ENOENT".to_string(),
                message: format!("Source file not found: {}", src.abs_path),
            }),
        }
    }
    if !missing.is_empty() {
        let errs = missing.len();
        let elapsed = t0.elapsed().as_millis() as u64;
        return Ok(BuildResult {
            success: false,
            axf_path: None,
            hex_path: None,
            bin_path: None,
            elapsed_ms: elapsed,
            errors: errs,
            warnings: 0,
            diagnostics: missing,
            log: format!(
                "*** Using Built-in uVision Fast Assembler ({}, Cortex-M3)\nBuilt-in assemble failed: {} Error(s). [{} ms]",
                project.options.device, errs, elapsed
            ),
        });
    }

    let axf_path = project.axf_path();
    let hex_path = project.options.create_hex.then(|| project.hex_path());
    let bin_path = project.options.create_bin.then(|| project.bin_path());
    let mut mem = assemble_sources(
        &project.options,
        &axf_path.to_string_lossy(),
        hex_path.as_ref().map(|p| p.to_string_lossy().into_owned()).as_deref(),
        bin_path.as_ref().map(|p| p.to_string_lossy().into_owned()).as_deref(),
        &sources,
    );
    if mem.result.success {
        if let Some(elf) = &mem.elf_bytes {
            std::fs::write(&axf_path, elf)?;
        }
        if let (Some(hp), Some(t)) = (hex_path, &mem.hex_text) {
            std::fs::write(&hp, t)?;
        }
        if let (Some(bp), Some(b)) = (bin_path, &mem.bin_bytes) {
            std::fs::write(&bp, b)?;
        }
    }
    let elapsed_ms = t0.elapsed().as_millis() as u64;
    mem.result.elapsed_ms = elapsed_ms;
    mem.result.log = format!("{} [{} ms]", mem.result.log, elapsed_ms);
    Ok(mem.result)
}

fn failed(log: &mut Vec<String>, asm: Asm) -> InMemoryBuild {
    let errors = asm.errors();
    let warnings = asm.diags.len() - errors;
    for d in &asm.diags {
        log.push(format!("{}({}): {}: {}", d.file, d.line, d.severity, d.message));
    }
    log.push(format!("Built-in assemble failed: {} Error(s), {} Warning(s).", errors, warnings));
    InMemoryBuild {
        result: BuildResult {
            success: false,
            axf_path: None,
            hex_path: None,
            bin_path: None,
            elapsed_ms: 0,
            errors,
            warnings,
            diagnostics: asm.diags,
            log: log.join("\n"),
        },
        elf_bytes: None,
        hex_text: None,
        bin_bytes: None,
    }
}

pub fn assemble_sources(
    options: &TargetOptions,
    axf_label: &str,
    hex_label: Option<&str>,
    bin_label: Option<&str>,
    sources: &[(String, String)],
) -> InMemoryBuild {
    let mut log = vec![format!("*** Using Built-in uVision Fast Assembler ({}, Cortex-M3)", options.device)];
    let map = DeviceMap::for_device(&options.device);
    let mut asm = Asm::new();
    for d in &options.a_defines {
        let (n, v) = match d.split_once('=') {
            Some((n, v)) => (n.trim().to_string(), v.trim().to_string()),
            None => (d.trim().to_string(), "1".to_string()),
        };
        if valid_ident(&n) {
            asm.defines.push((n, v));
        }
    }
    for (path, text) in sources {
        let short = path.replace('\\', "/");
        let short = short.rsplit('/').next().unwrap_or(path);
        log.push(format!("assembling {}...", short));
        asm.parse_file(path, text, false);
    }
    if asm.errors() > 0 {
        return failed(&mut log, asm);
    }
    asm.check_exports();
    let plan = asm.synthesize_startup(options, &map);
    asm.handler_warnings();
    if asm.errors() > 0 {
        return failed(&mut log, asm);
    }

    // Layout
    let (rom_order, rw_order, zi_order) = asm.orders();
    let mut lay = Layout {
        rom_order,
        rw_order,
        zi_order,
        rom_end: 0,
        rw_load: 0,
        rw_base: 0,
        rw_limit: 0,
        zi_base: 0,
        zi_limit: 0,
    };
    let ro_base = options.ro_base;
    let rw_base = options.rw_base;
    let mut converged = false;
    for pass in 0..200 {
        asm.place(ro_base, rw_base, &mut lay);
        if !asm.measure(pass < 8) {
            converged = true;
            break;
        }
    }
    if !converged {
        let f = asm.files.iter().position(|f| !f.virt).unwrap_or(0);
        asm.diag(f, 1, "error", "internal: layout did not converge");
        return failed(&mut log, asm);
    }

    // Emit
    let rom_len = (lay.rom_end - ro_base) as usize;
    let mut rom = vec![0u8; rom_len];
    let rw_len = (lay.rw_limit - lay.rw_base) as usize;
    let mut rw = vec![0u8; rw_len];
    let mut rows: Vec<elfout::LineRow> = Vec::new();
    let mut dwarf_files: Vec<String> = Vec::new();
    let mut dwarf_idx: HashMap<usize, usize> = HashMap::new();
    for (fi, f) in asm.files.iter().enumerate() {
        if !f.virt {
            dwarf_idx.insert(fi, dwarf_files.len());
            dwarf_files.push(f.path.clone());
        }
    }
    let mut errs: Vec<(usize, u32, String)> = Vec::new();
    let regions: Vec<(Vec<usize>, bool)> = vec![(lay.rom_order.clone(), true), (lay.rw_order.clone(), false)];
    for (order, is_rom) in regions {
        for a in order {
            let code = asm.areas[a].kind == AreaKind::Code;
            for &s in &asm.areas[a].stmts {
                let st = &asm.stmts[s];
                let (buf, base) = if is_rom { (&mut rom, ro_base) } else { (&mut rw, lay.rw_base) };
                let start = (st.addr - st.pad - base) as usize;
                fill_pad(&mut buf[start..start + st.pad as usize], st.addr - st.pad, code);
                match asm.emit_stmt(s) {
                    Ok(b) => {
                        let off = (st.addr - base) as usize;
                        buf[off..off + b.len()].copy_from_slice(&b);
                        if matches!(st.body, Body::Insn { .. }) {
                            if let Some(&di) = dwarf_idx.get(&st.file) {
                                rows.push(elfout::LineRow { addr: st.addr, file: di, line: st.line });
                            }
                        }
                    }
                    Err(m) => errs.push((st.file, st.line, m)),
                }
            }
        }
    }
    for (f, l, m) in errs {
        asm.diag(f, l, "error", m);
    }
    if rom_len as u32 > map.flash_size {
        let f = asm.files.iter().position(|f| !f.virt).unwrap_or(0);
        asm.diag(f, 1, "error", format!("Program ({} bytes) does not fit in the {} KB flash of {}", rom_len, map.flash_size / 1024, map.name));
    }
    let ram_used = lay.zi_limit - lay.rw_base;
    if ram_used > map.sram_size {
        let f = asm.files.iter().position(|f| !f.virt).unwrap_or(0);
        asm.diag(f, 1, "error", format!("RW + ZI data ({} bytes) does not fit in the {} KB SRAM of {}", ram_used, map.sram_size / 1024, map.name));
    }
    let scatter = plan.as_ref().is_some_and(|p| p.main && !p.reset_to_user_main);
    if rw_len > 0 && !scatter {
        let a = lay.rw_order[0];
        let f = asm.areas[a].file;
        let line = asm.areas[a].stmts.first().map(|&s| asm.stmts[s].line).unwrap_or(1);
        asm.diag(
            f,
            line,
            "warning",
            format!(
                "READWRITE data in AREA '{}' is only copied to RAM by the C library's __main (scatter-load); with your own entry code it starts uninitialised on the chip",
                asm.areas[a].name
            ),
        );
    }
    if asm.errors() > 0 {
        return failed(&mut log, asm);
    }
    rows.sort_by_key(|r| r.addr);

    // Symbols
    let mut syms: Vec<elfout::Sym> = Vec::new();
    let mut proc_size: HashMap<(usize, String), u32> = HashMap::new();
    for p in &asm.procs {
        if p.end > p.start {
            let first = &asm.stmts[p.start];
            let last = &asm.stmts[p.end - 1];
            if first.area == last.area {
                proc_size.insert((p.file, p.name.clone()), last.addr + last.size - first.addr);
            }
        }
    }
    for (fi, f) in asm.files.iter().enumerate() {
        let mut names: Vec<&String> = f.syms.keys().collect();
        names.sort();
        for name in names {
            if name == HIDDEN_ENTRY {
                continue;
            }
            let SymDef::Label(s) = &f.syms[name] else { continue };
            let st = &asm.stmts[*s];
            let thumb = matches!(st.body, Body::Insn { .. });
            let sec = match asm.areas[st.area].kind {
                AreaKind::Code | AreaKind::RoData => elfout::Sec::Text,
                AreaKind::RwData => elfout::Sec::Data,
                AreaKind::Zi => elfout::Sec::Bss,
            };
            syms.push(elfout::Sym {
                name: name.clone(),
                value: if thumb { st.addr | 1 } else { st.addr },
                size: proc_size.get(&(fi, name.clone())).copied().unwrap_or(0),
                global: f.exports.contains(name),
                func: thumb,
                notype: false,
                sec,
            });
        }
    }
    // Mapping symbols ($t code / $d data)
    let mut last: Option<bool> = None;
    for &a in lay.rom_order.iter().chain(lay.rw_order.iter()) {
        let rw_area = asm.areas[a].kind == AreaKind::RwData;
        if rw_area {
            last = None;
        }
        for &s in &asm.areas[a].stmts {
            let st = &asm.stmts[s];
            if st.size == 0 {
                continue;
            }
            let is_code = matches!(st.body, Body::Insn { .. });
            if last != Some(is_code) {
                syms.push(elfout::Sym {
                    name: if is_code { "$t".into() } else { "$d".into() },
                    value: st.addr,
                    size: 0,
                    global: false,
                    func: false,
                    notype: true,
                    sec: if rw_area { elfout::Sec::Data } else { elfout::Sec::Text },
                });
                last = Some(is_code);
            }
        }
    }

    let entry = if rom.len() >= 8 {
        u32::from_le_bytes([rom[4], rom[5], rom[6], rom[7]])
    } else {
        ro_base | 1
    };
    let zi_size = lay.zi_limit - lay.zi_base;
    let elf_bytes = elfout::build(&elfout::Image {
        entry,
        rom_base: ro_base,
        rom: &rom[..(lay.rw_load - ro_base) as usize],
        rw: (rw_len > 0).then_some((lay.rw_base, lay.rw_load, rw.as_slice())),
        zi: (zi_size > 0).then_some((lay.zi_base, zi_size)),
        syms,
        files: &dwarf_files,
        rows: &rows,
    });
    let mut image = rom.clone();
    image[(lay.rw_load - ro_base) as usize..].copy_from_slice(&rw);
    let hex_text = format_intel_hex(ro_base, &image);

    let code = asm.size_of_area_bytes(&[AreaKind::Code], None, None);
    let ro_data = asm.size_of_area_bytes(&[AreaKind::RoData], None, None);
    let errors = 0;
    let warnings = asm.diags.len();
    for d in &asm.diags {
        log.push(format!("{}({}): {}: {}", d.file, d.line, d.severity, d.message));
    }
    log.push(format!(
        "Program Size: Code={} RO-data={} RW-data={} ZI-data={}",
        code, ro_data, rw_len, zi_size
    ));
    log.push(format!("\"{}\" - {} Error(s), {} Warning(s).", axf_label, errors, warnings));

    InMemoryBuild {
        result: BuildResult {
            success: true,
            axf_path: Some(axf_label.to_string()),
            hex_path: hex_label.map(|s| s.to_string()),
            bin_path: bin_label.map(|s| s.to_string()),
            elapsed_ms: 0,
            errors,
            warnings,
            diagnostics: asm.diags,
            log: log.join("\n"),
        },
        elf_bytes: Some(elf_bytes),
        hex_text: Some(hex_text),
        bin_bytes: Some(image),
    }
}

#[cfg(test)]
mod tests;
