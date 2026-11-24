use crate::project::TargetOptions;
#[cfg(not(target_arch = "wasm32"))]
use crate::project::UvProject;
use crate::sim::cpu::thumb_expand_imm_c;
use crate::target::{BuildResult, Diagnostic};
use anyhow::{anyhow, Result};
use std::collections::HashMap;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SectionKind {
    Reset,
    Code,
    Data,
}

#[derive(Debug, Clone)]
struct SourceLineItem {
    file_idx: usize,
    file_path: String,
    line: u32,
    section: SectionKind,
    label: Option<String>,
    stmt: Option<Stmt>,
}

#[derive(Debug, Clone)]
enum Stmt {
    Dcd(Vec<String>),
    Dcw(Vec<String>),
    Dcb(Vec<String>),
    Space(String),
    Align(u32),
    Ltorg,
    Instr {
        raw_mnem: String,
        operands: Vec<String>,
    },
}

#[derive(Debug, Clone)]
struct SymEntry {
    name: String,
    addr: u32, // un-masked address; thumb func bit set separately if is_func
    is_func: bool,
    section: SectionKind,
}

#[derive(Debug, Clone)]
struct LiteralEntry {
    expr: String,
    addr: u32,
}

#[derive(Debug, Clone)]
struct EmittedLine {
    addr: u32,
    file_idx: usize,
    line: u32,
}

#[cfg(not(target_arch = "wasm32"))]
pub fn assemble_project(project: &UvProject) -> Result<BuildResult> {
    let t0 = Instant::now();
    let build_dir = project.build_dir();
    std::fs::create_dir_all(&build_dir)?;

    let mut missing_diags: Vec<Diagnostic> = Vec::new();
    let mut missing_log: Vec<String> = vec![format!(
        "*** Using Built-in uVision Fast Assembler ({}, Cortex-M3)",
        project.options.device
    )];
    let mut sources: Vec<(String, String)> = Vec::new();

    for src in project.source_files() {
        if src.file_type != 2 {
            continue;
        }
        let p = Path::new(&src.abs_path);
        if !p.exists() {
            missing_diags.push(Diagnostic {
                file: src.abs_path.clone(),
                line: 1,
                column: 1,
                severity: "error".to_string(),
                code: "ENOENT".to_string(),
                message: format!("Source file not found: {}", src.abs_path),
            });
            continue;
        }
        let text = std::fs::read_to_string(p)?;
        sources.push((src.abs_path.clone(), text));
    }

    if !missing_diags.is_empty() {
        let errs = missing_diags.len();
        let elapsed = t0.elapsed().as_millis() as u64;
        missing_log.push(format!(
            "Built-in assemble failed: {} Error(s). [{} ms]",
            errs, elapsed
        ));
        return Ok(BuildResult {
            success: false,
            axf_path: None,
            hex_path: None,
            bin_path: None,
            elapsed_ms: elapsed,
            errors: errs,
            warnings: 0,
            diagnostics: missing_diags,
            log: missing_log.join("\n"),
        });
    }

    let axf_path = project.axf_path();
    let hex_path = if project.options.create_hex {
        Some(project.hex_path())
    } else {
        None
    };
    let bin_path = if project.options.create_bin {
        Some(project.bin_path())
    } else {
        None
    };

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
        if let (Some(hp), Some(hex_text)) = (hex_path, &mem.hex_text) {
            std::fs::write(&hp, hex_text)?;
        }
        if let (Some(bp), Some(bin_bytes)) = (bin_path, &mem.bin_bytes) {
            std::fs::write(&bp, bin_bytes)?;
        }
    }

    let elapsed_ms = t0.elapsed().as_millis() as u64;
    mem.result.elapsed_ms = elapsed_ms;
    mem.result.log = format!("{} [{} ms]", mem.result.log, elapsed_ms);
    Ok(mem.result)
}

pub fn assemble_sources(
    options: &TargetOptions,
    axf_label: &str,
    hex_label: Option<&str>,
    bin_label: Option<&str>,
    sources: &[(String, String)],
) -> InMemoryBuild {
    let mut log = vec![format!(
        "*** Using Built-in uVision Fast Assembler ({}, Cortex-M3)",
        options.device
    )];
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut equs: HashMap<String, i64> = HashMap::new();
    let mut items: Vec<SourceLineItem> = Vec::new();
    let mut files: Vec<String> = Vec::new();

    for (src_path, text) in sources {
        let name = src_path.replace('\\', "/");
        let short_name = name.rsplit('/').next().unwrap_or(src_path);
        log.push(format!("assembling {} (built-in)...", short_name));
        let file_idx = files.len();
        files.push(src_path.clone());
        parse_asm_file(text, file_idx, src_path, &mut equs, &mut items, &mut diagnostics);
    }

    if !diagnostics.is_empty() {
        for d in &diagnostics {
            log.push(format!("{}({}): error: {}", d.file, d.line, d.message));
        }
        let errs = diagnostics.len();
        log.push(format!("Built-in assemble failed: {} Error(s).", errs));
        return InMemoryBuild {
            result: BuildResult {
                success: false,
                axf_path: None,
                hex_path: None,
                bin_path: None,
                elapsed_ms: 0,
                errors: errs,
                warnings: 0,
                diagnostics,
                log: log.join("\n"),
            },
            elf_bytes: None,
            hex_text: None,
            bin_bytes: None,
        };
    }

    let ro_base = options.ro_base;
    let rw_base = options.rw_base;

    // Partition items into Reset, Code, Data preserving order within each
    let mut reset_items = Vec::new();
    let mut code_items = Vec::new();
    let mut data_items = Vec::new();
    for it in items {
        match it.section {
            SectionKind::Reset => reset_items.push(it),
            SectionKind::Code => code_items.push(it),
            SectionKind::Data => data_items.push(it),
        }
    }

    // Iterative layout pass to resolve addresses + literal pool
    let mut symbols: HashMap<String, SymEntry> = HashMap::new();
    let mut literals: Vec<LiteralEntry> = Vec::new();
    let mut reset_size = 0u32;
    let mut code_size = 0u32;
    let mut data_size = 0u32;

    for pass in 0..6 {
        let mut next_symbols: HashMap<String, SymEntry> = HashMap::new();
        literals.clear();

        // 1) Reset section at ro_base
        let mut pc = ro_base;
        for it in &reset_items {
            if let Some(lbl) = &it.label {
                next_symbols.insert(
                    lbl.clone(),
                    SymEntry {
                        name: lbl.clone(),
                        addr: pc,
                        is_func: false,
                        section: SectionKind::Reset,
                    },
                );
            }
            if let Some(st) = &it.stmt {
                pc = advance_pc(pc, st, &equs, &symbols, &mut literals, pass == 0);
            }
        }
        pc = (pc + 3) & !3;
        reset_size = pc - ro_base;

        // 2) Code section right after Reset
        let code_base = ro_base + reset_size;
        pc = code_base;
        for it in &code_items {
            if let Some(lbl) = &it.label {
                next_symbols.insert(
                    lbl.clone(),
                    SymEntry {
                        name: lbl.clone(),
                        addr: pc,
                        is_func: true,
                        section: SectionKind::Code,
                    },
                );
            }
            if let Some(st) = &it.stmt {
                if matches!(st, Stmt::Ltorg) {
                    pc = (pc + 3) & !3;
                    for lit in &mut literals {
                        if lit.addr == 0 {
                            lit.addr = pc;
                            pc += 4;
                        }
                    }
                } else {
                    pc = advance_pc(pc, st, &equs, &symbols, &mut literals, pass == 0);
                }
            }
        }
        // Flush remaining literal pool at end of code section
        pc = (pc + 3) & !3;
        for lit in &mut literals {
            if lit.addr == 0 {
                lit.addr = pc;
                pc += 4;
            }
        }
        code_size = pc - code_base;

        // 3) Data section at rw_base
        pc = rw_base;
        for it in &data_items {
            if let Some(lbl) = &it.label {
                next_symbols.insert(
                    lbl.clone(),
                    SymEntry {
                        name: lbl.clone(),
                        addr: pc,
                        is_func: false,
                        section: SectionKind::Data,
                    },
                );
            }
            if let Some(st) = &it.stmt {
                pc = advance_pc(pc, st, &equs, &symbols, &mut literals, pass == 0);
            }
        }
        data_size = pc - rw_base;

        let converged = pass > 0
            && next_symbols.len() == symbols.len()
            && next_symbols
                .iter()
                .all(|(k, v)| symbols.get(k).map(|s| s.addr) == Some(v.addr));
        symbols = next_symbols;
        if converged {
            break;
        }
    }

    // Pass 2: Emit bytes
    let mut ro_bytes: Vec<u8> = Vec::with_capacity((reset_size + code_size) as usize);
    let mut rw_bytes: Vec<u8> = Vec::with_capacity(data_size as usize);
    let mut line_rows: Vec<EmittedLine> = Vec::new();
    let mut lit_cursor = 0usize;
    let mut lit_emit_cursor = 0usize;

    // Emit Reset section
    for it in &reset_items {
        let cur_addr = ro_base + ro_bytes.len() as u32;
        if let Some(st) = &it.stmt {
            if let Err(msg) = emit_stmt(
                cur_addr,
                st,
                &equs,
                &symbols,
                &literals,
                &mut lit_cursor,
                &mut ro_bytes,
                None,
            ) {
                diagnostics.push(Diagnostic {
                    file: it.file_path.clone(),
                    line: it.line,
                    column: 1,
                    severity: "error".to_string(),
                    code: "ASM".to_string(),
                    message: msg.to_string(),
                });
            }
        }
    }
    while ro_bytes.len() < reset_size as usize {
        ro_bytes.push(0);
    }

    // Emit Code section
    for it in &code_items {
        let cur_addr = ro_base + ro_bytes.len() as u32;
        if let Some(st) = &it.stmt {
            if matches!(st, Stmt::Ltorg) {
                while (ro_bytes.len() & 3) != 0 {
                    ro_bytes.push(0);
                }
                while lit_emit_cursor < literals.len() && literals[lit_emit_cursor].addr == ro_base + ro_bytes.len() as u32 {
                    let v = eval_expr(&literals[lit_emit_cursor].expr, &equs, &symbols).unwrap_or(0) as u32;
                    ro_bytes.extend_from_slice(&v.to_le_bytes());
                    lit_emit_cursor += 1;
                }
                continue;
            }
            let before = ro_bytes.len();
            if let Err(msg) = emit_stmt(
                cur_addr,
                st,
                &equs,
                &symbols,
                &literals,
                &mut lit_cursor,
                &mut ro_bytes,
                None,
            ) {
                diagnostics.push(Diagnostic {
                    file: it.file_path.clone(),
                    line: it.line,
                    column: 1,
                    severity: "error".to_string(),
                    code: "ASM".to_string(),
                    message: msg.to_string(),
                });
            } else if matches!(st, Stmt::Instr { .. }) && ro_bytes.len() > before {
                line_rows.push(EmittedLine {
                    addr: cur_addr,
                    file_idx: it.file_idx,
                    line: it.line,
                });
            }
        }
    }
    while (ro_bytes.len() & 3) != 0 {
        ro_bytes.push(0);
    }
    while lit_emit_cursor < literals.len() {
        let v = eval_expr(&literals[lit_emit_cursor].expr, &equs, &symbols).unwrap_or(0) as u32;
        ro_bytes.extend_from_slice(&v.to_le_bytes());
        lit_emit_cursor += 1;
    }

    // Emit Data section
    for it in &data_items {
        let cur_addr = rw_base + rw_bytes.len() as u32;
        if let Some(st) = &it.stmt {
            if let Err(msg) = emit_stmt(
                cur_addr,
                st,
                &equs,
                &symbols,
                &literals,
                &mut lit_cursor,
                &mut rw_bytes,
                None,
            ) {
                diagnostics.push(Diagnostic {
                    file: it.file_path.clone(),
                    line: it.line,
                    column: 1,
                    severity: "error".to_string(),
                    code: "ASM".to_string(),
                    message: msg.to_string(),
                });
            }
        }
    }

    if !diagnostics.is_empty() {
        for d in &diagnostics {
            log.push(format!("{}({}): error: {}", d.file, d.line, d.message));
        }
        let errs = diagnostics.len();
        log.push(format!("Built-in assemble failed: {} Error(s).", errs));
        return InMemoryBuild {
            result: BuildResult {
                success: false,
                axf_path: None,
                hex_path: None,
                bin_path: None,
                elapsed_ms: 0,
                errors: errs,
                warnings: 0,
                diagnostics,
                log: log.join("\n"),
            },
            elf_bytes: None,
            hex_text: None,
            bin_bytes: None,
        };
    }

    let entry = symbols
        .get("Reset_Handler")
        .or_else(|| symbols.get("main"))
        .or_else(|| symbols.get("_start"))
        .map(|s| s.addr | 1)
        .unwrap_or((ro_base + reset_size) | 1);

    let elf_bytes = build_elf32(
        entry,
        ro_base,
        &ro_bytes,
        rw_base,
        &rw_bytes,
        &symbols,
        &files,
        &line_rows,
    );
    let hex_text = format_intel_hex(ro_base, &ro_bytes);
    let bin_bytes = ro_bytes.clone();

    log.push(format!(
        "Program Size: Code={} bytes, RO-data={} bytes, RW-data={} bytes",
        code_size, reset_size, rw_bytes.len()
    ));
    log.push(format!(
        "\"{}\" - 0 Error(s), 0 Warning(s).",
        axf_label
    ));

    InMemoryBuild {
        result: BuildResult {
            success: true,
            axf_path: Some(axf_label.to_string()),
            hex_path: hex_label.map(|s| s.to_string()),
            bin_path: bin_label.map(|s| s.to_string()),
            elapsed_ms: 0,
            errors: 0,
            warnings: 0,
            diagnostics,
            log: log.join("\n"),
        },
        elf_bytes: Some(elf_bytes),
        hex_text: Some(hex_text),
        bin_bytes: Some(bin_bytes),
    }
}

