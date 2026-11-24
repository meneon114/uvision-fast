use crate::asm::{assemble_sources, InMemoryBuild};
use crate::elf::LoadedElf;
use crate::project::{resolve_virtual_path, TargetOptions, UvProject};
use crate::repl::{execute_debug_command, resolve_bp_spec};
use crate::sim::{parse_num_u32, Simulator};
use crate::target::{BuildResult, DebugTarget, Diagnostic};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::slice;

#[derive(Debug, Deserialize)]
struct RpcRequest {
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct WasmRpcResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

struct WasmState {
    vfs: HashMap<String, String>,
    vfs_keys: HashMap<String, String>, // lowercase norm -> display path
    project: Option<UvProject>,
    elf: Option<LoadedElf>,
    sim: Option<Simulator>,
    last_elf_bytes: Option<Vec<u8>>,
    last_hex_text: Option<String>,
    last_bin_bytes: Option<Vec<u8>>,
    last_output_name: String,
}

impl WasmState {
    fn new() -> Self {
        Self {
            vfs: HashMap::new(),
            vfs_keys: HashMap::new(),
            project: None,
            elf: None,
            sim: None,
            last_elf_bytes: None,
            last_hex_text: None,
            last_bin_bytes: None,
            last_output_name: "firmware".to_string(),
        }
    }

    fn norm_key(path: &str) -> String {
        path.replace('\\', "/").to_ascii_lowercase()
    }

    fn norm_display(path: &str) -> String {
        path.replace('\\', "/")
    }

    fn vfs_put(&mut self, path: &str, content: String) {
        let disp = Self::norm_display(path);
        let key = Self::norm_key(&disp);
        self.vfs_keys.insert(key.clone(), disp);
        self.vfs.insert(key, content);
    }

    fn vfs_get(&self, path: &str) -> Option<(&str, &str)> {
        let key = Self::norm_key(path);
        let content = self.vfs.get(&key)?;
        let disp = self.vfs_keys.get(&key)?;
        Some((disp.as_str(), content.as_str()))
    }

    fn vfs_contains(&self, path: &str) -> bool {
        self.vfs.contains_key(&Self::norm_key(path))
    }

    fn vfs_remove(&mut self, path: &str) {
        let key = Self::norm_key(path);
        self.vfs.remove(&key);
        self.vfs_keys.remove(&key);
    }

    fn open_virtual_project(&self, path: &str) -> Result<UvProject> {
        let disp_in = Self::norm_display(path);
        let ext = disp_in
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        if ext == "uvprojx" || ext == "uvproj" {
            let (disp, content) = self
                .vfs_get(&disp_in)
                .ok_or_else(|| anyhow!("Project file not found in workspace: {}", path))?;
            let root = match disp.rsplit_once('/') {
                Some((r, _)) if !r.is_empty() => r,
                _ => "/",
            };
            return UvProject::parse_uvprojx_str(content, disp, root);
        }
        if matches!(ext.as_str(), "s" | "asm" | "c") {
            let disp = self
                .vfs_get(&disp_in)
                .map(|(d, _)| d.to_string())
                .unwrap_or(disp_in);
            return Ok(UvProject::from_virtual_file(&disp));
        }
        Err(anyhow!("Unsupported project or source file: {}", path))
    }

