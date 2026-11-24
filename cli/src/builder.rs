use crate::asm;
use crate::project::{AsmSyntax, UvProject};
pub use crate::target::{BuildResult, Diagnostic};
use crate::toolchain::ToolchainInfo;
use anyhow::{anyhow, Result};
use object::{Object, ObjectSegment};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolchainMode {
    Auto,
    Builtin,
    Keil,
    Gnu,
}

impl ToolchainMode {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "builtin" => Self::Builtin,
            "keil" => Self::Keil,
            "gnu" => Self::Gnu,
            _ => Self::Auto,
        }
    }
}

pub fn build_project(project: &UvProject, tc: &ToolchainInfo, rebuild: bool) -> Result<BuildResult> {
    build_project_with_mode(project, tc, rebuild, ToolchainMode::Auto)
}

pub fn build_project_with_mode(
    project: &UvProject,
    tc: &ToolchainInfo,
    rebuild: bool,
    mode: ToolchainMode,
) -> Result<BuildResult> {
    let t0 = Instant::now();
    let build_dir = project.build_dir();
    if rebuild && build_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&build_dir) {
            for e in entries.filter_map(|x| x.ok()) {
                let p = e.path();
                if p.is_file() {
                    let _ = std::fs::remove_file(p);
                }
            }
        }
    }
    std::fs::create_dir_all(&build_dir)?;

    let sources = project.source_files();
    if sources.is_empty() {
        return Err(anyhow!("No compilable source files (.s/.c) in project"));
    }

    let has_c = sources.iter().any(|s| s.file_type == 1);
    let has_keil = tc.armasm.is_some() && tc.armclang.is_some() && tc.armlink.is_some();

    match mode {
        ToolchainMode::Builtin => {
            if has_c {
                return Err(anyhow!("Built-in assembler only supports ARM Assembly (.s/.asm) files. Install Arm GNU Toolchain (arm-none-eabi-gcc) or Keil MDK to compile C files."));
            }
            return asm::assemble_project(project);
        }
        ToolchainMode::Gnu => {
            return build_gnu_project(project, tc, t0);
        }
        ToolchainMode::Auto => {
            if !has_c && !has_keil {
                return asm::assemble_project(project);
            }
            if has_c && !has_keil {
                if tc.arm_gcc.is_some() {
                    return build_gnu_project(project, tc, t0);
                }
                return Err(anyhow!(
                    "No C compiler found. Assembly (.s) projects build with the built-in assembler; for .c files, install Arm GNU Toolchain (arm-none-eabi-gcc) or Keil MDK."
                ));
            }
        }
        ToolchainMode::Keil => {}
    }

    let mut log_lines: Vec<String> = Vec::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut objects: Vec<PathBuf> = Vec::new();
    let mut has_vectors = false;
    let mut has_reset_handler = false;

    log_lines.push(format!(
        "*** Using Keil Toolchain: {} ({})",
        tc.keil_root.as_deref().unwrap_or("Unknown"),
        project.options.device
    ));

    let root = PathBuf::from(&project.root_dir);
    let mut all_ok = true;

    for (idx, src) in sources.iter().enumerate() {
        let src_path = Path::new(&src.abs_path);
        if !src_path.exists() {
            diagnostics.push(Diagnostic {
                file: src.abs_path.clone(),
                line: 1,
                column: 1,
                severity: "error".to_string(),
                code: "ENOENT".to_string(),
                message: format!("Source file not found: {}", src.abs_path),
            });
            log_lines.push(format!("Error: Source file not found: {}", src.abs_path));
            all_ok = false;
            continue;
        }

        if let Ok(text) = std::fs::read_to_string(src_path) {
            if text.contains("__Vectors") {
                has_vectors = true;
            }
            if text.contains("Reset_Handler") {
                has_reset_handler = true;
            }
        }

        let stem = src_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unit");
        let obj_path = build_dir.join(format!("{}_{}.o", stem, idx));

        if src.file_type == 2 {
            let is_gnu = project.options.asm_syntax == AsmSyntax::GnuArmClang || looks_like_gnu_asm(src_path);
            if is_gnu {
                log_lines.push(format!("assembling {} (armclang)...", src.name));
                let armclang = tc.require_armclang()?;
                let mut cmd = Command::new(armclang);
                cmd.args([
                    "--target=arm-arm-none-eabi",
                    "-mcpu=cortex-m3",
                    "-mthumb",
                    "-g",
                    "-c",
                ]);
                for inc in &project.options.a_includes {
                    cmd.arg(format!("-I{}", resolve_path(&root, inc).display()));
                }
                for def in &project.options.a_defines {
                    cmd.arg(format!("-D{}", def));
                }
                for extra in split_shell_args(&project.options.a_misc) {
                    cmd.arg(extra);
                }
                cmd.arg(src_path).arg("-o").arg(&obj_path);

                let out = cmd.output()?;
                let txt = combine_output(&out.stdout, &out.stderr);
                if !txt.trim().is_empty() {
                    log_lines.push(txt.trim_end().to_string());
                    diagnostics.extend(parse_clang_diagnostics(&txt, &src.abs_path));
                }
                if !out.status.success() {
                    all_ok = false;
                } else {
                    objects.push(obj_path);
                }
            } else {
                log_lines.push(format!("assembling {}...", src.name));
                let armasm = tc.require_armasm()?;
                let mut cmd = Command::new(armasm);
                cmd.args([
                    "--cpu=Cortex-M3",
                    "-g",
                    "--apcs=interwork",
                    "--diag_suppress=A1950W",
                ]);
                for inc in &project.options.a_includes {
                    cmd.arg(format!("-I{}", resolve_path(&root, inc).display()));
                }
                for def in &project.options.a_defines {
                    cmd.arg("--predefine").arg(format!("{} SETA 1", def));
                }
                for extra in split_shell_args(&project.options.a_misc) {
                    cmd.arg(extra);
                }
                cmd.arg(src_path).arg("-o").arg(&obj_path);

                let out = cmd.output()?;
                let txt = combine_output(&out.stdout, &out.stderr);
                if !txt.trim().is_empty() {
                    log_lines.push(txt.trim_end().to_string());
                    diagnostics.extend(parse_armasm_diagnostics(&txt, &src.abs_path));
                }
                if !out.status.success() {
                    all_ok = false;
                } else {
                    objects.push(obj_path);
                }
            }
        } else if src.file_type == 1 {
            log_lines.push(format!("compiling {}...", src.name));
            let armclang = tc.require_armclang()?;
            let mut cmd = Command::new(armclang);
            cmd.args([
                "--target=arm-arm-none-eabi",
                "-mcpu=cortex-m3",
                "-mthumb",
                "-g",
                "-O0",
                "-c",
            ]);
            for def in &project.options.c_defines {
                cmd.arg(format!("-D{}", def));
            }
            for inc in &project.options.c_includes {
                cmd.arg(format!("-I{}", resolve_path(&root, inc).display()));
            }
            if let Some(cmsis_inc) = tc.cmsis_include.as_deref() {
                cmd.arg(format!("-I{}", cmsis_inc));
            }
            if let Some(st_inc) = tc.stm32f1_include.as_deref() {
                cmd.arg(format!("-I{}", st_inc));
            }
            for extra in split_shell_args(&project.options.c_misc) {
                cmd.arg(extra);
            }
            cmd.arg(src_path).arg("-o").arg(&obj_path);

            let out = cmd.output()?;
            let txt = combine_output(&out.stdout, &out.stderr);
            if !txt.trim().is_empty() {
                log_lines.push(txt.trim_end().to_string());
                diagnostics.extend(parse_clang_diagnostics(&txt, &src.abs_path));
            }
            if !out.status.success() {
                all_ok = false;
            } else {
                objects.push(obj_path);
            }
        }
    }

    let axf_path = project.axf_path();
    let mut hex_out_path: Option<String> = None;
    let mut bin_out_path: Option<String> = None;

    if all_ok && !objects.is_empty() {
        log_lines.push("linking...".to_string());
        let armlink = tc.require_armlink()?;
        let mut cmd = Command::new(armlink);
        cmd.arg("--cpu=Cortex-M3");
        if let Some(scat) = project.options.scatter_file.as_deref().filter(|s| !s.trim().is_empty()) {
            cmd.arg("--scatter").arg(resolve_path(&root, scat));
        } else {
            cmd.arg(format!("--ro-base=0x{:08X}", project.options.ro_base));
            cmd.arg(format!("--rw-base=0x{:08X}", project.options.rw_base));
        }
        if has_reset_handler {
            cmd.arg("--entry=Reset_Handler");
        }
        if has_vectors {
            cmd.arg("--first=__Vectors");
        }
        cmd.args(["--strict", "--info=totals"]);
        for extra in split_shell_args(&project.options.ld_misc) {
            cmd.arg(extra);
        }
        for obj in &objects {
            cmd.arg(obj);
        }
        cmd.arg("-o").arg(&axf_path);

        let out = cmd.output()?;
        let txt = combine_output(&out.stdout, &out.stderr);
        if !txt.trim().is_empty() {
            log_lines.push(txt.trim_end().to_string());
            diagnostics.extend(parse_armlink_diagnostics(&txt));
        }
        if !out.status.success() {
            all_ok = false;
        } else {
            if project.options.create_hex {
                let hp = project.hex_path();
                if let Ok(fromelf) = tc.require_fromelf() {
                    let h_out = Command::new(fromelf)
                        .arg("--i32")
                        .arg("--output")
                        .arg(&hp)
                        .arg(&axf_path)
                        .output()?;
                    if h_out.status.success() && hp.exists() {
                        hex_out_path = Some(hp.to_string_lossy().into_owned());
                    }
                } else if emit_hex_from_elf(&axf_path, &hp).is_ok() {
                    hex_out_path = Some(hp.to_string_lossy().into_owned());
                }
            }
            if project.options.create_bin {
                let bp = project.bin_path();
                if let Ok(fromelf) = tc.require_fromelf() {
                    let b_out = Command::new(fromelf)
                        .arg("--bin")
                        .arg("--output")
                        .arg(&bp)
                        .arg(&axf_path)
                        .output()?;
                    if b_out.status.success() && bp.exists() {
                        bin_out_path = Some(bp.to_string_lossy().into_owned());
                    }
                } else if emit_bin_from_elf(&axf_path, &bp).is_ok() {
                    bin_out_path = Some(bp.to_string_lossy().into_owned());
                }
            }
        }
    }

    let errors = diagnostics.iter().filter(|d| d.severity == "error").count()
        + if !all_ok && diagnostics.iter().all(|d| d.severity != "error") {
            1
        } else {
            0
        };
    let warnings = diagnostics.iter().filter(|d| d.severity == "warning").count();
    let elapsed_ms = t0.elapsed().as_millis() as u64;

    log_lines.push(format!(
        "\"{}\" - {} Error(s), {} Warning(s). [{} ms]",
        axf_path.display(),
        errors,
        warnings,
        elapsed_ms
    ));

    Ok(BuildResult {
        success: all_ok && axf_path.exists(),
        axf_path: if all_ok && axf_path.exists() {
            Some(axf_path.to_string_lossy().into_owned())
        } else {
            None
        },
        hex_path: hex_out_path,
        bin_path: bin_out_path,
        elapsed_ms,
        errors,
        warnings,
        diagnostics,
        log: log_lines.join("\n"),
    })
}