fn parse_asm_file(
    text: &str,
    file_idx: usize,
    file_path: &str,
    equs: &mut HashMap<String, i64>,
    items: &mut Vec<SourceLineItem>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut section = SectionKind::Code;
    for (idx, raw_line) in text.lines().enumerate() {
        let line_no = (idx + 1) as u32;
        let stripped = strip_comment(raw_line);
        if stripped.trim().is_empty() {
            continue;
        }

        // Handle EQU early (e.g. NAME EQU 0x100)
        if let Some((eq_name, eq_val)) = try_parse_equ(stripped) {
            if let Ok(v) = eval_expr(eq_val, equs, &HashMap::new()) {
                equs.insert(eq_name.to_string(), v);
                continue;
            }
        }

        let leading_ws = stripped.chars().next().map(|c| c.is_whitespace()).unwrap_or(false);
        let mut rest = stripped.trim();
        let mut label: Option<String> = None;

        // Colon label anywhere (e.g. "Reset_Handler:")
        if let Some(colon) = find_label_colon(rest) {
            let cand = rest[..colon].trim();
            if is_valid_ident(cand) {
                label = Some(cand.to_string());
                rest = rest[colon + 1..].trim();
            }
        } else if !leading_ws {
            // Keil column-0 label (without colon)
            let mut end = 0;
            for (bi, ch) in rest.char_indices() {
                if ch.is_whitespace() {
                    break;
                }
                end = bi + ch.len_utf8();
            }
            let first = &rest[..end];
            let first_up = first.to_ascii_uppercase();
            if !is_directive(&first_up) && is_valid_ident(first) {
                label = Some(first.to_string());
                rest = rest[end..].trim();
            }
        }

        if rest.is_empty() {
            if label.is_some() {
                items.push(SourceLineItem {
                    file_idx,
                    file_path: file_path.to_string(),
                    line: line_no,
                    section,
                    label,
                    stmt: None,
                });
            }
            continue;
        }

        let (head, tail) = split_first_word(rest);
        let head_up = head.to_ascii_uppercase();

        match head_up.as_str() {
            "PRESERVE8" | "REQUIRE8" | "THUMB" | "ARM" | "ENTRY" | "EXPORT" | "GLOBAL"
            | "IMPORT" | "EXTERN" | ".SYNTAX" | ".THUMB" | ".CPU" | ".GLOBAL" | ".GLOBL"
            | ".TYPE" | ".TEXT" => {
                if head_up == ".TEXT" {
                    section = SectionKind::Code;
                }
                if label.is_some() {
                    items.push(SourceLineItem {
                        file_idx,
                        file_path: file_path.to_string(),
                        line: line_no,
                        section,
                        label,
                        stmt: None,
                    });
                }
            }
            "END" => {
                if label.is_some() {
                    items.push(SourceLineItem {
                        file_idx,
                        file_path: file_path.to_string(),
                        line: line_no,
                        section,
                        label,
                        stmt: None,
                    });
                }
                break;
            }
            "AREA" | ".SECTION" => {
                let tail_up = tail.to_ascii_uppercase();
                if tail_up.contains("RESET") || tail_up.contains("VECTORS") {
                    section = SectionKind::Reset;
                } else if tail_up.contains("READWRITE") || tail_up.contains(".DATA") || tail_up.contains(".BSS") {
                    section = SectionKind::Data;
                } else {
                    section = SectionKind::Code;
                }
                if label.is_some() {
                    items.push(SourceLineItem {
                        file_idx,
                        file_path: file_path.to_string(),
                        line: line_no,
                        section,
                        label,
                        stmt: None,
                    });
                }
            }
            "ALIGN" | ".ALIGN" | ".BALIGN" => {
                let align_val = tail
                    .split(',')
                    .next()
                    .and_then(|s| eval_expr(s.trim(), equs, &HashMap::new()).ok())
                    .map(|v| v.max(1) as u32)
                    .unwrap_or(4);
                items.push(SourceLineItem {
                    file_idx,
                    file_path: file_path.to_string(),
                    line: line_no,
                    section,
                    label,
                    stmt: Some(Stmt::Align(align_val)),
                });
            }
            "LTORG" | ".LTORG" => {
                items.push(SourceLineItem {
                    file_idx,
                    file_path: file_path.to_string(),
                    line: line_no,
                    section,
                    label,
                    stmt: Some(Stmt::Ltorg),
                });
            }
            "DCD" | "DCDU" | ".WORD" | ".LONG" => {
                items.push(SourceLineItem {
                    file_idx,
                    file_path: file_path.to_string(),
                    line: line_no,
                    section,
                    label,
                    stmt: Some(Stmt::Dcd(split_operands(tail))),
                });
            }
            "DCW" | "DCWU" | ".HWORD" | ".SHORT" => {
                items.push(SourceLineItem {
                    file_idx,
                    file_path: file_path.to_string(),
                    line: line_no,
                    section,
                    label,
                    stmt: Some(Stmt::Dcw(split_operands(tail))),
                });
            }
            "DCB" | ".BYTE" => {
                items.push(SourceLineItem {
                    file_idx,
                    file_path: file_path.to_string(),
                    line: line_no,
                    section,
                    label,
                    stmt: Some(Stmt::Dcb(split_operands(tail))),
                });
            }
            "SPACE" | "FILL" | ".SPACE" => {
                items.push(SourceLineItem {
                    file_idx,
                    file_path: file_path.to_string(),
                    line: line_no,
                    section,
                    label,
                    stmt: Some(Stmt::Space(tail.trim().to_string())),
                });
            }
            _ => {
                if tail.is_empty() && !is_zero_operand_mnem(&head_up) && is_valid_ident(head) {
                    diagnostics.push(Diagnostic {
                        file: file_path.to_string(),
                        line: line_no,
                        column: 1,
                        severity: "error".to_string(),
                        code: "A1163E".to_string(),
                        message: format!("Unknown opcode '{}' (if it is a label, put it at column 0 or add ':')", head),
                    });
                    continue;
                }
                items.push(SourceLineItem {
                    file_idx,
                    file_path: file_path.to_string(),
                    line: line_no,
                    section,
                    label,
                    stmt: Some(Stmt::Instr {
                        raw_mnem: head_up,
                        operands: split_operands(tail),
                    }),
                });
            }
        }
    }
}

fn advance_pc(
    pc: u32,
    stmt: &Stmt,
    equs: &HashMap<String, i64>,
    symbols: &HashMap<String, SymEntry>,
    literals: &mut Vec<LiteralEntry>,
    first_pass: bool,
) -> u32 {
    match stmt {
        Stmt::Dcd(v) => pc + (v.len() as u32) * 4,
        Stmt::Dcw(v) => pc + (v.len() as u32) * 2,
        Stmt::Dcb(v) => pc + count_dcb_bytes(v),
        Stmt::Space(expr) => pc + (eval_expr(expr, equs, symbols).unwrap_or(0).max(0) as u32),
        Stmt::Align(a) => {
            let align = (*a).max(1);
            let rem = pc % align;
            if rem == 0 { pc } else { pc + (align - rem) }
        }
        Stmt::Ltorg => (pc + 3) & !3,
        Stmt::Instr { raw_mnem, operands } => {
            if raw_mnem == "LDR" && operands.len() == 2 && operands[1].trim().starts_with('=') {
                let expr = operands[1].trim()[1..].trim().to_string();
                literals.push(LiteralEntry { expr, addr: 0 });
                return pc + 4;
            }
            let sz = measure_instr(pc, raw_mnem, operands, equs, symbols, first_pass);
            pc + sz
        }
    }
}

fn measure_instr(
    pc: u32,
    mnem: &str,
    ops: &[String],
    equs: &HashMap<String, i64>,
    symbols: &HashMap<String, SymEntry>,
    first_pass: bool,
) -> u32 {
    let mut tmp = Vec::new();
    let dummy_lits = Vec::new();
    let mut dummy_cur = 0;
    if encode_instr(pc, mnem, ops, equs, symbols, &dummy_lits, &mut dummy_cur, &mut tmp, true).is_ok() {
        tmp.len() as u32
    } else if first_pass {
        // Forward-reference branch/load default size
        4
    } else {
        4
    }
}

fn emit_stmt(
    pc: u32,
    stmt: &Stmt,
    equs: &HashMap<String, i64>,
    symbols: &HashMap<String, SymEntry>,
    literals: &[LiteralEntry],
    lit_cursor: &mut usize,
    out: &mut Vec<u8>,
    _unused: Option<()>,
) -> Result<()> {
    match stmt {
        Stmt::Dcd(v) => {
            for e in v {
                let val = eval_expr_for_data(e, equs, symbols)?;
                out.extend_from_slice(&val.to_le_bytes());
            }
        }
        Stmt::Dcw(v) => {
            for e in v {
                let val = eval_expr(e, equs, symbols)? as u16;
                out.extend_from_slice(&val.to_le_bytes());
            }
        }
        Stmt::Dcb(v) => {
            for e in v {
                let t = e.trim();
                if t.starts_with('"') && t.ends_with('"') && t.len() >= 2 {
                    out.extend_from_slice(t[1..t.len() - 1].as_bytes());
                } else {
                    let val = eval_expr(t, equs, symbols)? as u8;
                    out.push(val);
                }
            }
        }
        Stmt::Space(expr) => {
            let n = eval_expr(expr, equs, symbols)?.max(0) as usize;
            out.resize(out.len() + n, 0);
        }
        Stmt::Align(a) => {
            let align = (*a).max(1) as usize;
            let rem = (pc as usize) % align;
            if rem != 0 {
                out.resize(out.len() + (align - rem), 0);
            }
        }
        Stmt::Ltorg => {}
        Stmt::Instr { raw_mnem, operands } => {
            encode_instr(pc, raw_mnem, operands, equs, symbols, literals, lit_cursor, out, false)?;
        }
    }
    Ok(())
}

