use crate::elf::LoadedElf;
use crate::sim::{parse_num_u32, reg_label};
use crate::target::{DebugState, DebugTarget};
use anyhow::Result;
#[cfg(not(target_arch = "wasm32"))]
use std::io::{self, Write};

#[cfg(not(target_arch = "wasm32"))]
pub fn run_repl(target: &mut dyn DebugTarget, elf: &LoadedElf) -> Result<()> {
    println!("uVision Fast Debugger REPL (backend: {}). Type 'help' or '?' for commands.", target.backend_name());
    let st = target.state()?;
    print_location(&st);

    let stdin = io::stdin();
    let mut line = String::new();
    loop {
        print!("uv> ");
        io::stdout().flush()?;
        line.clear();
        if stdin.read_line(&mut line)? == 0 {
            break;
        }
        let cmd = line.trim();
        if cmd.is_empty() {
            continue;
        }
        if matches!(cmd, "q" | "quit" | "exit") {
            break;
        }
        match execute_debug_command(target, elf, cmd) {
            Ok(out) => {
                if !out.is_empty() {
                    println!("{}", out);
                }
            }
            Err(e) => println!("Error: {}", e),
        }
    }
    Ok(())
}

pub fn execute_debug_command(
    target: &mut dyn DebugTarget,
    elf: &LoadedElf,
    raw_cmd: &str,
) -> Result<String> {
    let parts: Vec<&str> = raw_cmd.split_whitespace().collect();
    if parts.is_empty() {
        return Ok(String::new());
    }
    let op = parts[0].to_ascii_lowercase();
    match op.as_str() {
        "?" | "help" => Ok(HELP_TEXT.to_string()),
        "r" | "regs" => {
            if parts.len() == 3 {
                let val = parse_num_u32(parts[2]).ok_or_else(|| anyhow::anyhow!("Invalid hex/dec value"))?;
                let st = target.set_register(parts[1], val)?;
                Ok(format_regs(&st))
            } else {
                let st = target.state()?;
                Ok(format_regs(&st))
            }
        }
        "t" | "s" | "step" => {
            let st = target.step_into()?;
            Ok(format_step_summary(&st))
        }
        "p" | "n" | "next" => {
            let st = target.step_over()?;
            Ok(format_step_summary(&st))
        }
        "out" | "stepout" => {
            let st = target.step_out()?;
            Ok(format_step_summary(&st))
        }
        "g" | "go" | "run" => {
            let max_steps = parts.get(1).and_then(|s| s.parse::<usize>().ok()).unwrap_or(100_000);
            let st = target.run(max_steps, true)?;
            Ok(format!("{}\n{}", format_step_summary(&st), format_regs(&st)))
        }
        "reset" | "rst" => {
            let st = target.reset()?;
            Ok(format!("Target reset.\n{}", format_step_summary(&st)))
        }
        "d" | "dump" => {
            let addr = parts
                .get(1)
                .and_then(|s| parse_num_u32(s))
                .unwrap_or(0x2000_0000);
            let len = parts
                .get(2)
                .and_then(|s| parse_num_u32(s))
                .unwrap_or(64)
                .min(512) as usize;
            let mem = target.read_memory(addr, len)?;
            Ok(format_hex_dump(mem.addr, &mem.bytes))
        }
        "e" | "ew" | "edit" => {
            if parts.len() < 3 {
                return Err(anyhow::anyhow!("Usage: e <addr> <byte_or_word> [...]"));
            }
            let addr = parse_num_u32(parts[1]).ok_or_else(|| anyhow::anyhow!("Invalid address"))?;
            let mut bytes = Vec::new();
            if op == "ew" {
                for p in &parts[2..] {
                    let w = parse_num_u32(p).ok_or_else(|| anyhow::anyhow!("Invalid word {}", p))?;
                    bytes.extend_from_slice(&w.to_le_bytes());
                }
            } else {
                for p in &parts[2..] {
                    let v = parse_num_u32(p).ok_or_else(|| anyhow::anyhow!("Invalid byte {}", p))?;
                    bytes.push((v & 0xFF) as u8);
                }
            }
            target.write_memory(addr, &bytes)?;
            Ok(format!("Wrote {} byte(s) at 0x{:08X}", bytes.len(), addr))
        }
        "bs" | "bp" => {
            let spec = parts.get(1).ok_or_else(|| anyhow::anyhow!("Usage: bs <0xADDR | file:line | symbol>"))?;
            let (addr, file, line) = resolve_bp_spec(elf, spec)?;
            let bps = target.set_breakpoint_addr(addr, file, line)?;
            Ok(format!("Breakpoint set at 0x{:08X} (total: {})", addr, bps.len()))
        }
        "bc" => {
            if let Some(spec) = parts.get(1) {
                let (addr, _, _) = resolve_bp_spec(elf, spec)?;
                target.remove_breakpoint_addr(addr)?;
                Ok(format!("Cleared breakpoint 0x{:08X}", addr))
            } else {
                target.clear_breakpoints()?;
                Ok("Cleared all breakpoints.".to_string())
            }
        }
        "u" | "disasm" => {
            let st = target.state()?;
            let center = parts.get(1).and_then(|s| parse_num_u32(s)).unwrap_or(st.pc);
            let lines = target.disassembly(Some(center), 20)?;
            let start = lines
                .iter()
                .position(|l| l.addr >= center.saturating_sub(8))
                .unwrap_or(0);
            let mut out = Vec::new();
            for l in lines.iter().skip(start).take(20) {
                let marker = if l.addr == st.pc { "=>" } else { "  " };
                if let Some(lbl) = &l.label {
                    out.push(format!("   {}:", lbl));
                }
                out.push(format!("{} 0x{:08X}: {:<10} {}", marker, l.addr, l.opcode, l.mnemonic));
            }
            Ok(out.join("\n"))
        }
        "w" | "watch" => {
            let expr = parts.get(1).ok_or_else(|| anyhow::anyhow!("Usage: w <R0 | *0x20000000 | symbol>"))?;
            let res = target.eval_watch(expr)?;
            Ok(format!("{} = {} ({})", res.expr, res.value_hex, res.value_dec))
        }
        "periph" => {
            let Some(p) = target.peripherals()? else {
                return Ok("Peripherals snapshot not available in hardware mode.".to_string());
            };
            let filter = parts.get(1).map(|s| s.to_ascii_uppercase());
            let mut out = Vec::new();
            if filter.as_deref().map_or(true, |f| f == "RCC") {
                out.push(format!(
                    "RCC: CR=0x{:08X} CFGR=0x{:08X} APB2ENR=0x{:08X} APB1ENR=0x{:08X}",
                    p.rcc.cr, p.rcc.cfgr, p.rcc.apb2enr, p.rcc.apb1enr
                ));
            }
            for g in &p.gpios {
                if filter.as_deref().map_or(true, |f| g.name == f || f == "GPIO") {
                    out.push(format!(
                        "{}: CRL=0x{:08X} CRH=0x{:08X} IDR=0x{:04X} ODR=0x{:04X}",
                        g.name, g.crl, g.crh, g.idr & 0xFFFF, g.odr & 0xFFFF
                    ));
                }
            }
            for t in &p.timers {
                if filter.as_deref().map_or(true, |f| t.name == f || f == "TIM") {
                    out.push(format!(
                        "{}: CR1=0x{:04X} SR=0x{:04X} CNT={} PSC={} ARR={}",
                        t.name, t.cr1, t.sr, t.cnt, t.psc, t.arr
                    ));
                }
            }
            if !p.uart1_text.is_empty() {
                out.push(format!("USART1 TX: {:?}", p.uart1_text));
            }
            Ok(out.join("\n"))
        }
        "pin" => {
            if parts.len() < 4 {
                return Err(anyhow::anyhow!("Usage: pin <GPIOA..G> <0..15> <0|1>"));
            }
            let pin_num = parts[2].parse::<u8>()?;
            let high = parts[3] != "0";
            target.set_gpio_pin(parts[1], pin_num, high)?;
            Ok(format!("Set {}.{} = {}", parts[1].to_ascii_uppercase(), pin_num, if high { 1 } else { 0 }))
        }
        "uart" => {
            let msg = raw_cmd.trim_start_matches("uart").trim();
            target.uart_send(msg)?;
            Ok(format!("Queued {} byte(s) to USART1 RX", msg.len()))
        }
        _ => Err(anyhow::anyhow!("Unknown command '{}'. Type 'help'.", raw_cmd)),
    }
}