    fn build_virtual(&mut self, proj: &UvProject) -> Result<BuildResult> {
        let sources_meta = proj.source_files();
        if sources_meta.is_empty() {
            return Err(anyhow!("No compilable source files (.s/.asm) in project"));
        }

        let has_c = sources_meta.iter().any(|s| s.file_type == 1);
        if has_c {
            let c_src = sources_meta.iter().find(|s| s.file_type == 1).unwrap();
            let diag = Diagnostic {
                file: c_src.abs_path.clone(),
                line: 1,
                column: 1,
                severity: "error".to_string(),
                code: "WASM_C".to_string(),
                message: "WebAssembly built-in toolchain compiles ARM Assembly (.s/.asm) directly in your browser. For C compilation, use desktop uVision Fast CLI / VS Code extension with Arm GNU GCC or Keil MDK.".to_string(),
            };
            let log = format!(
                "*** WebAssembly Built-in Toolchain\n{}({}): error WASM_C: {}\nBuild failed: 1 Error(s).",
                diag.file, diag.line, diag.message
            );
            return Ok(BuildResult {
                success: false,
                axf_path: None,
                hex_path: None,
                bin_path: None,
                elapsed_ms: 0,
                errors: 1,
                warnings: 0,
                diagnostics: vec![diag],
                log,
            });
        }

        let mut sources: Vec<(String, String)> = Vec::new();
        let mut missing: Vec<Diagnostic> = Vec::new();
        for s in sources_meta {
            if let Some((disp, body)) = self.vfs_get(&s.abs_path) {
                sources.push((disp.to_string(), body.to_string()));
            } else {
                let candidate = resolve_virtual_path(&proj.root_dir, &s.path);
                if let Some((disp, body)) = self.vfs_get(&candidate) {
                    sources.push((disp.to_string(), body.to_string()));
                } else {
                    missing.push(Diagnostic {
                        file: s.abs_path.clone(),
                        line: 1,
                        column: 1,
                        severity: "error".to_string(),
                        code: "ENOENT".to_string(),
                        message: format!("Source file not found in workspace: {}", s.abs_path),
                    });
                }
            }
        }

        if !missing.is_empty() {
            let errs = missing.len();
            let log = missing
                .iter()
                .map(|d| format!("{}({}): error {}: {}", d.file, d.line, d.code, d.message))
                .collect::<Vec<_>>()
                .join("\n");
            return Ok(BuildResult {
                success: false,
                axf_path: None,
                hex_path: None,
                bin_path: None,
                elapsed_ms: 0,
                errors: errs,
                warnings: 0,
                diagnostics: missing,
                log,
            });
        }

        let axf_label = format!(
            "{}/{}/{}.axf",
            proj.root_dir.trim_end_matches('/'),
            proj.options.output_dir,
            proj.options.output_name
        );
        let hex_label = if proj.options.create_hex {
            Some(format!(
                "{}/{}/{}.hex",
                proj.root_dir.trim_end_matches('/'),
                proj.options.output_dir,
                proj.options.output_name
            ))
        } else {
            None
        };
        let bin_label = if proj.options.create_bin {
            Some(format!(
                "{}/{}/{}.bin",
                proj.root_dir.trim_end_matches('/'),
                proj.options.output_dir,
                proj.options.output_name
            ))
        } else {
            None
        };

        let InMemoryBuild {
            result,
            elf_bytes,
            hex_text,
            bin_bytes,
        } = assemble_sources(
            &proj.options,
            &axf_label,
            hex_label.as_deref(),
            bin_label.as_deref(),
            &sources,
        );

        if result.success {
            self.last_elf_bytes = elf_bytes;
            self.last_hex_text = hex_text;
            self.last_bin_bytes = bin_bytes;
            self.last_output_name = proj.options.output_name.clone();
        } else {
            self.last_elf_bytes = None;
            self.last_hex_text = None;
            self.last_bin_bytes = None;
        }

        Ok(result)
    }