fn encode_instr(
    pc: u32,
    raw_mnem: &str,
    ops: &[String],
    equs: &HashMap<String, i64>,
    symbols: &HashMap<String, SymEntry>,
    literals: &[LiteralEntry],
    lit_cursor: &mut usize,
    out: &mut Vec<u8>,
    sizing_only: bool,
) -> Result<()> {
    let (base, setflags, wide, cond) = parse_mnemonic(raw_mnem);

    // Zero-operand hints
    match base.as_str() {
        "NOP" => { out.extend_from_slice(&0xBF00u16.to_le_bytes()); return Ok(()); }
        "WFE" => { out.extend_from_slice(&0xBF20u16.to_le_bytes()); return Ok(()); }
        "WFI" => { out.extend_from_slice(&0xBF30u16.to_le_bytes()); return Ok(()); }
        "SEV" => { out.extend_from_slice(&0xBF40u16.to_le_bytes()); return Ok(()); }
        "DSB" => { out.extend_from_slice(&0xF3BFu16.to_le_bytes()); out.extend_from_slice(&0x8F4Fu16.to_le_bytes()); return Ok(()); }
        "DMB" => { out.extend_from_slice(&0xF3BFu16.to_le_bytes()); out.extend_from_slice(&0x8F5Fu16.to_le_bytes()); return Ok(()); }
        "ISB" => { out.extend_from_slice(&0xF3BFu16.to_le_bytes()); out.extend_from_slice(&0x8F6Fu16.to_le_bytes()); return Ok(()); }
        "SVC" => {
            let imm = eval_imm(ops.first().map(|s| s.as_str()).unwrap_or("#0"), equs, symbols)? as u16;
            out.extend_from_slice(&(0xDF00 | (imm & 0xFF)).to_le_bytes());
            return Ok(());
        }
        "BKPT" => {
            let imm = eval_imm(ops.first().map(|s| s.as_str()).unwrap_or("#0"), equs, symbols)? as u16;
            out.extend_from_slice(&(0xBE00 | (imm & 0xFF)).to_le_bytes());
            return Ok(());
        }
        "CPSID" | "CPSIE" => {
            let dis = if base == "CPSID" { 1u16 << 4 } else { 0 };
            let flag = ops.first().map(|s| s.trim().to_ascii_uppercase()).unwrap_or_default();
            let i_bit = if flag.contains('I') { 2 } else { 0 };
            let f_bit = if flag.contains('F') { 1 } else { 0 };
            out.extend_from_slice(&(0xB660 | dis | i_bit | f_bit).to_le_bytes());
            return Ok(());
        }
        _ => {}
    }

    // Branches
    if base == "B" {
        let target_str = ops.first().ok_or_else(|| anyhow!("Missing branch target"))?;
        let target = match eval_branch_target(target_str, equs, symbols) {
            Ok(t) => t,
            Err(e) if sizing_only => {
                // Assume 4 bytes for unknown forward branch
                let _ = e;
                out.extend_from_slice(&[0; 4]);
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        let off = (target as i64) - ((pc + 4) as i64);
        if (off & 1) != 0 {
            return Err(anyhow!("Unaligned branch offset {}", off));
        }
        if let Some(c) = cond {
            if !wide && (-256..=254).contains(&off) {
                let imm8 = ((off >> 1) as i8 as u8) as u16;
                out.extend_from_slice(&(0xD000 | ((c as u16) << 8) | imm8).to_le_bytes());
                return Ok(());
            }
            if !(-1048576..=1048574).contains(&off) {
                return Err(anyhow!("Conditional branch out of range: {}", off));
            }
            let enc = encode_t32_bcond(c, off as i32);
            out.extend_from_slice(&enc.0.to_le_bytes());
            out.extend_from_slice(&enc.1.to_le_bytes());
            return Ok(());
        } else {
            if !wide && (-2048..=2046).contains(&off) {
                let imm11 = ((off >> 1) as u32 & 0x7FF) as u16;
                out.extend_from_slice(&(0xE000 | imm11).to_le_bytes());
                return Ok(());
            }
            let enc = encode_t32_b_bl(off as i32, false);
            out.extend_from_slice(&enc.0.to_le_bytes());
            out.extend_from_slice(&enc.1.to_le_bytes());
            return Ok(());
        }
    }

    if base == "BL" {
        let target_str = ops.first().ok_or_else(|| anyhow!("Missing BL target"))?;
        let target = if sizing_only {
            pc + 4
        } else {
            eval_branch_target(target_str, equs, symbols)?
        };
        let off = (target as i64) - ((pc + 4) as i64);
        let enc = encode_t32_b_bl(off as i32, true);
        out.extend_from_slice(&enc.0.to_le_bytes());
        out.extend_from_slice(&enc.1.to_le_bytes());
        return Ok(());
    }

    if base == "BX" || base == "BLX" {
        let rm = parse_reg(ops.first().ok_or_else(|| anyhow!("Missing register"))?)?;
        let bit7 = if base == "BLX" { 1u16 << 7 } else { 0 };
        out.extend_from_slice(&(0x4700 | bit7 | ((rm as u16) << 3)).to_le_bytes());
        return Ok(());
    }

    if base == "CBZ" || base == "CBNZ" {
        if ops.len() < 2 {
            return Err(anyhow!("CBZ/CBNZ expects Rn, label"));
        }
        let rn = parse_reg(&ops[0])?;
        if rn > 7 {
            return Err(anyhow!("CBZ/CBNZ only supports R0-R7"));
        }
        let target = if sizing_only { pc + 4 } else { eval_branch_target(&ops[1], equs, symbols)? };
        let off = (target as i64) - ((pc + 4) as i64);
        if !(0..=126).contains(&off) {
            return Err(anyhow!("CBZ/CBNZ offset {} out of range (0..126)", off));
        }
        let nz = if base == "CBNZ" { 1u16 << 11 } else { 0 };
        let imm6 = (off >> 1) as u16;
        let i = (imm6 >> 5) & 1;
        let imm5 = imm6 & 0x1F;
        out.extend_from_slice(&(0xB100 | nz | (i << 9) | (imm5 << 3) | (rn as u16)).to_le_bytes());
        return Ok(());
    }

    if base == "PUSH" || base == "POP" {
        let is_pop = base == "POP";
        let list_str = ops.join(",");
        let mask = parse_reg_list(&list_str)?;
        let extra_bit = if is_pop { 1u16 << 15 } else { 1u16 << 14 };
        if (mask & !0x00FF & !extra_bit) == 0 {
            let m = if (mask & extra_bit) != 0 { 1u16 << 8 } else { 0 };
            let base_op = if is_pop { 0xBC00 } else { 0xB400 };
            out.extend_from_slice(&(base_op | m | (mask & 0xFF)).to_le_bytes());
        } else {
            let hw1 = if is_pop { 0xE8BDu16 } else { 0xE92Du16 };
            out.extend_from_slice(&hw1.to_le_bytes());
            out.extend_from_slice(&mask.to_le_bytes());
        }
        return Ok(());
    }

    if base == "STM" || base == "STMIA" || base == "LDM" || base == "LDMIA" || base == "STMDB" || base == "LDMDB" {
        if ops.len() < 2 {
            return Err(anyhow!("{} expects Rn(!), {{regs}}", base));
        }
        let rn_raw = ops[0].trim();
        let wback = rn_raw.ends_with('!');
        let rn = parse_reg(rn_raw.trim_end_matches('!'))?;
        let mask = parse_reg_list(&ops[1..].join(","))?;
        let is_load = base.starts_with("LDM");
        let is_db = base.ends_with("DB");
        if !is_db && rn < 8 && (mask & !0xFF) == 0 {
            let op16 = if is_load { 0xC800 } else { 0xC000 };
            out.extend_from_slice(&(op16 | ((rn as u16) << 8) | (mask & 0xFF)).to_le_bytes());
        } else {
            let op_bits = if is_db { 2u16 << 7 } else { 1u16 << 7 };
            let l_bit = if is_load { 1u16 << 4 } else { 0 };
            let w_bit = if wback { 1u16 << 5 } else { 0 };
            let hw1 = 0xE800 | op_bits | w_bit | l_bit | (rn as u16);
            out.extend_from_slice(&hw1.to_le_bytes());
            out.extend_from_slice(&mask.to_le_bytes());
        }
        return Ok(());
    }

    // Load / Store single
    if matches!(base.as_str(), "LDR" | "LDRB" | "LDRSB" | "LDRH" | "LDRSH" | "STR" | "STRB" | "STRH") {
        return encode_ldst(pc, &base, ops, equs, symbols, literals, lit_cursor, out, sizing_only);
    }

    // ADR (default 32-bit T3 so PC-mod-4 parity never oscillates across layout passes; ADR.N for 16-bit)
    if base == "ADR" {
        if ops.len() < 2 {
            return Err(anyhow!("ADR expects Rd, label"));
        }
        let rd = parse_reg(&ops[0])?;
        let target = if sizing_only { pc + 4 } else { eval_branch_target(&ops[1], equs, symbols)? };
        let base_pc = (pc + 4) & !3;
        let off = (target as i64) - (base_pc as i64);
        if raw_mnem.eq_ignore_ascii_case("ADR.N") {
            if rd < 8 && off >= 0 && (off & 3) == 0 && off <= 1020 {
                out.extend_from_slice(&(0xA000 | ((rd as u16) << 8) | ((off >> 2) as u16)).to_le_bytes());
                return Ok(());
            }
            return Err(anyhow!("ADR.N offset {} invalid or unaligned", off));
        }
        if off >= 0 && off <= 4095 {
            let imm12 = off as u16;
            let (i, imm3, imm8) = split_imm12(imm12);
            out.extend_from_slice(&(0xF20F | (i << 10)).to_le_bytes());
            out.extend_from_slice(&((imm3 << 12) | ((rd as u16) << 8) | imm8).to_le_bytes());
        } else if off < 0 && (-off) <= 4095 {
            let imm12 = (-off) as u16;
            let (i, imm3, imm8) = split_imm12(imm12);
            out.extend_from_slice(&(0xF2AF | (i << 10)).to_le_bytes());
            out.extend_from_slice(&((imm3 << 12) | ((rd as u16) << 8) | imm8).to_le_bytes());
        } else {
            return Err(anyhow!("ADR offset {} out of range", off));
        }
        return Ok(());
    }

    // MOV / MOVW / MOVT / MVN
    if matches!(base.as_str(), "MOV" | "MOVW" | "MOVT" | "MVN") {
        return encode_mov(&base, setflags, wide, ops, equs, symbols, out);
    }

    // Shifts (LSL, LSR, ASR, ROR, RRX)
    if matches!(base.as_str(), "LSL" | "LSR" | "ASR" | "ROR" | "RRX") {
        return encode_shift(&base, setflags, wide, ops, equs, symbols, out);
    }

    // Multiply & Divide
    if matches!(base.as_str(), "MUL" | "MLA" | "MLS" | "UDIV" | "SDIV" | "UMULL" | "SMULL") {
        return encode_mul_div(&base, setflags, ops, out);
    }

    // Extend / Bit / Byte reverse
    if matches!(base.as_str(), "UXTB" | "UXTH" | "SXTB" | "SXTH" | "CLZ" | "RBIT" | "REV" | "REV16" | "REVSH") {
        return encode_misc_dp(&base, ops, out);
    }

    // Data processing (ADD, SUB, ADC, SBC, RSB, NEG, AND, ORR, ORN, EOR, BIC, CMP, CMN, TST, TEQ)
    encode_dp(&base, setflags, wide, ops, equs, symbols, out)
}

fn encode_mov(
    base: &str,
    setflags: bool,
    wide: bool,
    ops: &[String],
    equs: &HashMap<String, i64>,
    symbols: &HashMap<String, SymEntry>,
    out: &mut Vec<u8>,
) -> Result<()> {
    if ops.len() < 2 {
        return Err(anyhow!("{} expects Rd, src", base));
    }
    let rd = parse_reg(&ops[0])?;
    let src = ops[1].trim();

    if base == "MOVW" || base == "MOVT" {
        let imm = eval_imm(src, equs, symbols)? as u32;
        if imm > 0xFFFF {
            return Err(anyhow!("{} immediate 0x{:X} > 0xFFFF", base, imm));
        }
        let imm4 = ((imm >> 12) & 0xF) as u16;
        let i = ((imm >> 11) & 1) as u16;
        let imm3 = ((imm >> 8) & 0x7) as u16;
        let imm8 = (imm & 0xFF) as u16;
        let op_top = if base == "MOVT" { 0xF2C0 } else { 0xF240 };
        out.extend_from_slice(&(op_top | (i << 10) | imm4).to_le_bytes());
        out.extend_from_slice(&((imm3 << 12) | ((rd as u16) << 8) | imm8).to_le_bytes());
        return Ok(());
    }

    if is_imm_syntax(src) {
        let imm = eval_imm(src, equs, symbols)? as u32;
        if base == "MOV" {
            if !wide && rd < 8 && imm <= 0xFF && (setflags || true) {
                // Note: Outside IT, MOV Rd, #0..255 with Rd<8 uses 16-bit MOVS when setflags OR compact code unless MOV.W requested
                if setflags {
                    out.extend_from_slice(&(0x2000 | ((rd as u16) << 8) | (imm as u16)).to_le_bytes());
                    return Ok(());
                }
            }
            if let Some(imm12) = encode_thumb_expand_imm(imm) {
                let (i, imm3, imm8) = split_imm12(imm12);
                let s_bit = if setflags { 1u16 << 4 } else { 0 };
                out.extend_from_slice(&(0xF04F | (i << 10) | s_bit).to_le_bytes());
                out.extend_from_slice(&((imm3 << 12) | ((rd as u16) << 8) | imm8).to_le_bytes());
                return Ok(());
            }
            if imm <= 0xFFFF && !setflags {
                let imm4 = ((imm >> 12) & 0xF) as u16;
                let i = ((imm >> 11) & 1) as u16;
                let imm3 = ((imm >> 8) & 0x7) as u16;
                let imm8 = (imm & 0xFF) as u16;
                out.extend_from_slice(&(0xF240 | (i << 10) | imm4).to_le_bytes());
                out.extend_from_slice(&((imm3 << 12) | ((rd as u16) << 8) | imm8).to_le_bytes());
                return Ok(());
            }
            if rd < 8 && imm <= 0xFF {
                out.extend_from_slice(&(0x2000 | ((rd as u16) << 8) | (imm as u16)).to_le_bytes());
                return Ok(());
            }
            return Err(anyhow!("Constant 0x{:X} cannot be encoded in MOV; use LDR {}, =0x{:X} or MOVW/MOVT", imm, ops[0], imm));
        } else {
            // MVN
            if let Some(imm12) = encode_thumb_expand_imm(imm) {
                let (i, imm3, imm8) = split_imm12(imm12);
                let s_bit = if setflags { 1u16 << 4 } else { 0 };
                out.extend_from_slice(&(0xF06F | (i << 10) | s_bit).to_le_bytes());
                out.extend_from_slice(&((imm3 << 12) | ((rd as u16) << 8) | imm8).to_le_bytes());
                return Ok(());
            }
            return Err(anyhow!("Constant 0x{:X} cannot be encoded in MVN", imm));
        }
    }

    let rm = parse_reg(src)?;
    if base == "MOV" {
        if setflags && rd < 8 && rm < 8 {
            out.extend_from_slice(&(((rm as u16) << 3) | (rd as u16)).to_le_bytes());
        } else if !wide && !setflags {
            let d = ((rd as u16) >> 3) & 1;
            out.extend_from_slice(&(0x4600 | (d << 7) | ((rm as u16) << 3) | ((rd as u16) & 7)).to_le_bytes());
        } else {
            let s_bit = if setflags { 1u16 << 4 } else { 0 };
            out.extend_from_slice(&(0xEA4F | s_bit).to_le_bytes());
            out.extend_from_slice(&(((rd as u16) << 8) | (rm as u16)).to_le_bytes());
        }
    } else {
        if rd < 8 && rm < 8 && setflags {
            out.extend_from_slice(&(0x43C0 | ((rm as u16) << 3) | (rd as u16)).to_le_bytes());
        } else {
            let s_bit = if setflags { 1u16 << 4 } else { 0 };
            out.extend_from_slice(&(0xEA6F | s_bit).to_le_bytes());
            out.extend_from_slice(&(((rd as u16) << 8) | (rm as u16)).to_le_bytes());
        }
    }
    Ok(())
}

fn encode_shift(
    base: &str,
    setflags: bool,
    wide: bool,
    ops: &[String],
    equs: &HashMap<String, i64>,
    symbols: &HashMap<String, SymEntry>,
    out: &mut Vec<u8>,
) -> Result<()> {
    let rd = parse_reg(ops.first().ok_or_else(|| anyhow!("Missing Rd"))?)?;
    if base == "RRX" {
        let rm = parse_reg(ops.get(1).ok_or_else(|| anyhow!("Missing Rm"))?)?;
        let s_bit = if setflags { 1u16 << 4 } else { 0 };
        out.extend_from_slice(&(0xEA4F | s_bit).to_le_bytes());
        out.extend_from_slice(&(((rd as u16) << 8) | 0x0030 | (rm as u16)).to_le_bytes());
        return Ok(());
    }
    let (rn, sh_op) = if ops.len() >= 3 {
        (parse_reg(&ops[1])?, ops[2].trim())
    } else if ops.len() == 2 {
        (rd, ops[1].trim())
    } else {
        return Err(anyhow!("{} expects Rd, Rn, shift", base));
    };

    let stype: u16 = match base {
        "LSL" => 0,
        "LSR" => 1,
        "ASR" => 2,
        "ROR" => 3,
        _ => 0,
    };

    if is_imm_syntax(sh_op) {
        let amt = eval_imm(sh_op, equs, symbols)? as u32;
        if stype < 3 && !wide && setflags && rd < 8 && rn < 8 && amt <= 32 {
            let imm5 = if amt == 32 && stype > 0 { 0 } else { (amt & 0x1F) as u16 };
            out.extend_from_slice(&((stype << 11) | (imm5 << 6) | ((rn as u16) << 3) | (rd as u16)).to_le_bytes());
            return Ok(());
        }
        let imm5 = if amt == 32 && (stype == 1 || stype == 2) { 0u16 } else { (amt & 0x1F) as u16 };
        let imm3 = (imm5 >> 2) & 0x7;
        let imm2 = imm5 & 0x3;
        let s_bit = if setflags { 1u16 << 4 } else { 0 };
        out.extend_from_slice(&(0xEA4F | s_bit).to_le_bytes());
        out.extend_from_slice(&((imm3 << 12) | ((rd as u16) << 8) | (imm2 << 6) | (stype << 4) | (rn as u16)).to_le_bytes());
        return Ok(());
    }

    let rm = parse_reg(sh_op)?;
    if !wide && setflags && rd == rn && rd < 8 && rm < 8 {
        let alu = match stype { 0 => 2, 1 => 3, 2 => 4, _ => 7 };
        out.extend_from_slice(&(0x4000 | (alu << 6) | ((rm as u16) << 3) | (rd as u16)).to_le_bytes());
    } else {
        let s_bit = if setflags { 1u16 << 4 } else { 0 };
        out.extend_from_slice(&(0xFA00 | (stype << 5) | s_bit | (rn as u16)).to_le_bytes());
        out.extend_from_slice(&(0xF000 | ((rd as u16) << 8) | (rm as u16)).to_le_bytes());
    }
    Ok(())
}

fn encode_mul_div(base: &str, setflags: bool, ops: &[String], out: &mut Vec<u8>) -> Result<()> {
    match base {
        "MUL" => {
            let (rd, rn, rm) = if ops.len() >= 3 {
                (parse_reg(&ops[0])?, parse_reg(&ops[1])?, parse_reg(&ops[2])?)
            } else {
                let r = parse_reg(&ops[0])?;
                (r, parse_reg(&ops[1])?, r)
            };
            if setflags && rd < 8 && rn < 8 && (rd == rn || rd == rm) {
                let other = if rd == rn { rm } else { rn };
                out.extend_from_slice(&(0x4340 | ((other as u16) << 3) | (rd as u16)).to_le_bytes());
            } else {
                out.extend_from_slice(&(0xFB00 | (rn as u16)).to_le_bytes());
                out.extend_from_slice(&(0xF000 | ((rd as u16) << 8) | (rm as u16)).to_le_bytes());
            }
        }
        "MLA" | "MLS" => {
            if ops.len() < 4 { return Err(anyhow!("{} expects Rd, Rn, Rm, Ra", base)); }
            let rd = parse_reg(&ops[0])?;
            let rn = parse_reg(&ops[1])?;
            let rm = parse_reg(&ops[2])?;
            let ra = parse_reg(&ops[3])?;
            let op2 = if base == "MLS" { 0x10 } else { 0x00 };
            out.extend_from_slice(&(0xFB00 | (rn as u16)).to_le_bytes());
            out.extend_from_slice(&(((ra as u16) << 12) | ((rd as u16) << 8) | op2 | (rm as u16)).to_le_bytes());
        }
        "SDIV" | "UDIV" => {
            let (rd, rn, rm) = if ops.len() >= 3 {
                (parse_reg(&ops[0])?, parse_reg(&ops[1])?, parse_reg(&ops[2])?)
            } else {
                let r = parse_reg(&ops[0])?;
                (r, r, parse_reg(&ops[1])?)
            };
            let op1 = if base == "UDIV" { 0xFBB0 } else { 0xFB90 };
            out.extend_from_slice(&(op1 | (rn as u16)).to_le_bytes());
            out.extend_from_slice(&(0xF0F0 | ((rd as u16) << 8) | (rm as u16)).to_le_bytes());
        }
        "SMULL" | "UMULL" => {
            if ops.len() < 4 { return Err(anyhow!("{} expects RdLo, RdHi, Rn, Rm", base)); }
            let rdlo = parse_reg(&ops[0])?;
            let rdhi = parse_reg(&ops[1])?;
            let rn = parse_reg(&ops[2])?;
            let rm = parse_reg(&ops[3])?;
            let op1 = if base == "UMULL" { 0xFBA0 } else { 0xFB80 };
            out.extend_from_slice(&(op1 | (rn as u16)).to_le_bytes());
            out.extend_from_slice(&(((rdlo as u16) << 12) | ((rdhi as u16) << 8) | (rm as u16)).to_le_bytes());
        }
        _ => {}
    }
    Ok(())
}

fn encode_misc_dp(base: &str, ops: &[String], out: &mut Vec<u8>) -> Result<()> {
    if ops.len() < 2 { return Err(anyhow!("{} expects Rd, Rm", base)); }
    let rd = parse_reg(&ops[0])?;
    let rm = parse_reg(&ops[1])?;
    if rd < 8 && rm < 8 {
        match base {
            "SXTH" => { out.extend_from_slice(&(0xB200 | ((rm as u16) << 3) | (rd as u16)).to_le_bytes()); return Ok(()); }
            "SXTB" => { out.extend_from_slice(&(0xB240 | ((rm as u16) << 3) | (rd as u16)).to_le_bytes()); return Ok(()); }
            "UXTH" => { out.extend_from_slice(&(0xB280 | ((rm as u16) << 3) | (rd as u16)).to_le_bytes()); return Ok(()); }
            "UXTB" => { out.extend_from_slice(&(0xB2C0 | ((rm as u16) << 3) | (rd as u16)).to_le_bytes()); return Ok(()); }
            "REV"  => { out.extend_from_slice(&(0xBA00 | ((rm as u16) << 3) | (rd as u16)).to_le_bytes()); return Ok(()); }
            "REV16"=> { out.extend_from_slice(&(0xBA40 | ((rm as u16) << 3) | (rd as u16)).to_le_bytes()); return Ok(()); }
            "REVSH"=> { out.extend_from_slice(&(0xBAC0 | ((rm as u16) << 3) | (rd as u16)).to_le_bytes()); return Ok(()); }
            _ => {}
        }
    }
    match base {
        "CLZ" => {
            out.extend_from_slice(&(0xFAB0 | (rm as u16)).to_le_bytes());
            out.extend_from_slice(&(0xF080 | ((rd as u16) << 8) | (rm as u16)).to_le_bytes());
        }
        "RBIT" => {
            out.extend_from_slice(&(0xFA90 | (rm as u16)).to_le_bytes());
            out.extend_from_slice(&(0xF0A0 | ((rd as u16) << 8) | (rm as u16)).to_le_bytes());
        }
        "SXTH" | "UXTH" | "SXTB" | "UXTB" => {
            let op: u16 = match base { "SXTH" => 0xFA0F, "UXTH" => 0xFA1F, "SXTB" => 0xFA4F, _ => 0xFA5F };
            out.extend_from_slice(&op.to_le_bytes());
            out.extend_from_slice(&(0xF080 | ((rd as u16) << 8) | (rm as u16)).to_le_bytes());
        }
        _ => {
            let op2 = match base { "REV" => 0x80, "REV16" => 0x90, _ => 0xB0 };
            out.extend_from_slice(&(0xFA90 | (rm as u16)).to_le_bytes());
            out.extend_from_slice(&(0xF000 | ((rd as u16) << 8) | op2 | (rm as u16)).to_le_bytes());
        }
    }
    Ok(())
}

fn encode_dp(
    base: &str,
    setflags: bool,
    wide: bool,
    ops: &[String],
    equs: &HashMap<String, i64>,
    symbols: &HashMap<String, SymEntry>,
    out: &mut Vec<u8>,
) -> Result<()> {
    let is_cmp_style = matches!(base, "CMP" | "CMN" | "TST" | "TEQ");
    let (rd, rn, rhs, extra_shift) = if is_cmp_style {
        if ops.len() < 2 { return Err(anyhow!("{} expects Rn, op2", base)); }
        let rn = parse_reg(&ops[0])?;
        let sh = ops.get(2).map(|s| s.as_str());
        (15usize, rn, ops[1].trim(), sh)
    } else if base == "NEG" {
        if ops.len() < 2 { return Err(anyhow!("NEG expects Rd, Rm")); }
        let rd = parse_reg(&ops[0])?;
        let rm = parse_reg(&ops[1])?;
        if rd < 8 && rm < 8 && setflags {
            out.extend_from_slice(&(0x4240 | ((rm as u16) << 3) | (rd as u16)).to_le_bytes());
        } else {
            let s_bit = if setflags { 1u16 << 4 } else { 0 };
            out.extend_from_slice(&(0xF1C0 | s_bit | (rm as u16)).to_le_bytes());
            out.extend_from_slice(&(((rd as u16) << 8)).to_le_bytes());
        }
        return Ok(());
    } else if ops.len() == 2 {
        let rd = parse_reg(&ops[0])?;
        (rd, rd, ops[1].trim(), None)
    } else if ops.len() >= 3 {
        let rd = parse_reg(&ops[0])?;
        let rn = parse_reg(&ops[1])?;
        let sh = ops.get(3).map(|s| s.as_str());
        (rd, rn, ops[2].trim(), sh)
    } else {
        return Err(anyhow!("{} expects at least 2 operands", base));
    };

    let op_code: u16 = match base {
        "AND" | "TST" => 0x0,
        "BIC" => 0x1,
        "ORR" => 0x2,
        "ORN" => 0x3,
        "EOR" | "TEQ" => 0x4,
        "ADD" | "CMN" => 0x8,
        "ADC" => 0xA,
        "SBC" => 0xB,
        "SUB" | "CMP" => 0xD,
        "RSB" => 0xE,
        _ => return Err(anyhow!("Unsupported instruction '{}'", base)),
    };

    if is_imm_syntax(rhs) && extra_shift.is_none() {
        let imm = eval_imm(rhs, equs, symbols)? as u32;

        // Compact 16-bit forms
        if !wide {
            if base == "CMP" && rn < 8 && imm <= 0xFF {
                out.extend_from_slice(&(0x2800 | ((rn as u16) << 8) | (imm as u16)).to_le_bytes());
                return Ok(());
            }
            if (base == "ADD" || base == "SUB") && rd == 13 && rn == 13 && (imm & 3) == 0 && imm <= 508 {
                let neg = if base == "SUB" { 1u16 << 7 } else { 0 };
                out.extend_from_slice(&(0xB000 | neg | ((imm >> 2) as u16)).to_le_bytes());
                return Ok(());
            }
            if base == "ADD" && rd < 8 && rn == 13 && (imm & 3) == 0 && imm <= 1020 {
                out.extend_from_slice(&(0xA800 | ((rd as u16) << 8) | ((imm >> 2) as u16)).to_le_bytes());
                return Ok(());
            }
            if (base == "ADD" || base == "SUB") && setflags && rd < 8 && rn < 8 {
                if imm <= 7 {
                    let sub_bit = if base == "SUB" { 1u16 << 9 } else { 0 };
                    out.extend_from_slice(&(0x1C00 | sub_bit | ((imm as u16) << 6) | ((rn as u16) << 3) | (rd as u16)).to_le_bytes());
                    return Ok(());
                }
                if rd == rn && imm <= 0xFF {
                    let top = if base == "ADD" { 0x3000 } else { 0x3800 };
                    out.extend_from_slice(&(top | ((rd as u16) << 8) | (imm as u16)).to_le_bytes());
                    return Ok(());
                }
            }
        }

        // Thumb-2 modified immediate
        if let Some(imm12) = encode_thumb_expand_imm(imm) {
            let (i, imm3, imm8) = split_imm12(imm12);
            let s_bit = if setflags || is_cmp_style { 1u16 << 4 } else { 0 };
            out.extend_from_slice(&(0xF000 | (i << 10) | (op_code << 5) | s_bit | (rn as u16)).to_le_bytes());
            out.extend_from_slice(&((imm3 << 12) | ((rd as u16) << 8) | imm8).to_le_bytes());
            return Ok(());
        }

        // ADDW / SUBW (12-bit plain immediate when flags not updated)
        if (base == "ADD" || base == "SUB") && !setflags && imm <= 4095 {
            let (i, imm3, imm8) = split_imm12(imm as u16);
            let op_top = if base == "ADD" { 0xF200 } else { 0xF2A0 };
            out.extend_from_slice(&(op_top | (i << 10) | (rn as u16)).to_le_bytes());
            out.extend_from_slice(&((imm3 << 12) | ((rd as u16) << 8) | imm8).to_le_bytes());
            return Ok(());
        }

        return Err(anyhow!("Immediate 0x{:X} cannot be encoded for {}", imm, base));
    }

    // Register (+ optional shift)
    let rm = parse_reg(rhs)?;
    let (stype, sh_amt) = if let Some(sh) = extra_shift {
        parse_inline_shift(sh, equs, symbols)?
    } else {
        (0u16, 0u16)
    };

    if !wide && extra_shift.is_none() {
        if base == "CMP" && (rn >= 8 || rm >= 8) {
            let n = ((rn as u16) >> 3) & 1;
            out.extend_from_slice(&(0x4500 | (n << 7) | ((rm as u16) << 3) | ((rn as u16) & 7)).to_le_bytes());
            return Ok(());
        }
        if base == "ADD" && !setflags && (rd >= 8 || rn >= 8 || rm >= 8) && rd == rn {
            let d = ((rd as u16) >> 3) & 1;
            out.extend_from_slice(&(0x4400 | (d << 7) | ((rm as u16) << 3) | ((rd as u16) & 7)).to_le_bytes());
            return Ok(());
        }
        if setflags && (base == "ADD" || base == "SUB") && rd < 8 && rn < 8 && rm < 8 {
            let sub_bit = if base == "SUB" { 1u16 << 9 } else { 0 };
            out.extend_from_slice(&(0x1800 | sub_bit | ((rm as u16) << 6) | ((rn as u16) << 3) | (rd as u16)).to_le_bytes());
            return Ok(());
        }
        if is_cmp_style && rn < 8 && rm < 8 {
            let alu = match base { "TST" => 8, "CMP" => 10, _ => 11 };
            out.extend_from_slice(&(0x4000 | (alu << 6) | ((rm as u16) << 3) | (rn as u16)).to_le_bytes());
            return Ok(());
        }
        if setflags && rd == rn && rd < 8 && rm < 8 {
            let alu = match base {
                "AND" => Some(0), "EOR" => Some(1), "ADC" => Some(5),
                "SBC" => Some(6), "ORR" => Some(12), "BIC" => Some(14),
                _ => None,
            };
            if let Some(a) = alu {
                out.extend_from_slice(&(0x4000 | (a << 6) | ((rm as u16) << 3) | (rd as u16)).to_le_bytes());
                return Ok(());
            }
        }
    }

    let imm3 = (sh_amt >> 2) & 0x7;
    let imm2 = sh_amt & 0x3;
    let s_bit = if setflags || is_cmp_style { 1u16 << 4 } else { 0 };
    out.extend_from_slice(&(0xEA00 | (op_code << 5) | s_bit | (rn as u16)).to_le_bytes());
    out.extend_from_slice(&((imm3 << 12) | ((rd as u16) << 8) | (imm2 << 6) | (stype << 4) | (rm as u16)).to_le_bytes());
    Ok(())
}

fn encode_ldst(
    pc: u32,
    base: &str,
    ops: &[String],
    equs: &HashMap<String, i64>,
    symbols: &HashMap<String, SymEntry>,
    literals: &[LiteralEntry],
    lit_cursor: &mut usize,
    out: &mut Vec<u8>,
    sizing_only: bool,
) -> Result<()> {
    if ops.len() < 2 {
        return Err(anyhow!("{} expects Rt, [address]", base));
    }
    let rt = parse_reg(&ops[0])?;
    let rest = ops[1..].join(",").trim().to_string();

    // Literal pool: LDR Rt, =expr
    if base == "LDR" && rest.starts_with('=') {
        let lit_addr = literals
            .get(*lit_cursor)
            .map(|l| l.addr)
            .unwrap_or(pc + 4);
        if !sizing_only {
            *lit_cursor += 1;
        }
        let base_pc = (pc + 4) & !3;
        let off = (lit_addr as i64) - (base_pc as i64);
        let u_bit = if off >= 0 { 1u16 << 7 } else { 0 };
        let abs_off = off.unsigned_abs() as u16 & 0xFFF;
        out.extend_from_slice(&(0xF85F | u_bit).to_le_bytes());
        out.extend_from_slice(&(((rt as u16) << 12) | abs_off).to_le_bytes());
        return Ok(());
    }

    // Label PC-relative: LDR Rt, label
    if !rest.starts_with('[') {
        let target = if sizing_only { pc + 4 } else { eval_branch_target(&rest, equs, symbols)? };
        let base_pc = (pc + 4) & !3;
        let off = (target as i64) - (base_pc as i64);
        if off >= 0 && (off & 3) == 0 && off <= 1020 && rt < 8 && base == "LDR" {
            out.extend_from_slice(&(0x4800 | ((rt as u16) << 8) | ((off >> 2) as u16)).to_le_bytes());
            return Ok(());
        }
        let u_bit = if off >= 0 { 1u16 << 7 } else { 0 };
        let abs_off = off.unsigned_abs() as u16 & 0xFFF;
        out.extend_from_slice(&(0xF85F | u_bit).to_le_bytes());
        out.extend_from_slice(&(((rt as u16) << 12) | abs_off).to_le_bytes());
        return Ok(());
    }

    // Parse [Rn, ...] (+ optional post-index , #imm)
    let is_load = base.starts_with("LDR");
    let is_signed = base.contains('S');
    let size_bits: u16 = if base.ends_with('B') { 0 } else if base.ends_with('H') { 1 } else { 2 };

    let close_br = rest.find(']').ok_or_else(|| anyhow!("Missing ']' in {}", rest))?;
    let inside = rest[1..close_br].trim();
    let after = rest[close_br + 1..].trim();
    let pre_wb = after.starts_with('!');
    let post_imm_str = if after.starts_with(',') { Some(after[1..].trim()) } else { None };

    let inner_parts = split_operands(inside);
    let rn = parse_reg(&inner_parts[0])?;

    if let Some(pimm_s) = post_imm_str {
        // Post-indexed: [Rn], #imm
        let imm = eval_imm(pimm_s, equs, symbols)?;
        let u_bit = if imm >= 0 { 1u16 << 9 } else { 0 };
        let abs = imm.unsigned_abs() as u16 & 0xFF;
        let l_bit = if is_load { 1u16 << 4 } else { 0 };
        let s_bit = if is_signed { 1u16 << 8 } else { 0 };
        let hw1 = 0xF800 | s_bit | (size_bits << 5) | l_bit | (rn as u16);
        let hw2 = ((rt as u16) << 12) | 0x0900 | u_bit | abs; // P=0, W=1
        out.extend_from_slice(&hw1.to_le_bytes());
        out.extend_from_slice(&hw2.to_le_bytes());
        return Ok(());
    }

    if inner_parts.len() == 1 || is_imm_syntax(&inner_parts[1]) {
        let imm = if inner_parts.len() >= 2 {
            eval_imm(&inner_parts[1], equs, symbols)?
        } else {
            0
        };

        // Compact 16-bit forms when no writeback and non-negative
        if !pre_wb && imm >= 0 && !is_signed {
            if size_bits == 2 && rn == 13 && rt < 8 && (imm & 3) == 0 && imm <= 1020 {
                let op = if is_load { 0x9800 } else { 0x9000 };
                out.extend_from_slice(&(op | ((rt as u16) << 8) | ((imm >> 2) as u16)).to_le_bytes());
                return Ok(());
            }
            if rt < 8 && rn < 8 {
                let shift = size_bits as i64;
                let max_imm = 31i64 << shift;
                if (imm & ((1 << shift) - 1)) == 0 && imm <= max_imm {
                    let top = match (size_bits, is_load) {
                        (2, false) => 0x6000,
                        (2, true)  => 0x6800,
                        (0, false) => 0x7000,
                        (0, true)  => 0x7800,
                        (1, false) => 0x8000,
                        _          => 0x8800,
                    };
                    out.extend_from_slice(&(top | (((imm >> shift) as u16) << 6) | ((rn as u16) << 3) | (rt as u16)).to_le_bytes());
                    return Ok(());
                }
            }
        }

        let l_bit = if is_load { 1u16 << 4 } else { 0 };
        let s_bit = if is_signed { 1u16 << 8 } else { 0 };
        if !pre_wb && imm >= 0 && imm <= 4095 {
            let hw1 = 0xF880 | s_bit | (size_bits << 5) | l_bit | (rn as u16);
            let hw2 = ((rt as u16) << 12) | (imm as u16);
            out.extend_from_slice(&hw1.to_le_bytes());
            out.extend_from_slice(&hw2.to_le_bytes());
        } else {
            let u_bit = if imm >= 0 { 1u16 << 9 } else { 0 };
            let w_bit = if pre_wb { 1u16 << 8 } else { 0 };
            let abs = imm.unsigned_abs() as u16 & 0xFF;
            let hw1 = 0xF800 | s_bit | (size_bits << 5) | l_bit | (rn as u16);
            let hw2 = ((rt as u16) << 12) | 0x0C00 | u_bit | w_bit | abs;
            out.extend_from_slice(&hw1.to_le_bytes());
            out.extend_from_slice(&hw2.to_le_bytes());
        }
        return Ok(());
    }

    // Register offset: [Rn, Rm(, LSL #n)]
    let rm = parse_reg(&inner_parts[1])?;
    let shift = if inner_parts.len() >= 3 {
        let (_, a) = parse_inline_shift(&inner_parts[2], equs, symbols)?;
        a & 3
    } else {
        0
    };
    if shift == 0 && rt < 8 && rn < 8 && rm < 8 {
        let sub = match (is_load, size_bits, is_signed) {
            (false, 2, _) => 0,
            (false, 1, _) => 1,
            (false, 0, _) => 2,
            (true, 0, true) => 3,
            (true, 2, _) => 4,
            (true, 1, false) => 5,
            (true, 0, false) => 6,
            _ => 7,
        };
        out.extend_from_slice(&(0x5000 | (sub << 9) | ((rm as u16) << 6) | ((rn as u16) << 3) | (rt as u16)).to_le_bytes());
    } else {
        let l_bit = if is_load { 1u16 << 4 } else { 0 };
        let s_bit = if is_signed { 1u16 << 8 } else { 0 };
        let hw1 = 0xF800 | s_bit | (size_bits << 5) | l_bit | (rn as u16);
        let hw2 = ((rt as u16) << 12) | (shift << 4) | (rm as u16);
        out.extend_from_slice(&hw1.to_le_bytes());
        out.extend_from_slice(&hw2.to_le_bytes());
    }
    Ok(())
}

// ---- Encoding & Expression Helpers ----

fn parse_mnemonic(raw: &str) -> (String, bool, bool, Option<u8>) {
    let mut s = raw.to_ascii_uppercase();
    let mut wide = false;
    if let Some(stem) = s.strip_suffix(".W") {
        wide = true;
        s = stem.to_string();
    } else if let Some(stem) = s.strip_suffix(".N") {
        s = stem.to_string();
    }

    // Condition check on B<cond>
    if s.len() == 3 && s.starts_with('B') {
        if let Some(c) = parse_cond(&s[1..]) {
            return ("B".to_string(), false, wide, Some(c));
        }
    }

    // Check explicit 'S' suffix on ALU mnemonics
    let base_candidates = [
        "MOVS", "MVNS", "ADDS", "ADCS", "SUBS", "SBCS", "RSBS", "ANDS", "ORRS",
        "ORNS", "EORS", "BICS", "LSLS", "LSRS", "ASRS", "RORS", "MULS",
    ];
    if base_candidates.contains(&s.as_str()) {
        return (s[..s.len() - 1].to_string(), true, wide, None);
    }

    (s, false, wide, None)
}

fn parse_cond(c: &str) -> Option<u8> {
    match c {
        "EQ" => Some(0),
        "NE" => Some(1),
        "CS" | "HS" => Some(2),
        "CC" | "LO" => Some(3),
        "MI" => Some(4),
        "PL" => Some(5),
        "VS" => Some(6),
        "VC" => Some(7),
        "HI" => Some(8),
        "LS" => Some(9),
        "GE" => Some(10),
        "LT" => Some(11),
        "GT" => Some(12),
        "LE" => Some(13),
        "AL" => Some(14),
        _ => None,
    }
}

fn encode_t32_bcond(cond: u8, off: i32) -> (u16, u16) {
    let v = (off >> 1) as u32;
    let s = ((v >> 19) & 1) as u16;
    let j2 = ((v >> 18) & 1) as u16;
    let j1 = ((v >> 17) & 1) as u16;
    let imm6 = ((v >> 11) & 0x3F) as u16;
    let imm11 = (v & 0x7FF) as u16;
    let hw1 = 0xF000 | (s << 10) | ((cond as u16) << 6) | imm6;
    let hw2 = 0x8000 | (j1 << 13) | (j2 << 11) | imm11;
    (hw1, hw2)
}

fn encode_t32_b_bl(off: i32, is_bl: bool) -> (u16, u16) {
    let v = (off >> 1) as u32;
    let s = ((v >> 23) & 1) as u16;
    let i1 = ((v >> 22) & 1) as u16;
    let i2 = ((v >> 21) & 1) as u16;
    let j1 = (!(i1 ^ s)) & 1;
    let j2 = (!(i2 ^ s)) & 1;
    let imm10 = ((v >> 11) & 0x3FF) as u16;
    let imm11 = (v & 0x7FF) as u16;
    let hw1 = 0xF000 | (s << 10) | imm10;
    let bl_bit = if is_bl { 1u16 << 14 } else { 0 };
    let hw2 = 0x9000 | bl_bit | (j1 << 13) | (j2 << 11) | imm11;
    (hw1, hw2)
}

pub fn encode_thumb_expand_imm(val: u32) -> Option<u16> {
    if (val & 0xFFFF_FF00) == 0 {
        return Some(val as u16);
    }
    let b0 = val & 0xFF;
    if val == ((b0 << 16) | b0) {
        return Some(0x100 | (b0 as u16));
    }
    let b1 = (val >> 8) & 0xFF;
    if val == ((b1 << 24) | (b1 << 8)) {
        return Some(0x200 | (b1 as u16));
    }
    if val == ((b0 << 24) | (b0 << 16) | (b0 << 8) | b0) {
        return Some(0x300 | (b0 as u16));
    }
    for rot in 8..32u32 {
        let u = val.rotate_left(rot);
        if (u & !0xFF) == 0 && (u & 0x80) != 0 {
            let enc = ((rot as u16) << 7) | ((u & 0x7F) as u16);
            let (check, _) = thumb_expand_imm_c(enc as u32, false);
            if check == val {
                return Some(enc);
            }
        }
    }
    None
}

#[inline]
fn split_imm12(imm12: u16) -> (u16, u16, u16) {
    ((imm12 >> 11) & 1, (imm12 >> 8) & 7, imm12 & 0xFF)
}

fn parse_inline_shift(s: &str, equs: &HashMap<String, i64>, symbols: &HashMap<String, SymEntry>) -> Result<(u16, u16)> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.is_empty() {
        return Ok((0, 0));
    }
    let kind = parts[0].to_ascii_uppercase();
    let stype = match kind.as_str() {
        "LSL" => 0,
        "LSR" => 1,
        "ASR" => 2,
        "ROR" => 3,
        "RRX" => return Ok((3, 0)),
        _ => return Err(anyhow!("Unknown shift '{}'", s)),
    };
    let amt = if parts.len() >= 2 {
        eval_imm(parts[1], equs, symbols)? as u16
    } else {
        0
    };
    Ok((stype, amt))
}

fn parse_reg(s: &str) -> Result<usize> {
    match s.trim().to_ascii_uppercase().as_str() {
        "R0" | "A1" => Ok(0),
        "R1" | "A2" => Ok(1),
        "R2" | "A3" => Ok(2),
        "R3" | "A4" => Ok(3),
        "R4" | "V1" => Ok(4),
        "R5" | "V2" => Ok(5),
        "R6" | "V3" => Ok(6),
        "R7" | "V4" => Ok(7),
        "R8" | "V5" => Ok(8),
        "R9" | "V6" => Ok(9),
        "R10" | "V7" => Ok(10),
        "R11" | "V8" | "FP" => Ok(11),
        "R12" | "IP" => Ok(12),
        "R13" | "SP" => Ok(13),
        "R14" | "LR" => Ok(14),
        "R15" | "PC" => Ok(15),
        _ => Err(anyhow!("Unknown register '{}'", s.trim())),
    }
}

fn parse_reg_list(s: &str) -> Result<u16> {
    let trimmed = s.trim().trim_start_matches('{').trim_end_matches('}');
    let mut mask = 0u16;
    for tok in trimmed.split(',') {
        let t = tok.trim();
        if t.is_empty() { continue; }
        if let Some((a, b)) = t.split_once('-') {
            let r1 = parse_reg(a)?;
            let r2 = parse_reg(b)?;
            for r in r1..=r2 {
                mask |= 1u16 << r;
            }
        } else {
            let r = parse_reg(t)?;
            mask |= 1u16 << r;
        }
    }
    Ok(mask)
}

fn is_imm_syntax(s: &str) -> bool {
    let t = s.trim();
    t.starts_with('#') || t.chars().next().map(|c| c.is_ascii_digit() || c == '-').unwrap_or(false)
}

fn eval_imm(s: &str, equs: &HashMap<String, i64>, symbols: &HashMap<String, SymEntry>) -> Result<i64> {
    let t = s.trim().strip_prefix('#').unwrap_or(s.trim()).trim();
    eval_expr(t, equs, symbols)
}

fn eval_expr_for_data(s: &str, equs: &HashMap<String, i64>, symbols: &HashMap<String, SymEntry>) -> Result<u32> {
    let t = s.trim();
    if let Some(sym) = symbols.get(t) {
        return Ok(if sym.is_func { sym.addr | 1 } else { sym.addr });
    }
    Ok(eval_expr(t, equs, symbols)? as u32)
}

fn eval_branch_target(s: &str, equs: &HashMap<String, i64>, symbols: &HashMap<String, SymEntry>) -> Result<u32> {
    let t = s.trim();
    if let Some(sym) = symbols.get(t) {
        return Ok(sym.addr & !1);
    }
    // Case-insensitive lookup fallback
    if let Some((_, sym)) = symbols.iter().find(|(k, _)| k.eq_ignore_ascii_case(t)) {
        return Ok(sym.addr & !1);
    }
    Ok((eval_expr(t, equs, symbols)? as u32) & !1)
}

fn eval_expr(s: &str, equs: &HashMap<String, i64>, symbols: &HashMap<String, SymEntry>) -> Result<i64> {
    let t = s.trim();
    if t.is_empty() {
        return Err(anyhow!("Empty expression"));
    }
    // Simple bitwise/arithmetic evaluation (low to high precedence: |, &, << >>, + -, * /)
    for op in ["|", "^", "&", "<<", ">>", "+", "-"] {
        if let Some(pos) = find_top_level_op(t, op) {
            if pos == 0 && (op == "-" || op == "+") {
                let rhs = eval_expr(&t[1..], equs, symbols)?;
                return Ok(if op == "-" { -rhs } else { rhs });
            }
            let lhs = eval_expr(&t[..pos], equs, symbols)?;
            let rhs = eval_expr(&t[pos + op.len()..], equs, symbols)?;
            return Ok(match op {
                "|" => lhs | rhs,
                "^" => lhs ^ rhs,
                "&" => lhs & rhs,
                "<<" => lhs.wrapping_shl(rhs as u32),
                ">>" => lhs.wrapping_shr(rhs as u32),
                "+" => lhs.wrapping_add(rhs),
                "-" => lhs.wrapping_sub(rhs),
                _ => 0,
            });
        }
    }
    for op in ["*", "/"] {
        if let Some(pos) = find_top_level_op(t, op) {
            let lhs = eval_expr(&t[..pos], equs, symbols)?;
            let rhs = eval_expr(&t[pos + 1..], equs, symbols)?;
            return Ok(if op == "*" { lhs.wrapping_mul(rhs) } else if rhs != 0 { lhs / rhs } else { 0 });
        }
    }
    if t.starts_with('(') && t.ends_with(')') {
        return eval_expr(&t[1..t.len() - 1], equs, symbols);
    }
    if t.starts_with('\'') && t.ends_with('\'') && t.len() == 3 {
        return Ok(t.as_bytes()[1] as i64);
    }
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).or_else(|| t.strip_prefix('&')) {
        return u64::from_str_radix(hex, 16)
            .map(|v| v as i64)
            .map_err(|_| anyhow!("Invalid hex literal '{}'", t));
    }
    if let Some(bin) = t.strip_prefix("2_").or_else(|| t.strip_prefix("0b")).or_else(|| t.strip_prefix("0B")) {
        return u64::from_str_radix(bin, 2)
            .map(|v| v as i64)
            .map_err(|_| anyhow!("Invalid binary literal '{}'", t));
    }
    if let Ok(num) = t.parse::<i64>() {
        return Ok(num);
    }
    if let Some(&v) = equs.get(t) {
        return Ok(v);
    }
    if let Some(sym) = symbols.get(t) {
        return Ok((if sym.is_func { sym.addr | 1 } else { sym.addr }) as i64);
    }
    Err(anyhow!("Undefined symbol or invalid constant '{}'", t))
}

