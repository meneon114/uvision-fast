use crate::elf::{DisasmLine, LoadedElf};
use crate::sim::peripherals::{LogicChannel, PeripheralSnapshot};
use crate::sim::reg_label;
use crate::target::{
    BreakpointInfo, CallFrame, DebugState, DebugTarget, MemoryReadResult, RegisterSnapshot,
    WatchEval,
};
use crate::toolchain::ToolchainInfo;
use anyhow::{anyhow, Context, Result};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub fn flash_via_openocd(
    tc: &ToolchainInfo,
    firmware_path: &Path,
    interface_cfg: &str,
    target_cfg: &str,
) -> Result<String> {
    let ocd = tc
        .openocd
        .as_deref()
        .ok_or_else(|| anyhow!("openocd.exe not found. Ensure Software\\openocd\\bin is present."))?;
    let mut cmd = Command::new(ocd);
    if let Some(scripts) = tc.openocd_scripts.as_deref() {
        cmd.arg("-s").arg(scripts);
    }
    let iface = normalize_cfg(interface_cfg, "interface");
    let tgt = normalize_cfg(target_cfg, "target");
    let fw_fwd = firmware_path.to_string_lossy().replace('\\', "/");

    cmd.args([
        "-f",
        &iface,
        "-f",
        &tgt,
        "-c",
        &format!("program \"{}\" verify reset exit", fw_fwd),
    ]);

    let out = cmd.output().context("Failed executing openocd.exe")?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() {
        return Err(anyhow!("OpenOCD flash failed:\n{}", combined.trim()));
    }
    Ok(combined)
}

fn normalize_cfg(name: &str, folder: &str) -> String {
    let trimmed = name.trim();
    if trimmed.ends_with(".cfg") || trimmed.contains('/') || trimmed.contains('\\') {
        trimmed.replace('\\', "/")
    } else {
        format!("{}/{}.cfg", folder, trimmed)
    }
}

pub struct HardwareTarget {
    pub elf: LoadedElf,
    child: Child,
    stream: TcpStream,
    breakpoints: BTreeMap<u32, BreakpointInfo>,
    prev_r: [u32; 16],
    prev_xpsr: u32,
}