    fn handle(&mut self, method: &str, params: Value) -> Result<Value> {
        match method {
            "vfs.sync" => {
                self.last_elf_bytes = None;
                self.last_hex_text = None;
                self.last_bin_bytes = None;
                if params.get("replace").and_then(|v| v.as_bool()).unwrap_or(false) {
                    self.vfs.clear();
                    self.vfs_keys.clear();
                }
                if let Some(obj) = params.get("files").and_then(|v| v.as_object()) {
                    for (k, v) in obj {
                        if let Some(txt) = v.as_str() {
                            self.vfs_put(k, txt.to_string());
                        }
                    }
                }
                Ok(json!({ "count": self.vfs.len() }))
            }

            "vfs.write" => {
                let p = req_str(&params, "path")?;
                let c = req_str(&params, "content")?;
                self.vfs_put(&p, c);
                self.last_elf_bytes = None;
                Ok(json!({ "ok": true }))
            }

            "vfs.read" => {
                let p = req_str(&params, "path")?;
                let (disp, content) = self
                    .vfs_get(&p)
                    .ok_or_else(|| anyhow!("File not found: {}", p))?;
                Ok(json!({ "path": disp, "content": content }))
            }

            "vfs.delete" => {
                let p = req_str(&params, "path")?;
                self.vfs_remove(&p);
                self.last_elf_bytes = None;
                Ok(json!({ "ok": true }))
            }

            "vfs.list" => {
                let mut paths: Vec<String> = self.vfs_keys.values().cloned().collect();
                paths.sort();
                Ok(serde_json::to_value(paths)?)
            }

            "vfs.dump" => {
                let mut map: HashMap<String, String> = HashMap::new();
                for (k, disp) in &self.vfs_keys {
                    if let Some(body) = self.vfs.get(k) {
                        map.insert(disp.clone(), body.clone());
                    }
                }
                Ok(serde_json::to_value(map)?)
            }

            "toolchain.info" => Ok(json!({
                "builtin_assembler": true,
                "runtime": "wasm32-unknown-unknown",
                "keil_root": Value::Null,
                "armasm": Value::Null,
                "armclang": Value::Null,
                "armlink": Value::Null,
                "fromelf": Value::Null,
                "arm_gcc": Value::Null,
                "packs_dir": Value::Null,
                "openocd": Value::Null,
                "openocd_scripts": Value::Null,
                "packs": [
                    { "vendor": "Keil", "name": "STM32F1xx_DFP (Built-in)", "version": "2.4.1", "id": "Keil.STM32F1xx_DFP.2.4.1" },
                    { "vendor": "ARM", "name": "CMSIS (Cortex-M3 Simulator)", "version": "5.9.0", "id": "ARM.CMSIS.5.9.0" }
                ]
            })),

            "project.open" => {
                let p = req_str(&params, "path")?;
                let proj = self.open_virtual_project(&p)?;
                self.project = Some(proj.clone());
                Ok(serde_json::to_value(proj)?)
            }

            "project.new" => {
                let dir = params
                    .get("dir")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("/Workspace/LabProject");
                let name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("LabProject");
                let device = params
                    .get("device")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("STM32F100RB");
                let template = params
                    .get("template")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("asm");
                let (proj, files) = UvProject::new_virtual(dir, name, device, template)?;
                for (path, content) in files {
                    self.vfs_put(&path, content);
                }
                self.project = Some(proj.clone());
                Ok(serde_json::to_value(proj)?)
            }

            "project.addFile" => {
                if let Some(pp) = params.get("project_path").and_then(|v| v.as_str()) {
                    if self.project.is_none() {
                        self.project = Some(self.open_virtual_project(pp)?);
                    }
                }
                let file_path = req_str(&params, "file_path")?;
                let template = params.get("template").and_then(|v| v.as_str());
                let mut proj = self
                    .project
                    .take()
                    .ok_or_else(|| anyhow!("No project currently open"))?;
                let abs_candidate = resolve_virtual_path(&proj.root_dir, &file_path);
                let exists = self.vfs_contains(&abs_candidate);
                let (added, updated_xml, new_file) =
                    proj.add_virtual_file(&file_path, template, exists)?;
                if let Some((path, content)) = new_file {
                    self.vfs_put(&path, content);
                }
                if let (Some(pf), Some(xml)) = (&proj.project_file, updated_xml) {
                    self.vfs_put(pf, xml);
                }
                self.project = Some(proj.clone());
                Ok(json!({ "added": added, "project": proj }))
            }

            "project.saveOptions" => {
                let opts: TargetOptions = serde_json::from_value(
                    params.get("options").cloned().unwrap_or(params.clone()),
                )?;
                let mut proj = self
                    .project
                    .take()
                    .ok_or_else(|| anyhow!("No project currently open"))?;
                proj.options = opts;
                if let Some(pf) = &proj.project_file {
                    let xml = proj.to_uvprojx_xml()?;
                    self.vfs_put(pf, xml);
                }
                self.project = Some(proj.clone());
                Ok(serde_json::to_value(proj)?)
            }

            "build.run" => {
                if let Some(p) = params.get("path").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                    self.project = Some(self.open_virtual_project(p)?);
                }
                let proj = self
                    .project
                    .clone()
                    .ok_or_else(|| anyhow!("No project or file specified to build"))?;
                let res = self.build_virtual(&proj)?;
                Ok(serde_json::to_value(res)?)
            }

            "build.artifacts" => Ok(json!({
                "output_name": self.last_output_name,
                "has_artifacts": self.last_elf_bytes.is_some(),
                "hex_text": self.last_hex_text,
                "bin_b64": self.last_bin_bytes.as_deref().map(b64_encode),
                "elf_b64": self.last_elf_bytes.as_deref().map(b64_encode),
            })),

            "debug.start" => {
                let mut build_log = None;
                if let Some(p) = params.get("path").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                    self.project = Some(self.open_virtual_project(p)?);
                }
                if self.last_elf_bytes.is_none() {
                    let proj = self
                        .project
                        .clone()
                        .ok_or_else(|| anyhow!("No project open for debug.start"))?;
                    let br = self.build_virtual(&proj)?;
                    build_log = Some(br.log.clone());
                    if !br.success {
                        return Err(anyhow!("Build failed before debug:\n{}", br.log));
                    }
                }
                let elf_bytes = self
                    .last_elf_bytes
                    .as_deref()
                    .ok_or_else(|| anyhow!("No built ELF image available"))?;
                let label = format!("{}.axf", self.last_output_name);
                let elf = LoadedElf::load_from_bytes(&label, elf_bytes)?;
                let clk = params
                    .get("clock_hz")
                    .and_then(|v| v.as_u64())
                    .map(|x| x as u32)
                    .or_else(|| self.project.as_ref().map(|p| p.options.xtal_hz))
                    .unwrap_or(8_000_000);
                let mut sim = Simulator::new(elf.clone(), clk);

                if let Some(bps) = params.get("breakpoints").and_then(|v| v.as_array()) {
                    for bp in bps {
                        if let (Some(file), Some(line)) = (
                            bp.get("file").and_then(|v| v.as_str()),
                            bp.get("line").and_then(|v| v.as_u64()),
                        ) {
                            if let Some(addr) = elf.addr_for_line(file, line as u32) {
                                let _ = sim.set_breakpoint_addr(
                                    addr,
                                    Some(file.to_string()),
                                    Some(line as u32),
                                );
                            }
                        } else if let Some(addr) = bp.get("addr").and_then(|v| v.as_u64()) {
                            let _ = sim.set_breakpoint_addr(addr as u32, None, None);
                        }
                    }
                }

                let state = sim.state()?;
                let mut exec_lines_by_file: HashMap<String, Vec<u32>> = HashMap::new();
                for l in &elf.lines {
                    exec_lines_by_file
                        .entry(l.file.clone())
                        .or_default()
                        .push(l.line);
                }
                for v in exec_lines_by_file.values_mut() {
                    v.sort_unstable();
                    v.dedup();
                }

                let disasm = elf.disassembly.clone();
                let symbols = elf.symbols.clone();
                self.elf = Some(elf);
                self.sim = Some(sim);

                Ok(json!({
                    "state": state,
                    "disassembly": disasm,
                    "symbols": symbols,
                    "executable_lines": exec_lines_by_file,
                    "build_log": build_log
                }))
            }