fn build_gnu_project(project: &UvProject, tc: &ToolchainInfo, t0: Instant) -> Result<BuildResult> {
    let gcc = tc
        .arm_gcc
        .as_deref()
        .ok_or_else(|| anyhow!("arm-none-eabi-gcc.exe not found"))?;
    let build_dir = project.build_dir();
    let root = PathBuf::from(&project.root_dir);
    let mut log_lines = vec![format!("*** Using Arm GNU Toolchain: {} ({})", gcc, project.options.device)];
    let mut diagnostics = Vec::new();
    let mut objs = Vec::new();
    let mut all_ok = true;

    for (idx, src) in project.source_files().iter().enumerate() {
        let src_path = Path::new(&src.abs_path);
        let stem = src_path.file_stem().and_then(|s| s.to_str()).unwrap_or("unit");
        let obj_path = build_dir.join(format!("{}_{}.o", stem, idx));
        log_lines.push(format!("compiling {} (arm-none-eabi-gcc)...", src.name));
        let mut cmd = Command::new(gcc);
        cmd.args(["-mcpu=cortex-m3", "-mthumb", "-g", "-O0", "-c"]);
        for def in &project.options.c_defines {
            cmd.arg(format!("-D{}", def));
        }
        for inc in &project.options.c_includes {
            cmd.arg(format!("-I{}", resolve_path(&root, inc).display()));
        }
        cmd.arg(src_path).arg("-o").arg(&obj_path);
        let out = cmd.output()?;
        let txt = combine_output(&out.stdout, &out.stderr);
        if !txt.trim().is_empty() {
            log_lines.push(txt.trim_end().to_string());
            diagnostics.extend(parse_clang_diagnostics(&txt, &src.abs_path));
        }
        if !out.status.success() {
            all_ok = false;
        } else {
            objs.push(obj_path);
        }
    }

    let axf_path = project.axf_path();
    let mut hex_out = None;
    let mut bin_out = None;

    if all_ok && !objs.is_empty() {
        let ld_path = build_dir.join("auto_stm32f1.ld");
        let ld_script = format!(
            "ENTRY(Reset_Handler)\nMEMORY {{\n  FLASH (rx) : ORIGIN = 0x{:08X}, LENGTH = 0x{:08X}\n  RAM (rwx)  : ORIGIN = 0x{:08X}, LENGTH = 0x{:08X}\n}}\nSECTIONS {{\n  .text : {{\n    KEEP(*(RESET))\n    KEEP(*(.isr_vector))\n    *(.text*)\n    *(.rodata*)\n  }} > FLASH\n  .data : {{ *(.data*) }} > RAM AT > FLASH\n  .bss : {{ *(.bss*) *(COMMON) }} > RAM\n}}\n",
            project.options.ro_base,
            project.options.ro_size,
            project.options.rw_base,
            project.options.rw_size
        );
        std::fs::write(&ld_path, ld_script)?;

        let mut cmd = Command::new(gcc);
        cmd.args(["-mcpu=cortex-m3", "-mthumb", "-nostdlib", "-T"])
            .arg(&ld_path);
        for o in &objs {
            cmd.arg(o);
        }
        cmd.arg("-o").arg(&axf_path);
        let out = cmd.output()?;
        let txt = combine_output(&out.stdout, &out.stderr);
        if !txt.trim().is_empty() {
            log_lines.push(txt.trim_end().to_string());
        }
        if !out.status.success() {
            all_ok = false;
        } else {
            if project.options.create_hex {
                let hp = project.hex_path();
                if emit_hex_from_elf(&axf_path, &hp).is_ok() {
                    hex_out = Some(hp.to_string_lossy().into_owned());
                }
            }
            if project.options.create_bin {
                let bp = project.bin_path();
                if emit_bin_from_elf(&axf_path, &bp).is_ok() {
                    bin_out = Some(bp.to_string_lossy().into_owned());
                }
            }
        }
    }

    let errors = diagnostics.iter().filter(|d| d.severity == "error").count()
        + if !all_ok && diagnostics.iter().all(|d| d.severity != "error") { 1 } else { 0 };
    let warnings = diagnostics.iter().filter(|d| d.severity == "warning").count();
    let elapsed_ms = t0.elapsed().as_millis() as u64;
    log_lines.push(format!("\"{}\" - {} Error(s), {} Warning(s). [{} ms]", axf_path.display(), errors, warnings, elapsed_ms));
    Ok(BuildResult {
        success: all_ok && axf_path.exists(),
        axf_path: if all_ok && axf_path.exists() { Some(axf_path.to_string_lossy().into_owned()) } else { None },
        hex_path: hex_out,
        bin_path: bin_out,
        elapsed_ms,
        errors,
        warnings,
        diagnostics,
        log: log_lines.join("\n"),
    })
}

