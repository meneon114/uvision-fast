use crate::builder::{build_project, build_project_with_mode, ToolchainMode};
use crate::elf::LoadedElf;
use crate::hardware::{flash_via_openocd, HardwareTarget};
use crate::project::{DebugBackendMode, TargetOptions, UvProject};
use crate::repl::{execute_debug_command, resolve_bp_spec};
use crate::sim::{parse_num_u32, Simulator};
use crate::target::DebugTarget;
use crate::toolchain::ToolchainInfo;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct RpcRequest {
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct RpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Debug, Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

struct ServerState {
    tc: ToolchainInfo,
    project: Option<UvProject>,
    elf: Option<LoadedElf>,
    target: Option<Box<dyn DebugTarget>>,
}

impl ServerState {
    fn new(keil_override: Option<&str>, openocd_override: Option<&str>) -> Self {
        Self {
            tc: ToolchainInfo::discover(keil_override, openocd_override),
            project: None,
            elf: None,
            target: None,
        }
    }

    fn handle(&mut self, method: &str, params: Value) -> Result<Value> {
        match method {
            "toolchain.info" => Ok(serde_json::to_value(&self.tc)?),

            "project.open" => {
                let path = req_str(&params, "path")?;
                let proj = UvProject::open(Path::new(&path))?;
                let val = serde_json::to_value(&proj)?;
                self.project = Some(proj);
                Ok(val)
            }

            "project.new" => {
                let dir = req_str(&params, "dir")?;
                let name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("LabProject");
                let device = params
                    .get("device")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("STM32F103C8");
                let template = params
                    .get("template")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("asm");
                let proj = UvProject::create_new(Path::new(&dir), name, device, template)?;
                let val = serde_json::to_value(&proj)?;
                self.project = Some(proj);
                Ok(val)
            }

            "project.addFile" => {
                if let Some(pp) = params.get("project_path").and_then(|v| v.as_str()) {
                    if self.project.is_none() {
                        self.project = Some(UvProject::open(Path::new(pp))?);
                    }
                }
                let file_path = req_str(&params, "file_path")?;
                let template = params.get("template").and_then(|v| v.as_str());
                let proj = self
                    .project
                    .as_mut()
                    .ok_or_else(|| anyhow!("No project currently open"))?;
                let added = proj.add_file(Path::new(&file_path), template)?;
                Ok(json!({
                    "added": added,
                    "project": proj
                }))
            }

            "project.saveOptions" => {
                let opts: TargetOptions = serde_json::from_value(
                    params.get("options").cloned().unwrap_or(params.clone()),
                )?;
                let proj = self
                    .project
                    .as_mut()
                    .ok_or_else(|| anyhow!("No project currently open"))?;
                proj.options = opts;
                proj.save()?;
                Ok(serde_json::to_value(proj)?)
            }

            "build.run" => {
                if let Some(p) = params.get("path").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                    let opened = UvProject::open(Path::new(p))?;
                    self.project = Some(opened);
                }
                let rebuild = params.get("rebuild").and_then(|v| v.as_bool()).unwrap_or(false);
                let mode = params
                    .get("toolchain")
                    .and_then(|v| v.as_str())
                    .map(ToolchainMode::parse)
                    .unwrap_or(ToolchainMode::Auto);
                let proj = self
                    .project
                    .as_ref()
                    .ok_or_else(|| anyhow!("No project or file specified to build"))?;
                let res = build_project_with_mode(proj, &self.tc, rebuild, mode)?;
                Ok(serde_json::to_value(res)?)
            }

            "debug.start" => {
                let mut build_log = None;
                let axf_path: PathBuf = if let Some(axf) = params.get("axf").and_then(|v| v.as_str()) {
                    PathBuf::from(axf)
                } else {
                    if let Some(p) = params.get("path").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                        let p_path = Path::new(p);
                        if p_path.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("axf")).unwrap_or(false) {
                            p_path.to_path_buf()
                        } else {
                            self.project = Some(UvProject::open(p_path)?);
                            let proj = self.project.as_ref().unwrap();
                            let ap = proj.axf_path();
                            if !ap.exists() {
                                let br = build_project(proj, &self.tc, false)?;
                                build_log = Some(br.log.clone());
                                if !br.success {
                                    return Err(anyhow!("Build failed before debug:\n{}", br.log));
                                }
                            }
                            ap
                        }
                    } else {
                        let proj = self
                            .project
                            .as_ref()
                            .ok_or_else(|| anyhow!("No project open for debug.start"))?;
                        let ap = proj.axf_path();
                        if !ap.exists() {
                            let br = build_project(proj, &self.tc, false)?;
                            build_log = Some(br.log.clone());
                            if !br.success {
                                return Err(anyhow!("Build failed before debug:\n{}", br.log));
                            }
                        }
                        ap
                    }
                };

                let elf = LoadedElf::load(&axf_path, &self.tc)?;
                let mode_str = params
                    .get("mode")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_ascii_lowercase())
                    .unwrap_or_else(|| match self.project.as_ref().map(|p| &p.options.debug_mode) {
                        Some(DebugBackendMode::Hardware) => "hardware".to_string(),
                        _ => "simulator".to_string(),
                    });

                let mut target: Box<dyn DebugTarget> = if mode_str == "hardware" {
                    let iface = params
                        .get("interface")
                        .and_then(|v| v.as_str())
                        .or_else(|| self.project.as_ref().map(|p| p.options.hw_interface.as_str()))
                        .unwrap_or("stlink");
                    let tgt = params
                        .get("target")
                        .and_then(|v| v.as_str())
                        .or_else(|| self.project.as_ref().map(|p| p.options.hw_target.as_str()))
                        .unwrap_or("stm32f1x");
                    let prog = params.get("program_first").and_then(|v| v.as_bool()).unwrap_or(true);
                    Box::new(HardwareTarget::connect(&self.tc, elf.clone(), iface, tgt, prog)?)
                } else {
                    let clk = params
                        .get("clock_hz")
                        .and_then(|v| v.as_u64())
                        .map(|x| x as u32)
                        .or_else(|| self.project.as_ref().map(|p| p.options.xtal_hz))
                        .unwrap_or(8_000_000);
                    let device = self.project.as_ref().map(|p| p.options.device.clone()).unwrap_or_else(|| "STM32F103C8".into());
                    Box::new(Simulator::new_for_device(elf.clone(), clk, &device))
                };

                // Apply initial breakpoints if passed
                if let Some(bps) = params.get("breakpoints").and_then(|v| v.as_array()) {
                    for bp in bps {
                        if let (Some(file), Some(line)) = (
                            bp.get("file").and_then(|v| v.as_str()),
                            bp.get("line").and_then(|v| v.as_u64()),
                        ) {
                            if let Some(addr) = elf.addr_for_line(file, line as u32) {
                                let _ = target.set_breakpoint_addr(addr, Some(file.to_string()), Some(line as u32));
                            }
                        } else if let Some(addr) = bp.get("addr").and_then(|v| v.as_u64()) {
                            let _ = target.set_breakpoint_addr(addr as u32, None, None);
                        }
                    }
                }

                let state = target.state()?;
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
                self.target = Some(target);

                Ok(json!({
                    "state": state,
                    "disassembly": disasm,
                    "symbols": symbols,
                    "executable_lines": exec_lines_by_file,
                    "build_log": build_log
                }))
            }

            "debug.stop" => {
                self.target = None;
                self.elf = None;
                Ok(json!({ "stopped": true }))
            }

            "debug.reset" => Ok(serde_json::to_value(self.req_target()?.reset()?)?),
            "debug.step" => Ok(serde_json::to_value(self.req_target()?.step_into()?)?),
            "debug.stepOver" => Ok(serde_json::to_value(self.req_target()?.step_over()?)?),
            "debug.stepOut" => Ok(serde_json::to_value(self.req_target()?.step_out()?)?),
            "debug.run" => {
                let max_steps = params.get("max_steps").and_then(|v| v.as_u64()).unwrap_or(100_000) as usize;
                let stop_loop = params.get("stop_on_self_loop").and_then(|v| v.as_bool()).unwrap_or(true);
                Ok(serde_json::to_value(self.req_target()?.run(max_steps, stop_loop)?)?)
            }
            "debug.pause" => Ok(serde_json::to_value(self.req_target()?.pause()?)?),
            "debug.state" => Ok(serde_json::to_value(self.req_target()?.state()?)?),

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
                Ok(serde_json::to_value(self.req_target()?.run_to(addr, max_steps)?)?)
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

                let t = self.req_target()?;
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
                self.req_target()?.clear_breakpoints()?;
                Ok(json!({ "breakpoints": [] }))
            }

            "debug.disassembly" => {
                let around = params.get("around_addr").and_then(|v| v.as_u64()).map(|x| x as u32);
                let count = params.get("count").and_then(|v| v.as_u64()).unwrap_or(200) as usize;
                let lines = self.req_target()?.disassembly(around, count)?;
                Ok(serde_json::to_value(lines)?)
            }

            "debug.memoryRead" => {
                let addr = match params.get("addr") {
                    Some(Value::Number(n)) => n.as_u64().unwrap_or(0x2000_0000) as u32,
                    Some(Value::String(s)) => parse_num_u32(s).ok_or_else(|| anyhow!("Bad memory address '{}'", s))?,
                    _ => 0x2000_0000,
                };
                let len = params.get("len").and_then(|v| v.as_u64()).unwrap_or(256).min(4096) as usize;
                let res = self.req_target()?.read_memory(addr, len)?;
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
                self.req_target()?.write_memory(addr, &data)?;
                Ok(json!({ "ok": true }))
            }

            "debug.setRegister" => {
                let name = req_str(&params, "name")?;
                let value = match params.get("value") {
                    Some(Value::Number(n)) => n.as_u64().unwrap_or(0) as u32,
                    Some(Value::String(s)) => parse_num_u32(s).ok_or_else(|| anyhow!("Bad value '{}'", s))?,
                    _ => return Err(anyhow!("Missing value")),
                };
                let st = self.req_target()?.set_register(&name, value)?;
                Ok(serde_json::to_value(st)?)
            }

            "debug.evalWatch" => {
                let expr = req_str(&params, "expr")?;
                let res = self.req_target()?.eval_watch(&expr)?;
                Ok(serde_json::to_value(res)?)
            }

            "debug.command" => {
                let cmd = req_str(&params, "command")?;
                let elf = self.elf.as_ref().ok_or_else(|| anyhow!("No active debug session"))?.clone();
                let t = self.target.as_deref_mut().ok_or_else(|| anyhow!("No active debug session"))?;
                let output = execute_debug_command(t, &elf, &cmd)?;
                let state = t.state()?;
                Ok(json!({ "output": output, "state": state }))
            }

            "debug.peripherals" => {
                let snap = self.req_target()?.peripherals()?;
                Ok(serde_json::to_value(snap)?)
            }

            "debug.setGpioPin" => {
                let port = req_str(&params, "port")?;
                let pin = params.get("pin").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
                let high = params.get("high").and_then(|v| v.as_bool()).unwrap_or(false);
                self.req_target()?.set_gpio_pin(&port, pin, high)?;
                let snap = self.req_target()?.peripherals()?;
                Ok(serde_json::to_value(snap)?)
            }

            "debug.connectPins" => {
                let a = req_str(&params, "a")?;
                let b = req_str(&params, "b")?;
                let connected = params.get("connected").and_then(|v| v.as_bool()).unwrap_or(true);
                self.req_target()?.connect_pins(&a, &b, connected)?;
                let snap = self.req_target()?.peripherals()?;
                Ok(serde_json::to_value(snap)?)
            }

            "debug.uartSend" => {
                let text = req_str(&params, "text")?;
                self.req_target()?.uart_send(&text)?;
                Ok(json!({ "ok": true }))
            }

            "debug.logicAdd" => {
                let signal = req_str(&params, "signal")?;
                let ch = self.req_target()?.logic_add(&signal)?;
                Ok(serde_json::to_value(ch)?)
            }

            "debug.logicRemove" => {
                let signal = req_str(&params, "signal")?;
                let ch = self.req_target()?.logic_remove(&signal)?;
                Ok(serde_json::to_value(ch)?)
            }

            "debug.logicSamples" => {
                let ch = self.req_target()?.logic_samples()?;
                Ok(serde_json::to_value(ch)?)
            }

            "flash.program" => {
                if let Some(p) = params.get("path").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                    self.project = Some(UvProject::open(Path::new(p))?);
                }
                let proj = self
                    .project
                    .as_ref()
                    .ok_or_else(|| anyhow!("No project open to flash"))?;
                let fw = if proj.hex_path().exists() {
                    proj.hex_path()
                } else if proj.axf_path().exists() {
                    proj.axf_path()
                } else {
                    let br = build_project(proj, &self.tc, false)?;
                    if !br.success {
                        return Err(anyhow!("Build failed before flash:\n{}", br.log));
                    }
                    proj.hex_path()
                };
                let iface = params
                    .get("interface")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&proj.options.hw_interface);
                let tgt = params
                    .get("target")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&proj.options.hw_target);
                let out = flash_via_openocd(&self.tc, &fw, iface, tgt)?;
                Ok(json!({ "ok": true, "log": out }))
            }

            "pack.list" => Ok(serde_json::to_value(&self.tc.packs)?),

            "pack.add" => {
                let spec = req_str(&params, "spec")?;
                let log = self.tc.pack_add(&spec)?;
                self.tc = ToolchainInfo::discover(
                    self.tc.keil_root.as_deref(),
                    self.tc.openocd.as_deref(),
                );
                Ok(json!({ "ok": true, "log": log, "packs": self.tc.packs }))
            }

            _ => Err(anyhow!("Unknown method '{}'", method)),
        }
    }

    fn req_target(&mut self) -> Result<&mut (dyn DebugTarget + 'static)> {
        self.target
            .as_deref_mut()
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

pub fn run_json_rpc_server(keil_override: Option<&str>, openocd_override: Option<&str>) -> Result<()> {
    let mut state = ServerState::new(keil_override, openocd_override);
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line_res in stdin.lock().lines() {
        let line = line_res?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let resp = match serde_json::from_str::<RpcRequest>(trimmed) {
            Ok(req) => match state.handle(&req.method, req.params) {
                Ok(res) => RpcResponse {
                    jsonrpc: "2.0",
                    id: req.id,
                    result: Some(res),
                    error: None,
                },
                Err(e) => RpcResponse {
                    jsonrpc: "2.0",
                    id: req.id,
                    result: None,
                    error: Some(RpcError {
                        code: -32000,
                        message: e.to_string(),
                    }),
                },
            },
            Err(e) => RpcResponse {
                jsonrpc: "2.0",
                id: Value::Null,
                result: None,
                error: Some(RpcError {
                    code: -32700,
                    message: format!("Parse error: {}", e),
                }),
            },
        };
        let out_line = serde_json::to_string(&resp)?;
        writeln!(stdout, "{}", out_line)?;
        stdout.flush()?;
    }
    Ok(())
}