            "debug.stop" => {
                self.sim = None;
                self.elf = None;
                Ok(json!({ "stopped": true }))
            }

            "debug.reset" => Ok(serde_json::to_value(self.req_sim()?.reset()?)?),
            "debug.step" => Ok(serde_json::to_value(self.req_sim()?.step_into()?)?),
            "debug.stepOver" => Ok(serde_json::to_value(self.req_sim()?.step_over()?)?),
            "debug.stepOut" => Ok(serde_json::to_value(self.req_sim()?.step_out()?)?),
            "debug.run" => {
                let max_steps = params.get("max_steps").and_then(|v| v.as_u64()).unwrap_or(100_000) as usize;
                let stop_loop = params.get("stop_on_self_loop").and_then(|v| v.as_bool()).unwrap_or(true);
                Ok(serde_json::to_value(self.req_sim()?.run(max_steps, stop_loop)?)?)
            }
            "debug.pause" => Ok(serde_json::to_value(self.req_sim()?.pause()?)?),
            "debug.state" => Ok(serde_json::to_value(self.req_sim()?.state()?)?),

            "debug.runTo" => {
                let addr = if let Some(a) = params.get("addr").and_then(|v| v.as_u64()) {
                    a as u32
                } else if let (Some(f), Some(l)) = (
                    params.get("file").and_then(|v| v.as_str()),
                    params.get("line").and_then(|v| v.as_u64()),
                ) {
                    let elf = self.elf.as_ref().ok_or_else(|| anyhow!("No ELF loaded"))?;
                    elf.addr_for_line(f, l as u32)
                        .ok_or_else(|| anyhow!("No instruction at {}:{}", f, l))?
                } else {
                    return Err(anyhow!("Provide addr or file+line"));
                };
                let max_steps = params.get("max_steps").and_then(|v| v.as_u64()).unwrap_or(100_000) as usize;
                Ok(serde_json::to_value(self.req_sim()?.run_to(addr, max_steps)?)?)
            }