fn emit_hex_from_elf(elf_path: &Path, hex_path: &Path) -> Result<()> {
    let data = std::fs::read(elf_path)?;
    let obj = object::File::parse(&*data)?;
    for seg in obj.segments() {
        let addr = seg.address() as u32;
        if let Ok(bytes) = seg.data() {
            if !bytes.is_empty() && addr < 0x1000_0000 {
                std::fs::write(hex_path, asm::format_intel_hex(addr, bytes))?;
                return Ok(());
            }
        }
    }
    Err(anyhow!("No loadable ROM segment in ELF"))
}

fn emit_bin_from_elf(elf_path: &Path, bin_path: &Path) -> Result<()> {
    let data = std::fs::read(elf_path)?;
    let obj = object::File::parse(&*data)?;
    for seg in obj.segments() {
        let addr = seg.address() as u32;
        if let Ok(bytes) = seg.data() {
            if !bytes.is_empty() && addr < 0x1000_0000 {
                std::fs::write(bin_path, bytes)?;
                return Ok(());
            }
        }
    }
    Err(anyhow!("No loadable ROM segment in ELF"))
}

fn looks_like_gnu_asm(path: &Path) -> bool {
    let Ok(txt) = std::fs::read_to_string(path) else {
        return false;
    };
    let has_armasm = txt.lines().any(|l| {
        let t = l.split(';').next().unwrap_or("").trim().to_ascii_uppercase();
        t.starts_with("AREA ") || t == "THUMB" || t.starts_with("EXPORT ") || t.starts_with("DCD ")
    });
    if has_armasm {
        return false;
    }
    txt.lines().any(|l| {
        let t = l.trim();
        t.starts_with(".syntax") || t.starts_with(".section") || t.starts_with(".global") || t.starts_with(".thumb")
    })
}