impl HardwareTarget {
    pub fn connect(
        tc: &ToolchainInfo,
        elf: LoadedElf,
        interface_cfg: &str,
        target_cfg: &str,
        program_first: bool,
    ) -> Result<Self> {
        let ocd = tc
            .openocd
            .as_deref()
            .ok_or_else(|| anyhow!("openocd.exe not found"))?;
        let gdb_port = pick_free_port()?;

        let mut cmd = Command::new(ocd);
        if let Some(scripts) = tc.openocd_scripts.as_deref() {
            cmd.arg("-s").arg(scripts);
        }
        let iface = normalize_cfg(interface_cfg, "interface");
        let tgt = normalize_cfg(target_cfg, "target");
        let fw_fwd = elf.path.replace('\\', "/");

        let init_cmd = if program_first {
            format!(
                "gdb_port {}; tcl_port disabled; telnet_port disabled; init; reset halt; program \"{}\" verify; reset halt",
                gdb_port, fw_fwd
            )
        } else {
            format!(
                "gdb_port {}; tcl_port disabled; telnet_port disabled; init; reset halt",
                gdb_port
            )
        };

        cmd.args(["-f", &iface, "-f", &tgt, "-c", &init_cmd])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().context("Failed starting OpenOCD daemon")?;

        let deadline = Instant::now() + Duration::from_secs(5);
        let stream = loop {
            if let Ok(Some(status)) = child.try_wait() {
                let mut err = String::new();
                if let Some(mut se) = child.stderr.take() {
                    let _ = se.read_to_string(&mut err);
                }
                return Err(anyhow!(
                    "OpenOCD exited early ({}): {}",
                    status,
                    err.trim()
                ));
            }
            match TcpStream::connect(("127.0.0.1", gdb_port)) {
                Ok(s) => break s,
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(80));
                }
                Err(e) => {
                    let _ = child.kill();
                    return Err(anyhow!("Timed out connecting to OpenOCD GDB port {}: {}", gdb_port, e));
                }
            }
        };

        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;

        let mut hw = Self {
            elf,
            child,
            stream,
            breakpoints: BTreeMap::new(),
            prev_r: [0; 16],
            prev_xpsr: 0x0100_0000,
        };

        let _ = hw.stream.write_all(b"+");
        let _ = hw.send_packet("qSupported:hwbreak+;swbreak+");
        let (r, xpsr) = hw.read_all_regs()?;
        hw.prev_r = r;
        hw.prev_xpsr = xpsr;
        Ok(hw)
    }

    fn send_packet(&mut self, payload: &str) -> Result<String> {
        let pkt = format_gdb_packet(payload);
        self.stream.write_all(pkt.as_bytes())?;
        self.stream.flush()?;
        self.read_packet()
    }

    fn read_packet(&mut self) -> Result<String> {
        let mut buf = [0u8; 1];
        // Wait for '$'
        loop {
            self.stream.read_exact(&mut buf)?;
            if buf[0] == b'$' {
                break;
            }
        }
        let mut data = Vec::new();
        loop {
            self.stream.read_exact(&mut buf)?;
            if buf[0] == b'#' {
                break;
            }
            data.push(buf[0]);
        }
        let mut csum = [0u8; 2];
        self.stream.read_exact(&mut csum)?;
        let _ = self.stream.write_all(b"+");
        Ok(String::from_utf8_lossy(&data).into_owned())
    }

    fn read_all_regs(&mut self) -> Result<([u32; 16], u32)> {
        let resp = self.send_packet("g")?;
        if resp.starts_with('E') || resp.len() < 16 * 8 {
            return Err(anyhow!("Unexpected GDB 'g' response: {}", resp));
        }
        let mut r = [0u32; 16];
        for i in 0..16 {
            r[i] = decode_le_u32(&resp[i * 8..(i + 1) * 8]).unwrap_or(0);
        }
        // Cortex-M GDB xPSR is typically register 25 (offset 25*8) or 16
        let xpsr = if resp.len() >= 26 * 8 {
            decode_le_u32(&resp[25 * 8..26 * 8]).unwrap_or(0x0100_0000)
        } else if resp.len() >= 17 * 8 {
            decode_le_u32(&resp[16 * 8..17 * 8]).unwrap_or(0x0100_0000)
        } else {
            0x0100_0000
        };
        Ok((r, xpsr))
    }

    fn read_reg_idx(&mut self, reg_num: usize) -> Result<u32> {
        let resp = self.send_packet(&format!("p{:x}", reg_num))?;
        decode_le_u32(&resp).ok_or_else(|| anyhow!("Bad register response: {}", resp))
    }

    fn build_state(&mut self, reason: &str) -> Result<DebugState> {
        let (r, xpsr) = self.read_all_regs()?;
        let msp = self.read_reg_idx(17).unwrap_or(r[13]);
        let psp = self.read_reg_idx(18).unwrap_or(0);
        let primask = self.read_reg_idx(19).unwrap_or(0) & 1;

        let mut changed = Vec::new();
        for i in 0..16 {
            if r[i] != self.prev_r[i] {
                changed.push(reg_label(i).to_string());
            }
        }
        if xpsr != self.prev_xpsr {
            changed.push("xPSR".to_string());
        }
        self.prev_r = r;
        self.prev_xpsr = xpsr;

        let pc = r[15] & !1;
        let le = self.elf.line_for_addr(pc);
        let sym = self
            .elf
            .symbol_for_addr(pc)
            .map(|(s, off)| if off == 0 { s.name.clone() } else { format!("{}+0x{:X}", s.name, off) })
            .unwrap_or_else(|| format!("0x{:08X}", pc));

        Ok(DebugState {
            backend: "hardware".to_string(),
            running: false,
            halt_reason: reason.to_string(),
            pc,
            current_file: le.map(|l| l.file.clone()),
            current_line: le.map(|l| l.line),
            registers: RegisterSnapshot {
                r,
                msp,
                psp,
                xpsr,
                primask,
                control: 0,
                cycles: 0,
                time_sec: 0.0,
                changed_regs: changed,
                n: (xpsr >> 31) & 1 != 0,
                z: (xpsr >> 30) & 1 != 0,
                c: (xpsr >> 29) & 1 != 0,
                v: (xpsr >> 28) & 1 != 0,
                q: (xpsr >> 27) & 1 != 0,
                t: (xpsr >> 24) & 1 != 0,
                mode: "Hardware(SWD)".to_string(),
            },
            call_stack: vec![CallFrame {
                index: 0,
                addr: pc,
                symbol: sym,
                file: le.map(|l| l.file.clone()),
                line: le.map(|l| l.line),
            }],
            breakpoints: self.breakpoints.values().cloned().collect(),
            uart1_output: String::new(),
        })
    }

    fn monitor_cmd(&mut self, cmd: &str) -> Result<String> {
        let hex: String = cmd.as_bytes().iter().map(|b| format!("{:02x}", b)).collect();
        self.send_packet(&format!("qRcmd,{}", hex))
    }
}