pub fn resolve_bp_spec(elf: &LoadedElf, spec: &str) -> Result<(u32, Option<String>, Option<u32>)> {
    if let Some(addr) = parse_num_u32(spec) {
        let le = elf.line_for_addr(addr);
        return Ok((addr & !1, le.map(|l| l.file.clone()), le.map(|l| l.line)));
    }
    if let Some((f, lstr)) = spec.rsplit_once(':') {
        if let Ok(line) = lstr.parse::<u32>() {
            if let Some(addr) = elf.addr_for_line(f, line) {
                return Ok((addr, Some(f.to_string()), Some(line)));
            }
            return Err(anyhow::anyhow!("No instruction mapped to {}:{}", f, line));
        }
    }
    if let Some(&addr) = elf.sym_by_name.get(spec) {
        let clean = addr & !1;
        let le = elf.line_for_addr(clean);
        return Ok((clean, le.map(|l| l.file.clone()), le.map(|l| l.line)));
    }
    Err(anyhow::anyhow!("Cannot resolve breakpoint target '{}'", spec))
}

#[cfg(not(target_arch = "wasm32"))]
fn print_location(st: &DebugState) {
    println!("{}", format_step_summary(st));
}

pub fn format_step_summary(st: &DebugState) -> String {
    let loc = match (&st.current_file, st.current_line) {
        (Some(f), Some(l)) => {
            let name = std::path::Path::new(f)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(f);
            format!(" ({}:{})", name, l)
        }
        _ => String::new(),
    };
    format!(
        "[{}] PC=0x{:08X}{} | Cycles={} ({:.6}s) | {}",
        st.halt_reason,
        st.pc,
        loc,
        st.registers.cycles,
        st.registers.time_sec,
        format!(
            "NZCVQ={}{}{}{}{}",
            st.registers.n as u8,
            st.registers.z as u8,
            st.registers.c as u8,
            st.registers.v as u8,
            st.registers.q as u8
        )
    )
}