fn resolve_path(root: &Path, p: &str) -> PathBuf {
    let candidate = PathBuf::from(p.replace('\\', "/"));
    if candidate.is_absolute() {
        candidate
    } else {
        root.join(candidate)
    }
}

fn split_shell_args(s: &str) -> Vec<String> {
    s.split_whitespace().map(|x| x.to_string()).collect()
}

fn combine_output(stdout: &[u8], stderr: &[u8]) -> String {
    let mut s = String::from_utf8_lossy(stdout).into_owned();
    let err = String::from_utf8_lossy(stderr);
    if !err.is_empty() {
        if !s.is_empty() && !s.ends_with('\n') {
            s.push('\n');
        }
        s.push_str(&err);
    }
    s
}

fn parse_armasm_diagnostics(text: &str, default_file: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for line in text.lines() {
        // Format: "path\file.s", line 12: Error: A1163E: message
        let trimmed = line.trim();
        if let Some(idx) = trimmed.find(", line ") {
            let file = trimmed[..idx].trim().trim_matches('"').to_string();
            let rest = &trimmed[idx + 7..];
            if let Some(colon) = rest.find(':') {
                let line_num = rest[..colon].trim().parse::<u32>().unwrap_or(1);
                let msg_part = rest[colon + 1..].trim();
                let (sev, msg_after_sev) = if let Some(m) = msg_part.strip_prefix("Error:") {
                    ("error", m.trim())
                } else if let Some(m) = msg_part.strip_prefix("Warning:") {
                    ("warning", m.trim())
                } else if let Some(m) = msg_part.strip_prefix("Fatal error:") {
                    ("error", m.trim())
                } else {
                    continue;
                };
                let (code, msg) = split_keil_code(msg_after_sev);
                out.push(Diagnostic {
                    file: if file.is_empty() { default_file.to_string() } else { file },
                    line: line_num,
                    column: 1,
                    severity: sev.to_string(),
                    code,
                    message: msg,
                });
            }
        } else if trimmed.starts_with("Error:") || trimmed.starts_with("Fatal error:") {
            let msg = trimmed
                .trim_start_matches("Fatal error:")
                .trim_start_matches("Error:")
                .trim();
            let (code, message) = split_keil_code(msg);
            out.push(Diagnostic {
                file: default_file.to_string(),
                line: 1,
                column: 1,
                severity: "error".to_string(),
                code,
                message,
            });
        }
    }
    out
}