impl Drop for HardwareTarget {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl DebugTarget for HardwareTarget {
    fn backend_name(&self) -> &str {
        "hardware"
    }

    fn reset(&mut self) -> Result<DebugState> {
        let _ = self.monitor_cmd("reset halt");
        self.build_state("Reset Halt")
    }

    fn step_into(&mut self) -> Result<DebugState> {
        let resp = self.send_packet("s")?;
        self.build_state(&format!("Step ({})", resp))
    }

    fn step_over(&mut self) -> Result<DebugState> {
        self.step_into()
    }

    fn step_out(&mut self) -> Result<DebugState> {
        let (r, _) = self.read_all_regs()?;
        let lr = r[14] & !1;
        if lr != 0 && lr != 0xFFFF_FFFE {
            return self.run_to(lr, 1);
        }
        self.step_into()
    }

    fn run(&mut self, _max_steps: usize, _stop_on_self_loop: bool) -> Result<DebugState> {
        let resp = self.send_packet("c")?;
        self.build_state(&format!("Halted ({})", resp))
    }

    fn run_to(&mut self, addr: u32, _max_steps: usize) -> Result<DebugState> {
        let clean = addr & !1;
        let _ = self.send_packet(&format!("Z1,{:x},2", clean));
        let resp = self.send_packet("c")?;
        let _ = self.send_packet(&format!("z1,{:x},2", clean));
        self.build_state(&format!("RunTo ({})", resp))
    }

    fn pause(&mut self) -> Result<DebugState> {
        self.stream.write_all(&[0x03])?;
        self.stream.flush()?;
        let resp = self.read_packet()?;
        self.build_state(&format!("Paused ({})", resp))
    }

    fn state(&mut self) -> Result<DebugState> {
        self.build_state("Ready")
    }

    fn set_breakpoint_addr(
        &mut self,
        addr: u32,
        file: Option<String>,
        line: Option<u32>,
    ) -> Result<Vec<BreakpointInfo>> {
        let clean = addr & !1;
        let _ = self.send_packet(&format!("Z1,{:x},2", clean));
        self.breakpoints.insert(
            clean,
            BreakpointInfo {
                addr: clean,
                file,
                line,
                enabled: true,
            },
        );
        Ok(self.breakpoints.values().cloned().collect())
    }

    fn remove_breakpoint_addr(&mut self, addr: u32) -> Result<Vec<BreakpointInfo>> {
        let clean = addr & !1;
        let _ = self.send_packet(&format!("z1,{:x},2", clean));
        self.breakpoints.remove(&clean);
        Ok(self.breakpoints.values().cloned().collect())
    }

    fn clear_breakpoints(&mut self) -> Result<()> {
        let addrs: Vec<u32> = self.breakpoints.keys().copied().collect();
        for a in addrs {
            let _ = self.send_packet(&format!("z1,{:x},2", a));
        }
        self.breakpoints.clear();
        Ok(())
    }

    fn read_memory(&mut self, addr: u32, len: usize) -> Result<MemoryReadResult> {
        let resp = self.send_packet(&format!("m{:x},{:x}", addr, len))?;
        if resp.starts_with('E') {
            return Err(anyhow!("GDB memory read failed: {}", resp));
        }
        let mut bytes = Vec::with_capacity(len);
        let raw = resp.as_bytes();
        let mut i = 0;
        while i + 1 < raw.len() {
            if let Ok(b) = u8::from_str_radix(&resp[i..i + 2], 16) {
                bytes.push(b);
            }
            i += 2;
        }
        Ok(MemoryReadResult {
            addr,
            bytes,
            changed_offsets: Vec::new(),
        })
    }