            "debug.setBreakpoint" => {
                let elf = self.elf.as_ref().ok_or_else(|| anyhow!("No active debug session"))?.clone();
                let enabled = params.get("enabled").and_then(|v| v.as_bool());
                let (addr, file, line) = if let Some(a) = params.get("addr").and_then(|v| v.as_u64()) {
                    let le = elf.line_for_addr(a as u32);
                    (a as u32 & !1, le.map(|x| x.file.clone()), le.map(|x| x.line))
                } else if let (Some(f), Some(l)) = (
                    params.get("file").and_then(|v| v.as_str()),
                    params.get("line").and_then(|v| v.as_u64()),
                ) {
                    let a = elf
                        .addr_for_line(f, l as u32)
                        .ok_or_else(|| anyhow!("No instruction mapped to {}:{}", f, l))?;
                    (a, Some(f.to_string()), Some(l as u32))
                } else if let Some(spec) = params.get("spec").and_then(|v| v.as_str()) {
                    resolve_bp_spec(&elf, spec)?
                } else {
                    return Err(anyhow!("Provide addr, file+line, or spec"));
                };

                let t = self.req_sim()?;
                let st = t.state()?;
                let exists = st.breakpoints.iter().any(|b| b.addr == addr);
                let bps = match enabled {
                    Some(true) => t.set_breakpoint_addr(addr, file, line)?,
                    Some(false) => t.remove_breakpoint_addr(addr)?,
                    None => {
                        if exists {
                            t.remove_breakpoint_addr(addr)?
                        } else {
                            t.set_breakpoint_addr(addr, file, line)?
                        }
                    }
                };
                Ok(json!({ "breakpoints": bps }))
            }

            "debug.clearBreakpoints" => {
                self.req_sim()?.clear_breakpoints()?;
                Ok(json!({ "breakpoints": [] }))
            }