fn parse_clang_diagnostics(text: &str, default_file: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for line in text.lines() {
        // path:line:col: error: message  (note Windows drive letter C:\... or E:\...)
        let bytes = line.as_bytes();
        let search_start = if bytes.len() >= 2 && bytes[1] == b':' { 2 } else { 0 };
        let Some(c1) = line[search_start..].find(':') else { continue };
        let p1 = search_start + c1;
        let file = line[..p1].trim();
        let rest1 = &line[p1 + 1..];
        let Some(c2) = rest1.find(':') else { continue };
        let Ok(line_num) = rest1[..c2].trim().parse::<u32>() else { continue };
        let rest2 = &rest1[c2 + 1..];
        let Some(c3) = rest2.find(':') else { continue };
        let col_num = rest2[..c3].trim().parse::<u32>().unwrap_or(1);
        let rest3 = rest2[c3 + 1..].trim();
        let (sev, msg) = if let Some(m) = rest3.strip_prefix("error:") {
            ("error", m.trim())
        } else if let Some(m) = rest3.strip_prefix("fatal error:") {
            ("error", m.trim())
        } else if let Some(m) = rest3.strip_prefix("warning:") {
            ("warning", m.trim())
        } else {
            continue;
        };
        out.push(Diagnostic {
            file: if file.is_empty() { default_file.to_string() } else { file.to_string() },
            line: line_num,
            column: col_num,
            severity: sev.to_string(),
            code: String::new(),
            message: msg.to_string(),
        });
    }
    out
}

fn parse_armlink_diagnostics(text: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(m) = t.strip_prefix("Error:").or_else(|| t.strip_prefix("Fatal error:")) {
            let (code, message) = split_keil_code(m.trim());
            out.push(Diagnostic {
                file: String::new(),
                line: 0,
                column: 0,
                severity: "error".to_string(),
                code,
                message,
            });
        } else if let Some(m) = t.strip_prefix("Warning:") {
            let (code, message) = split_keil_code(m.trim());
            out.push(Diagnostic {
                file: String::new(),
                line: 0,
                column: 0,
                severity: "warning".to_string(),
                code,
                message,
            });
        }
    }
    out
}

fn split_keil_code(s: &str) -> (String, String) {
    if let Some(idx) = s.find(':') {
        let cand = s[..idx].trim();
        if !cand.is_empty() && cand.len() <= 10 && cand.chars().all(|c| c.is_ascii_alphanumeric()) {
            return (cand.to_string(), s[idx + 1..].trim().to_string());
        }
    }
    (String::new(), s.to_string())
}