    fn write_memory(&mut self, addr: u32, data: &[u8]) -> Result<()> {
        let hex: String = data.iter().map(|b| format!("{:02x}", b)).collect();
        let resp = self.send_packet(&format!("M{:x},{:x}:{}", addr, data.len(), hex))?;
        if resp != "OK" {
            return Err(anyhow!("GDB memory write failed: {}", resp));
        }
        Ok(())
    }

    fn set_register(&mut self, name: &str, value: u32) -> Result<DebugState> {
        let u = name.trim().to_ascii_uppercase();
        let idx = crate::sim::parse_reg_name(&u)
            .ok_or_else(|| anyhow!("Unsupported hardware register '{}'", name))?;
        let le = encode_le_u32(value);
        let resp = self.send_packet(&format!("P{:x}={}", idx, le))?;
        if resp != "OK" {
            return Err(anyhow!("GDB set register failed: {}", resp));
        }
        self.build_state(&format!("Set {}", name))
    }

    fn eval_watch(&mut self, expr: &str) -> Result<WatchEval> {
        let trimmed = expr.trim();
        let u = trimmed.to_ascii_uppercase();
        if let Some(idx) = crate::sim::parse_reg_name(&u) {
            let (r, _) = self.read_all_regs()?;
            let v = r[idx];
            return Ok(WatchEval {
                expr: trimmed.to_string(),
                value_hex: format!("0x{:08X}", v),
                value_dec: format!("{}", v),
                kind: "register".to_string(),
            });
        }
        if let Some(addr) = crate::sim::parse_num_u32(trimmed.trim_start_matches('*')) {
            let mem = self.read_memory(addr, 4)?;
            let mut arr = [0u8; 4];
            for (i, &b) in mem.bytes.iter().take(4).enumerate() {
                arr[i] = b;
            }
            let v = u32::from_le_bytes(arr);
            return Ok(WatchEval {
                expr: trimmed.to_string(),
                value_hex: format!("0x{:08X}", v),
                value_dec: format!("{}", v),
                kind: "memory32".to_string(),
            });
        }
        Err(anyhow!("Cannot evaluate hardware watch '{}'", expr))
    }

    fn disassembly(&mut self, _around_addr: Option<u32>, _count: usize) -> Result<Vec<DisasmLine>> {
        Ok(self.elf.disassembly.clone())
    }

    fn peripherals(&mut self) -> Result<Option<PeripheralSnapshot>> {
        Ok(None)
    }

    fn set_gpio_pin(&mut self, _port: &str, _pin: u8, _high: bool) -> Result<()> {
        Err(anyhow!("External GPIO pin simulation only applies in Simulator mode"))
    }

    fn uart_send(&mut self, _text: &str) -> Result<()> {
        Err(anyhow!("Simulated UART console only applies in Simulator mode"))
    }

    fn logic_add(&mut self, _signal: &str) -> Result<Vec<LogicChannel>> {
        Ok(Vec::new())
    }

    fn logic_remove(&mut self, _signal: &str) -> Result<Vec<LogicChannel>> {
        Ok(Vec::new())
    }

    fn logic_samples(&mut self) -> Result<Vec<LogicChannel>> {
        Ok(Vec::new())
    }
}

fn pick_free_port() -> Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

fn format_gdb_packet(payload: &str) -> String {
    let sum: u8 = payload.bytes().fold(0u8, |acc, b| acc.wrapping_add(b));
    format!("${}#{:02x}", payload, sum)
}

fn decode_le_u32(hex8: &str) -> Option<u32> {
    if hex8.len() < 8 {
        return None;
    }
    let b0 = u8::from_str_radix(&hex8[0..2], 16).ok()?;
    let b1 = u8::from_str_radix(&hex8[2..4], 16).ok()?;
    let b2 = u8::from_str_radix(&hex8[4..6], 16).ok()?;
    let b3 = u8::from_str_radix(&hex8[6..8], 16).ok()?;
    Some(u32::from_le_bytes([b0, b1, b2, b3]))
}

fn encode_le_u32(val: u32) -> String {
    let b = val.to_le_bytes();
    format!("{:02x}{:02x}{:02x}{:02x}", b[0], b[1], b[2], b[3])
}