fn find_top_level_op(s: &str, op: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let op_b = op.as_bytes();
    let mut depth = 0i32;
    let mut i = bytes.len();
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b')' => depth += 1,
            b'(' => depth -= 1,
            _ if depth == 0 && i + op_b.len() <= bytes.len() && &bytes[i..i + op_b.len()] == op_b => {
                // Avoid splitting inside '0x...' or unary '-' after another operator
                if op == "-" && i > 0 && matches!(bytes[i - 1], b'+' | b'-' | b'*' | b'/' | b'<' | b'>' | b'|' | b'&' | b'(') {
                    continue;
                }
                return Some(i);
            }
            _ => {}
        }
    }
    None
}

fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut in_str = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' {
            in_str = !in_str;
        } else if !in_str {
            if b == b';' || (b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/') {
                return &line[..i];
            }
        }
        i += 1;
    }
    line
}

fn try_parse_equ(line: &str) -> Option<(&str, &str)> {
    let t = line.trim();
    let mut parts = t.split_whitespace();
    let name = parts.next()?;
    let kw = parts.next()?;
    if kw.eq_ignore_ascii_case("EQU") {
        let idx = t.to_ascii_uppercase().find("EQU")?;
        return Some((name, t[idx + 3..].trim()));
    }
    if let Some(rest) = t.strip_prefix(".equ").or_else(|| t.strip_prefix(".EQU")) {
        if let Some((n, v)) = rest.split_once(',') {
            return Some((n.trim(), v.trim()));
        }
    }
    None
}