            "debug.disassembly" => {
                let around = params.get("around_addr").and_then(|v| v.as_u64()).map(|x| x as u32);
                let count = params.get("count").and_then(|v| v.as_u64()).unwrap_or(200) as usize;
                let lines = self.req_sim()?.disassembly(around, count)?;
                Ok(serde_json::to_value(lines)?)
            }

            "debug.memoryRead" => {
                let addr = match params.get("addr") {
                    Some(Value::Number(n)) => n.as_u64().unwrap_or(0x2000_0000) as u32,
                    Some(Value::String(s)) => parse_num_u32(s).ok_or_else(|| anyhow!("Bad memory address '{}'", s))?,
                    _ => 0x2000_0000,
                };
                let len = params.get("len").and_then(|v| v.as_u64()).unwrap_or(256).min(4096) as usize;
                let res = self.req_sim()?.read_memory(addr, len)?;
                Ok(serde_json::to_value(res)?)
            }

            "debug.memoryWrite" => {
                let addr = match params.get("addr") {
                    Some(Value::Number(n)) => n.as_u64().unwrap_or(0) as u32,
                    Some(Value::String(s)) => parse_num_u32(s).ok_or_else(|| anyhow!("Bad address '{}'", s))?,
                    _ => return Err(anyhow!("Missing addr")),
                };
                let data: Vec<u8> = if let Some(arr) = params.get("bytes").and_then(|v| v.as_array()) {
                    arr.iter().filter_map(|v| v.as_u64().map(|x| (x & 0xFF) as u8)).collect()
                } else if let Some(hex) = params.get("hex").and_then(|v| v.as_str()) {
                    hex.split_whitespace()
                        .filter_map(|tok| u8::from_str_radix(tok.trim_start_matches("0x"), 16).ok())
                        .collect()
                } else {
                    return Err(anyhow!("Missing bytes or hex"));
                };
                self.req_sim()?.write_memory(addr, &data)?;
                Ok(json!({ "ok": true }))
            }

            "debug.setRegister" => {
                let name = req_str(&params, "name")?;
                let value = match params.get("value") {
                    Some(Value::Number(n)) => n.as_u64().unwrap_or(0) as u32,
                    Some(Value::String(s)) => parse_num_u32(s).ok_or_else(|| anyhow!("Bad value '{}'", s))?,
                    _ => return Err(anyhow!("Missing value")),
                };
                let st = self.req_sim()?.set_register(&name, value)?;
                Ok(serde_json::to_value(st)?)
            }

            "debug.evalWatch" => {
                let expr = req_str(&params, "expr")?;
                let res = self.req_sim()?.eval_watch(&expr)?;
                Ok(serde_json::to_value(res)?)
            }

            "debug.command" => {
                let cmd = req_str(&params, "command")?;
                let elf = self.elf.as_ref().ok_or_else(|| anyhow!("No active debug session"))?.clone();
                let sim = self.sim.as_mut().ok_or_else(|| anyhow!("No active debug session"))?;
                let output = execute_debug_command(sim, &elf, &cmd)?;
                let state = sim.state()?;
                Ok(json!({ "output": output, "state": state }))
            }

            "debug.peripherals" => {
                let snap = self.req_sim()?.peripherals()?;
                Ok(serde_json::to_value(snap)?)
            }

            "debug.setGpioPin" => {
                let port = req_str(&params, "port")?;
                let pin = params.get("pin").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
                let high = params.get("high").and_then(|v| v.as_bool()).unwrap_or(false);
                self.req_sim()?.set_gpio_pin(&port, pin, high)?;
                let snap = self.req_sim()?.peripherals()?;
                Ok(serde_json::to_value(snap)?)
            }

            "debug.uartSend" => {
                let text = req_str(&params, "text")?;
                self.req_sim()?.uart_send(&text)?;
                Ok(json!({ "ok": true }))
            }