pub fn format_regs(st: &DebugState) -> String {
    let mut lines = Vec::new();
    for row in 0..4 {
        let mut cols = Vec::new();
        for col in 0..4 {
            let idx = row * 4 + col;
            cols.push(format!("{:<8}= 0x{:08X}", reg_label(idx), st.registers.r[idx]));
        }
        lines.push(cols.join("  "));
    }
    lines.push(format!(
        "xPSR=0x{:08X} (N={} Z={} C={} V={} Q={} T={})  MSP=0x{:08X}  PSP=0x{:08X}  Mode={}",
        st.registers.xpsr,
        st.registers.n as u8,
        st.registers.z as u8,
        st.registers.c as u8,
        st.registers.v as u8,
        st.registers.q as u8,
        st.registers.t as u8,
        st.registers.msp,
        st.registers.psp,
        st.registers.mode
    ));
    lines.join("\n")
}

pub fn format_hex_dump(base: u32, bytes: &[u8]) -> String {
    let mut out = Vec::new();
    for (row_idx, chunk) in bytes.chunks(16).enumerate() {
        let addr = base.wrapping_add((row_idx * 16) as u32);
        let mut hex = String::new();
        let mut ascii = String::new();
        for i in 0..16 {
            if let Some(&b) = chunk.get(i) {
                hex.push_str(&format!("{:02X} ", b));
                ascii.push(if (0x20..=0x7E).contains(&b) { b as char } else { '.' });
            } else {
                hex.push_str("   ");
                ascii.push(' ');
            }
        }
        out.push(format!("0x{:08X}:  {} |{}|", addr, hex, ascii));
    }
    out.join("\n")
}

const HELP_TEXT: &str = "\
Keil-compatible Debugger Commands:
  r [reg val]             Show registers (or set register e.g. 'r R0 0x10')
  t | s | step            Step Into (F11)
  p | n | next            Step Over (F10)
  out                     Step Out (Ctrl+F11)
  g [max_steps]           Run / Go (F5)
  reset                   Reset target CPU & memory
  d [addr] [len]          Dump memory (e.g. 'd 0x20000000 64')
  e <addr> <b...>         Edit memory bytes (or 'ew' for 32-bit words)
  bs <addr|file:line|sym> Set breakpoint
  bc [addr|file:line]     Clear breakpoint (or all if omitted)
  u [addr]                Disassemble
  w <expr>                Evaluate watch expression (R0, *0x20000000, GPIOA.ODR)
  periph [RCC|GPIO|TIM]   Inspect STM32F1 peripherals
  pin <GPIOx> <pin> <0|1> Drive external input pin in simulator
  uart <text>             Send bytes to USART1 RX in simulator
  q                       Quit debugger";