fn find_label_colon(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut in_str = false;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'"' { in_str = !in_str; }
        else if !in_str {
            if b == b':' { return Some(i); }
            if b.is_ascii_whitespace() { break; }
        }
    }
    None
}

fn is_valid_ident(s: &str) -> bool {
    !s.is_empty()
        && s.chars().next().map(|c| c.is_ascii_alphabetic() || c == '_' || c == '.').unwrap_or(false)
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

fn split_first_word(s: &str) -> (&str, &str) {
    for (i, ch) in s.char_indices() {
        if ch.is_whitespace() {
            return (&s[..i], s[i..].trim());
        }
    }
    (s, "")
}

fn split_operands(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut bracket = 0i32;
    let mut brace = 0i32;
    let mut in_str = false;

    for ch in s.chars() {
        match ch {
            '"' => {
                in_str = !in_str;
                cur.push(ch);
            }
            '[' if !in_str => { bracket += 1; cur.push(ch); }
            ']' if !in_str => { bracket -= 1; cur.push(ch); }
            '{' if !in_str => { brace += 1; cur.push(ch); }
            '}' if !in_str => { brace -= 1; cur.push(ch); }
            ',' if !in_str && bracket == 0 && brace == 0 => {
                if !cur.trim().is_empty() {
                    out.push(cur.trim().to_string());
                }
                cur.clear();
            }
            _ => cur.push(ch),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

fn count_dcb_bytes(ops: &[String]) -> u32 {
    let mut n = 0u32;
    for op in ops {
        let t = op.trim();
        if t.starts_with('"') && t.ends_with('"') && t.len() >= 2 {
            n += (t.len() - 2) as u32;
        } else {
            n += 1;
        }
    }
    n
}

fn is_directive(up: &str) -> bool {
    matches!(
        up,
        "PRESERVE8" | "REQUIRE8" | "THUMB" | "ARM" | "AREA" | "EXPORT" | "IMPORT"
            | "EXTERN" | "ENTRY" | "END" | "ALIGN" | "LTORG" | "DCD" | "DCDU"
            | "DCW" | "DCWU" | "DCB" | "SPACE" | "FILL" | "EQU" | "NOP" | "WFI"
            | "WFE" | "SEV" | "DSB" | "DMB" | "ISB"
    ) || up.starts_with('.')
}

fn is_zero_operand_mnem(up: &str) -> bool {
    matches!(up, "NOP" | "WFI" | "WFE" | "SEV" | "DSB" | "DMB" | "ISB")
}

// ---- ELF32 + DWARF v2 + Intel HEX Emitters ----

fn build_elf32(
    entry: u32,
    ro_base: u32,
    ro_bytes: &[u8],
    rw_base: u32,
    rw_bytes: &[u8],
    symbols: &HashMap<String, SymEntry>,
    files: &[String],
    line_rows: &[EmittedLine],
) -> Vec<u8> {
    let debug_line = build_dwarf_v2_line(files, line_rows);

    let mut strtab = vec![0u8];
    let mut symtab = Vec::new();
    // Null ELF symbol
    symtab.extend_from_slice(&[0u8; 16]);

    let mut sorted_syms: Vec<&SymEntry> = symbols.values().collect();
    sorted_syms.sort_by_key(|s| (s.addr, &s.name));
    for s in sorted_syms {
        let name_off = strtab.len() as u32;
        strtab.extend_from_slice(s.name.as_bytes());
        strtab.push(0);
        let val = if s.is_func { s.addr | 1 } else { s.addr };
        let st_info: u8 = if s.is_func { (1 << 4) | 2 } else { (1 << 4) | 1 }; // STB_GLOBAL | STT_FUNC/OBJECT
        let st_shndx: u16 = if s.section == SectionKind::Data && !rw_bytes.is_empty() { 2 } else { 1 };
        symtab.extend_from_slice(&name_off.to_le_bytes());
        symtab.extend_from_slice(&val.to_le_bytes());
        symtab.extend_from_slice(&0u32.to_le_bytes());
        symtab.push(st_info);
        symtab.push(0);
        symtab.extend_from_slice(&st_shndx.to_le_bytes());
    }

    let debug_abbrev: [u8; 8] = [1, 0x11, 0, 0x10, 0x06, 0, 0, 0];
    let mut debug_info = Vec::with_capacity(16);
    debug_info.extend_from_slice(&12u32.to_le_bytes()); // unit_length
    debug_info.extend_from_slice(&2u16.to_le_bytes());  // version = 2
    debug_info.extend_from_slice(&0u32.to_le_bytes());  // debug_abbrev_offset = 0
    debug_info.push(4);                                  // address_size = 4
    debug_info.push(1);                                  // abbrev code = 1
    debug_info.extend_from_slice(&0u32.to_le_bytes());  // DW_AT_stmt_list = 0

    let mut shstrtab = vec![0u8];
    let sh_text = push_cstr(&mut shstrtab, ".text");
    let sh_data = if !rw_bytes.is_empty() { push_cstr(&mut shstrtab, ".data") } else { 0 };
    let sh_symtab = push_cstr(&mut shstrtab, ".symtab");
    let sh_strtab = push_cstr(&mut shstrtab, ".strtab");
    let sh_dline = push_cstr(&mut shstrtab, ".debug_line");
    let sh_dinfo = push_cstr(&mut shstrtab, ".debug_info");
    let sh_dabbrev = push_cstr(&mut shstrtab, ".debug_abbrev");
    let sh_shstrtab = push_cstr(&mut shstrtab, ".shstrtab");

    let phnum: u16 = if rw_bytes.is_empty() { 1 } else { 2 };
    let phoff: u32 = 52;
    let data_start: u32 = phoff + (phnum as u32) * 32;

    let text_off = align4(data_start);
    let mut cur = text_off + ro_bytes.len() as u32;

    let data_off = align4(cur);
    if !rw_bytes.is_empty() {
        cur = data_off + rw_bytes.len() as u32;
    }

    let sym_off = align4(cur);
    cur = sym_off + symtab.len() as u32;

    let str_off = align4(cur);
    cur = str_off + strtab.len() as u32;

    let dline_off = align4(cur);
    cur = dline_off + debug_line.len() as u32;

    let dinfo_off = align4(cur);
    cur = dinfo_off + debug_info.len() as u32;

    let dabbrev_off = align4(cur);
    cur = dabbrev_off + debug_abbrev.len() as u32;

    let shstr_off = align4(cur);
    cur = shstr_off + shstrtab.len() as u32;

    let shoff = align4(cur);
    let has_data = !rw_bytes.is_empty();
    let sym_idx: u16 = if has_data { 3 } else { 2 };
    let str_idx: u16 = sym_idx + 1;
    let dline_idx: u16 = str_idx + 1;
    let dinfo_idx: u16 = dline_idx + 1;
    let dabbrev_idx: u16 = dinfo_idx + 1;
    let shstr_idx: u16 = dabbrev_idx + 1;
    let shnum: u16 = shstr_idx + 1;

    let mut elf = Vec::with_capacity((shoff + (shnum as u32) * 40) as usize);
    // ELF32 Header (52 bytes)
    elf.extend_from_slice(b"\x7FELF\x01\x01\x01\x00");
    elf.extend_from_slice(&[0u8; 8]);
    elf.extend_from_slice(&2u16.to_le_bytes()); // ET_EXEC
    elf.extend_from_slice(&40u16.to_le_bytes()); // EM_ARM
    elf.extend_from_slice(&1u32.to_le_bytes()); // EV_CURRENT
    elf.extend_from_slice(&entry.to_le_bytes());
    elf.extend_from_slice(&phoff.to_le_bytes());
    elf.extend_from_slice(&shoff.to_le_bytes());
    elf.extend_from_slice(&0x0500_0200u32.to_le_bytes()); // EABI5 + HASENTRY
    elf.extend_from_slice(&52u16.to_le_bytes());
    elf.extend_from_slice(&32u16.to_le_bytes());
    elf.extend_from_slice(&phnum.to_le_bytes());
    elf.extend_from_slice(&40u16.to_le_bytes());
    elf.extend_from_slice(&shnum.to_le_bytes());
    elf.extend_from_slice(&shstr_idx.to_le_bytes());

    // Program header 0: Flash
    push_phdr(&mut elf, 1, text_off, ro_base, ro_bytes.len() as u32, ro_bytes.len() as u32, 5);
    if has_data {
        push_phdr(&mut elf, 1, data_off, rw_base, rw_bytes.len() as u32, rw_bytes.len() as u32, 6);
    }

    pad_to(&mut elf, text_off);
    elf.extend_from_slice(ro_bytes);
    if has_data {
        pad_to(&mut elf, data_off);
        elf.extend_from_slice(rw_bytes);
    }
    pad_to(&mut elf, sym_off);
    elf.extend_from_slice(&symtab);
    pad_to(&mut elf, str_off);
    elf.extend_from_slice(&strtab);
    pad_to(&mut elf, dline_off);
    elf.extend_from_slice(&debug_line);
    pad_to(&mut elf, dinfo_off);
    elf.extend_from_slice(&debug_info);
    pad_to(&mut elf, dabbrev_off);
    elf.extend_from_slice(&debug_abbrev);
    pad_to(&mut elf, shstr_off);
    elf.extend_from_slice(&shstrtab);
    pad_to(&mut elf, shoff);

    // Section headers
    elf.extend_from_slice(&[0u8; 40]); // Null
    push_shdr(&mut elf, sh_text, 1, 6, ro_base, text_off, ro_bytes.len() as u32, 0, 0, 4, 0);
    if has_data {
        push_shdr(&mut elf, sh_data, 1, 3, rw_base, data_off, rw_bytes.len() as u32, 0, 0, 4, 0);
    }
    push_shdr(&mut elf, sh_symtab, 2, 0, 0, sym_off, symtab.len() as u32, str_idx as u32, 1, 4, 16);
    push_shdr(&mut elf, sh_strtab, 3, 0, 0, str_off, strtab.len() as u32, 0, 0, 1, 0);
    push_shdr(&mut elf, sh_dline, 1, 0, 0, dline_off, debug_line.len() as u32, 0, 0, 1, 0);
    push_shdr(&mut elf, sh_dinfo, 1, 0, 0, dinfo_off, debug_info.len() as u32, 0, 0, 1, 0);
    push_shdr(&mut elf, sh_dabbrev, 1, 0, 0, dabbrev_off, debug_abbrev.len() as u32, 0, 0, 1, 0);
    push_shdr(&mut elf, sh_shstrtab, 3, 0, 0, shstr_off, shstrtab.len() as u32, 0, 0, 1, 0);

    elf
}

fn build_dwarf_v2_line(files: &[String], rows: &[EmittedLine]) -> Vec<u8> {
    let mut header = Vec::new();
    header.push(2); // minimum_instruction_length
    header.push(1); // default_is_stmt
    header.push((-5i8) as u8); // line_base
    header.push(14); // line_range
    header.push(10); // opcode_base
    header.extend_from_slice(&[0, 1, 1, 1, 1, 0, 0, 0, 1]); // standard_opcode_lengths (1..9)
    header.push(0); // include_directories (empty)
    for f in files {
        header.extend_from_slice(f.as_bytes());
        header.push(0);
        header.push(0); // dir_index = 0
        header.push(0); // mtime = 0
        header.push(0); // size = 0
    }
    header.push(0); // end of file_names

    let header_len = header.len() as u32;
    let mut prog = Vec::new();
    if let Some(first) = rows.first() {
        // DW_LNE_set_address
        prog.push(0);
        prog.push(5);
        prog.push(2);
        prog.extend_from_slice(&first.addr.to_le_bytes());

        let mut cur_addr = first.addr;
        let mut cur_line: i64 = 1;
        let mut cur_file: usize = 0;

        for r in rows {
            if r.file_idx != cur_file {
                prog.push(4); // DW_LNS_set_file
                write_uleb128(&mut prog, (r.file_idx + 1) as u64);
                cur_file = r.file_idx;
            }
            let delta_pc = r.addr.saturating_sub(cur_addr) / 2;
            if delta_pc > 0 {
                prog.push(2); // DW_LNS_advance_pc
                write_uleb128(&mut prog, delta_pc as u64);
                cur_addr = r.addr;
            }
            let delta_line = (r.line as i64) - cur_line;
            if delta_line != 0 {
                prog.push(3); // DW_LNS_advance_line
                write_sleb128(&mut prog, delta_line);
                cur_line = r.line as i64;
            }
            prog.push(1); // DW_LNS_copy
        }
        prog.push(0);
        prog.push(1);
        prog.push(1); // DW_LNE_end_sequence
    }

    let unit_len: u32 = 2 + 4 + header_len + prog.len() as u32;
    let mut out = Vec::with_capacity((4 + unit_len) as usize);
    out.extend_from_slice(&unit_len.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes()); // DWARF v2
    out.extend_from_slice(&header_len.to_le_bytes());
    out.extend_from_slice(&header);
    out.extend_from_slice(&prog);
    out
}

pub fn format_intel_hex(base_addr: u32, bytes: &[u8]) -> String {
    let mut lines = Vec::new();
    let mut cur_hi: Option<u16> = None;
    for (idx, chunk) in bytes.chunks(16).enumerate() {
        let addr = base_addr + (idx * 16) as u32;
        let hi = (addr >> 16) as u16;
        if cur_hi != Some(hi) {
            cur_hi = Some(hi);
            let rec = [0x02u8, 0x00, 0x00, 0x04, (hi >> 8) as u8, (hi & 0xFF) as u8];
            lines.push(hex_record(&rec));
        }
        let lo = (addr & 0xFFFF) as u16;
        let mut rec = Vec::with_capacity(4 + chunk.len());
        rec.push(chunk.len() as u8);
        rec.push((lo >> 8) as u8);
        rec.push((lo & 0xFF) as u8);
        rec.push(0x00);
        rec.extend_from_slice(chunk);
        lines.push(hex_record(&rec));
    }
    lines.push(":00000001FF".to_string());
    lines.join("\r\n") + "\r\n"
}

fn hex_record(body: &[u8]) -> String {
    let sum: u8 = body.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
    let csum = (!sum).wrapping_add(1);
    let mut s = String::from(":");
    for &b in body {
        s.push_str(&format!("{:02X}", b));
    }
    s.push_str(&format!("{:02X}", csum));
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
        if val == 0 { break; }
    }
}

fn write_sleb128(out: &mut Vec<u8>, mut val: i64) {
    loop {
        let mut b = (val & 0x7F) as u8;
        val >>= 7;
        let done = (val == 0 && (b & 0x40) == 0) || (val == -1 && (b & 0x40) != 0);
        if !done {
            b |= 0x80;
        }
        out.push(b);
        if done { break; }
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
    while (buf.len() as u32) < target {
        buf.push(0);
    }
}

fn push_phdr(buf: &mut Vec<u8>, p_type: u32, off: u32, vaddr: u32, filesz: u32, memsz: u32, flags: u32) {
    buf.extend_from_slice(&p_type.to_le_bytes());
    buf.extend_from_slice(&off.to_le_bytes());
    buf.extend_from_slice(&vaddr.to_le_bytes());
    buf.extend_from_slice(&vaddr.to_le_bytes());
    buf.extend_from_slice(&filesz.to_le_bytes());
    buf.extend_from_slice(&memsz.to_le_bytes());
    buf.extend_from_slice(&flags.to_le_bytes());
    buf.extend_from_slice(&4u32.to_le_bytes());
}

fn push_shdr(
    buf: &mut Vec<u8>,
    name: u32,
    sh_type: u32,
    flags: u32,
    addr: u32,
    off: u32,
    size: u32,
    link: u32,
    info: u32,
    align: u32,
    entsize: u32,
) {
    buf.extend_from_slice(&name.to_le_bytes());
    buf.extend_from_slice(&sh_type.to_le_bytes());
    buf.extend_from_slice(&flags.to_le_bytes());
    buf.extend_from_slice(&addr.to_le_bytes());
    buf.extend_from_slice(&off.to_le_bytes());
    buf.extend_from_slice(&size.to_le_bytes());
    buf.extend_from_slice(&link.to_le_bytes());
    buf.extend_from_slice(&info.to_le_bytes());
    buf.extend_from_slice(&align.to_le_bytes());
    buf.extend_from_slice(&entsize.to_le_bytes());
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::elf::LoadedElf;
    use crate::sim::Simulator;
    use crate::target::DebugTarget;
    use crate::toolchain::ToolchainInfo;

    #[test]
    fn test_builtin_assemble_and_simulate() {
        let tmp = std::env::temp_dir().join("uvcli_asm_unit_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let s_path = tmp.join("test_unit.s");
        std::fs::write(
            &s_path,
            r#"
    PRESERVE8
    THUMB
    AREA RESET, DATA, READONLY
    EXPORT __Vectors
__Vectors
    DCD 0x20001000
    DCD Reset_Handler
    ALIGN
    AREA MYCODE, CODE, READONLY
    ENTRY
    EXPORT Reset_Handler
Reset_Handler
    MOV R1, #0xFF000000
    ORR R1, #0xFF0000
    ORR R1, #0xFF00
    MOV R2, #4
    LSL R3, R1, #4
    LSR R4, R1, #4
    ASR R5, R1, R2
    ROR R6, R1, #3
    LDR R0, =0x12345678
STOP
    B STOP
    END
"#,
        )
        .unwrap();

        let proj = UvProject::from_standalone_file(&s_path).unwrap();
        let res = assemble_project(&proj).unwrap();
        assert!(res.success, "Assemble failed: {}", res.log);
        let axf = res.axf_path.unwrap();
        let tc = ToolchainInfo::discover(Some(r"C:\nonexistent_keil"), None);
        let elf = LoadedElf::load(Path::new(&axf), &tc).unwrap();
        assert!(!elf.disassembly.is_empty(), "Built-in disassembly should be populated");
        assert!(!elf.lines.is_empty(), "DWARF lines should be populated");

        let mut sim = Simulator::new(elf, 8_000_000);
        let st = sim.run(500, true).unwrap();
        assert_eq!(st.registers.r[0], 0x1234_5678);
        assert_eq!(st.registers.r[1], 0xFFFF_FF00);
        assert_eq!(st.registers.r[2], 4);
        assert_eq!(st.registers.r[3], 0xFFFF_F000);
        assert_eq!(st.registers.r[4], 0x0FFF_FFF0);
        assert_eq!(st.registers.r[5], 0xFFFF_FFF0);
        assert_eq!(st.registers.r[6], 0x1FFF_FFE0);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