            "debug.logicAdd" => {
                let signal = req_str(&params, "signal")?;
                let ch = self.req_sim()?.logic_add(&signal)?;
                Ok(serde_json::to_value(ch)?)
            }

            "debug.logicRemove" => {
                let signal = req_str(&params, "signal")?;
                let ch = self.req_sim()?.logic_remove(&signal)?;
                Ok(serde_json::to_value(ch)?)
            }

            "debug.logicSamples" => {
                let ch = self.req_sim()?.logic_samples()?;
                Ok(serde_json::to_value(ch)?)
            }

            "pack.list" => Ok(json!([
                { "vendor": "Keil", "name": "STM32F1xx_DFP (Built-in)", "version": "2.4.1", "id": "Keil.STM32F1xx_DFP.2.4.1" },
                { "vendor": "ARM", "name": "CMSIS (Cortex-M3 Simulator)", "version": "5.9.0", "id": "ARM.CMSIS.5.9.0" }
            ])),

            "pack.add" => Ok(json!({
                "ok": true,
                "log": "STM32F1xx + Cortex-M3 simulation & assembler support is built directly into the offline WebAssembly engine."
            })),

            _ => Err(anyhow!("Unknown method '{}'", method)),
        }
    }

    fn req_sim(&mut self) -> Result<&mut Simulator> {
        self.sim
            .as_mut()
            .ok_or_else(|| anyhow!("No active debug session. Start debugging first (Ctrl+F5)."))
    }
}

fn req_str(params: &Value, key: &str) -> Result<String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("Missing string parameter '{}'", key))
}

fn b64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = *chunk.get(1).unwrap_or(&0) as usize;
        let b2 = *chunk.get(2).unwrap_or(&0) as usize;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[(n >> 18) & 63] as char);
        out.push(TABLE[(n >> 12) & 63] as char);
        out.push(if chunk.len() > 1 { TABLE[(n >> 6) & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[n & 63] as char } else { '=' });
    }
    out
}

thread_local! {
    static STATE: RefCell<WasmState> = RefCell::new(WasmState::new());
    static OUT_BUF: RefCell<Vec<u8>> = RefCell::new(Vec::new());
}

#[no_mangle]
pub extern "C" fn uv_alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

#[no_mangle]
pub unsafe extern "C" fn uv_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() && len > 0 {
        let _ = Vec::from_raw_parts(ptr, 0, len);
    }
}

#[no_mangle]
pub extern "C" fn uv_out_ptr() -> *const u8 {
    OUT_BUF.with(|b| b.borrow().as_ptr())
}

#[no_mangle]
pub unsafe extern "C" fn uv_rpc(req_ptr: *const u8, req_len: usize) -> u32 {
    let req_bytes = if req_ptr.is_null() || req_len == 0 {
        &[][..]
    } else {
        slice::from_raw_parts(req_ptr, req_len)
    };
    let resp = match std::str::from_utf8(req_bytes) {
        Ok(req_str) => match serde_json::from_str::<RpcRequest>(req_str) {
            Ok(req) => STATE.with(|st| match st.borrow_mut().handle(&req.method, req.params) {
                Ok(res) => WasmRpcResponse {
                    result: Some(res),
                    error: None,
                },
                Err(e) => WasmRpcResponse {
                    result: None,
                    error: Some(e.to_string()),
                },
            }),
            Err(e) => WasmRpcResponse {
                result: None,
                error: Some(format!("JSON parse error: {}", e)),
            },
        },
        Err(e) => WasmRpcResponse {
            result: None,
            error: Some(format!("UTF-8 error: {}", e)),
        },
    };
    let bytes = serde_json::to_vec(&resp).unwrap_or_else(|e| {
        format!("{{\"error\":\"Serialize error: {}\"}}", e).into_bytes()
    });
    OUT_BUF.with(|b| {
        let mut guard = b.borrow_mut();
        *guard = bytes;
        guard.len() as u32
    })
}
