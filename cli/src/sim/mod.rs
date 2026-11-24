pub mod cpu;
pub mod memory;
pub mod nvic;
pub mod peripherals;

#[cfg(test)]
mod core_tests;
#[cfg(test)]
mod lab_tests;

use crate::elf::{DisasmLine, LoadedElf};
use crate::sim::cpu::{is_thumb32, Cpu, HaltReason};
use crate::sim::memory::{DeviceMap, Memory};
use crate::sim::peripherals::{LogicAnalyzer, LogicChannel, PeripheralSnapshot};
use crate::target::{
    BreakpointInfo, CallFrame, DebugState, DebugTarget, MemoryReadResult, RegisterSnapshot,
    WatchEval,
};
use anyhow::{anyhow, Result};
use std::collections::BTreeMap;

pub struct Simulator {
    pub elf: LoadedElf,
    pub cpu: Cpu,
    pub mem: Memory,
    pub logic: LogicAnalyzer,
    pub breakpoints: BTreeMap<u32, BreakpointInfo>,
    prev_r: [u32; 16],
    prev_xpsr: u32,
    prev_msp: u32,
    prev_psp: u32,
    prev_primask: u32,
    prev_mem_snapshot: BTreeMap<u32, u8>,
}

impl Simulator {
    /// Whether the PC lies in the callee of the call that set LR: a `BL` just before the
    /// return address, whose target starts a region running up to the next symbol. A `BLX`
    /// register call (unknown target) counts as containing the PC.
    fn lr_call_contains_pc(&mut self) -> bool {
        let ret = self.cpu.r[14] & !1;
        let pc = self.cpu.r[15];
        if !(4..0x4000_0000).contains(&ret) {
            return false; // not a code address: never read peripherals for this
        }
        if self.mem.read16(ret.wrapping_sub(2)) & 0xFF87 == 0x4780 {
            return true; // BLX Rm
        }
        let (h1, h2) = (self.mem.read16(ret.wrapping_sub(4)) as u32, self.mem.read16(ret.wrapping_sub(2)) as u32);
        if h1 & 0xF800 != 0xF000 || h2 & 0xD000 != 0xD000 {
            return false;
        }
        let s = (h1 >> 10) & 1;
        let i1 = !((h2 >> 13) & 1 ^ s) & 1;
        let i2 = !((h2 >> 11) & 1 ^ s) & 1;
        let imm = (s << 24) | (i1 << 23) | (i2 << 22) | ((h1 & 0x3FF) << 12) | ((h2 & 0x7FF) << 1);
        let target = ret.wrapping_add(((imm << 7) as i32 >> 7) as u32);
        let end = self.elf.symbols.iter().map(|s| s.addr).filter(|&a| a > target).min().unwrap_or(u32::MAX);
        (target..end).contains(&pc)
    }

    /// Create a simulator for the default part (STM32F103C8, Blue Pill). `xtal_hz` is the
    /// HSE crystal frequency; the core starts on the 8 MHz HSI like real silicon and runs
    /// at whatever clock the program configures through RCC.
    pub fn new(elf: LoadedElf, xtal_hz: u32) -> Self {
        Self::new_for_device(elf, xtal_hz, "STM32F103C8")
    }

    /// Create a simulator whose flash/SRAM sizes follow the given ST part number.
    pub fn new_for_device(elf: LoadedElf, xtal_hz: u32, device: &str) -> Self {
        let mut cpu = Cpu::default();
        let mut mem = Memory::new(DeviceMap::for_device(device), xtal_hz.max(1));
        mem.reset_and_load(&elf);
        cpu.reset(&mut mem, Some(elf.entry));

        let mut logic = LogicAnalyzer::default();
        logic.add_signal("R0");
        logic.add_signal("GPIOA.ODR");

        let mut sim = Self {
            elf,
            cpu,
            mem,
            logic,
            breakpoints: BTreeMap::new(),
            prev_r: [0; 16],
            prev_xpsr: 0x0100_0000,
            prev_msp: 0x2000_1000,
            prev_psp: 0,
            prev_primask: 0,
            prev_mem_snapshot: BTreeMap::new(),
        };
        sim.snapshot_prev_regs();
        sim.record_logic_points();
        sim
    }

    fn snapshot_prev_regs(&mut self) {
        self.prev_r = self.cpu.r;
        self.prev_xpsr = self.cpu.xpsr;
        self.prev_msp = self.cpu.msp;
        self.prev_psp = self.cpu.psp;
        self.prev_primask = self.cpu.primask;
    }

    fn record_logic_points(&mut self) {
        let cycle = self.cpu.cycles;
        for idx in 0..self.logic.channels.len() {
            let sig = self.logic.channels[idx].signal.clone();
            if let Some(val) = self.eval_signal_u64(&sig) {
                self.logic.record_point(idx, cycle, val);
            }
        }
    }

    fn eval_signal_u64(&mut self, sig: &str) -> Option<u64> {
        let u = sig.trim().to_ascii_uppercase();
        if let Some(reg_idx) = parse_reg_name(&u) {
            return Some(self.cpu.r[reg_idx] as u64);
        }
        if let Some(v) = self.mem.peripherals.eval_signal_field(&u) {
            return Some(v);
        }
        if let Some(addr_str) = u.strip_prefix('*').or_else(|| Some(u.as_str()).filter(|s| s.starts_with("0X"))) {
            if let Some(addr) = parse_num_u32(addr_str) {
                return Some(self.mem.read32(addr) as u64);
            }
        }
        None
    }

    fn build_state(&self, reason: &str) -> DebugState {
        let mut changed = Vec::new();
        for i in 0..16 {
            if self.cpu.r[i] != self.prev_r[i] {
                changed.push(reg_label(i).to_string());
            }
        }
        if self.cpu.xpsr != self.prev_xpsr {
            changed.push("xPSR".to_string());
        }
        if self.cpu.msp != self.prev_msp {
            changed.push("MSP".to_string());
        }
        if self.cpu.psp != self.prev_psp {
            changed.push("PSP".to_string());
        }
        if self.cpu.primask != self.prev_primask {
            changed.push("PRIMASK".to_string());
        }

        let pc = self.cpu.r[15];
        let le = self.elf.line_for_addr(pc);
        let exc = self.cpu.xpsr & 0x1FF;
        let mode = if exc == 0 {
            "Thread".to_string()
        } else {
            format!("Handler({})", exc)
        };

        let registers = RegisterSnapshot {
            r: self.cpu.r,
            msp: self.cpu.msp,
            psp: self.cpu.psp,
            xpsr: self.cpu.xpsr,
            primask: self.cpu.primask,
            control: self.cpu.control,
            cycles: self.cpu.cycles,
            time_sec: self.cpu.time_sec,
            changed_regs: changed,
            n: self.cpu.flag_n(),
            z: self.cpu.flag_z(),
            c: self.cpu.flag_c(),
            v: self.cpu.flag_v(),
            q: self.cpu.flag_q(),
            t: (self.cpu.xpsr & (1 << 24)) != 0,
            mode,
        };

        let call_stack = self.build_call_stack();
        let uart1_output = String::from_utf8_lossy(&self.mem.peripherals.usarts[0].tx_buffer).into_owned();

        DebugState {
            backend: "simulator".to_string(),
            running: false,
            halt_reason: reason.to_string(),
            pc,
            current_file: le.map(|l| l.file.clone()),
            current_line: le.map(|l| l.line),
            registers,
            call_stack,
            breakpoints: self.breakpoints.values().cloned().collect(),
            uart1_output,
        }
    }

    fn build_call_stack(&self) -> Vec<CallFrame> {
        let mut frames = Vec::new();
        let pc = self.cpu.r[15];
        let le = self.elf.line_for_addr(pc);
        let sym = self
            .elf
            .symbol_for_addr(pc)
            .map(|(s, off)| if off == 0 { s.name.clone() } else { format!("{}+0x{:X}", s.name, off) })
            .unwrap_or_else(|| format!("0x{:08X}", pc));
        frames.push(CallFrame {
            index: 0,
            addr: pc,
            symbol: sym,
            file: le.map(|l| l.file.clone()),
            line: le.map(|l| l.line),
        });

        let lr = self.cpu.r[14];
        if lr != 0xFFFF_FFFF && lr != 0 && (lr & 0xF000_0000) == 0 {
            let caller = (lr & !1).saturating_sub(2);
            let cle = self.elf.line_for_addr(caller);
            let csym = self
                .elf
                .symbol_for_addr(caller)
                .map(|(s, off)| if off == 0 { s.name.clone() } else { format!("{}+0x{:X}", s.name, off) })
                .unwrap_or_else(|| format!("0x{:08X}", caller));
            frames.push(CallFrame {
                index: 1,
                addr: caller,
                symbol: csym,
                file: cle.map(|l| l.file.clone()),
                line: cle.map(|l| l.line),
            });
        }
        frames
    }

    fn step_one_internal(&mut self) -> HaltReason {
        self.cpu.halted_reason = HaltReason::Running;
        let _cost = self.cpu.step(&mut self.mem);
        self.record_logic_points();
        self.cpu.halted_reason.clone()
    }
}

impl DebugTarget for Simulator {
    fn backend_name(&self) -> &str {
        "simulator"
    }

    fn reset(&mut self) -> Result<DebugState> {
        self.mem.reset_and_load(&self.elf);
        self.cpu.reset(&mut self.mem, Some(self.elf.entry));
        self.logic.clear_samples();
        self.prev_mem_snapshot.clear();
        self.snapshot_prev_regs();
        self.record_logic_points();
        Ok(self.build_state("Reset"))
    }

    fn step_into(&mut self) -> Result<DebugState> {
        self.snapshot_prev_regs();
        self.mem.dirty_sram.clear();
        let hr = self.step_one_internal();
        let reason = match hr {
            HaltReason::Bkpt(n) => format!("BKPT #{}", n),
            HaltReason::Fault(msg) => format!("Fault: {}", msg),
            _ => "Step".to_string(),
        };
        Ok(self.build_state(&reason))
    }

    fn step_over(&mut self) -> Result<DebugState> {
        self.snapshot_prev_regs();
        self.mem.dirty_sram.clear();
        let pc = self.cpu.r[15];
        let hw1 = self.mem.read16(pc);
        let is_bl = (is_thumb32(hw1) && ((hw1 >> 11) == 0b11110) && ((self.mem.read16(pc + 2) & 0xD000) == 0xD000))
            || ((hw1 & 0xFF80) == 0x4780); // BLX Rm

        if !is_bl {
            return self.step_into();
        }

        let next_addr = pc + if is_thumb32(hw1) { 4 } else { 2 };
        let mut reason = "StepOver".to_string();
        for _ in 0..50_000 {
            let hr = self.step_one_internal();
            if self.cpu.r[15] == next_addr {
                break;
            }
            if let Some(bp) = self.breakpoints.get(&self.cpu.r[15]) {
                if bp.enabled {
                    reason = format!("Breakpoint 0x{:08X}", self.cpu.r[15]);
                    break;
                }
            }
            if !matches!(hr, HaltReason::Running) {
                reason = format!("{:?}", hr);
                break;
            }
        }
        Ok(self.build_state(&reason))
    }

    fn step_out(&mut self) -> Result<DebugState> {
        self.snapshot_prev_regs();
        self.mem.dirty_sram.clear();
        let ret_target = self.cpu.r[14] & !1;
        if self.cpu.r[14] == 0xFFFF_FFFF {
            return self.step_into();
        }
        // In a handler, the step also ends when the handler's exception stops being active (a
        // nested handler that preempts it does not count). LR is only trusted while the PC is
        // inside the routine that the BL before LR called; otherwise it holds EXC_RETURN or is
        // stale from an earlier BL, and only the exception return ends the step.
        let handler = self.mem.peripherals.ipsr as usize;
        let lr_stale = handler != 0 && (self.cpu.r[14] >= 0xFFFF_FFF0 || !self.lr_call_contains_pc());
        let mut reason = "StepOut".to_string();
        for _ in 0..50_000 {
            let hr = self.step_one_internal();
            if !matches!(hr, HaltReason::Running) {
                reason = format!("{:?}", hr); // e.g. a fault on the exception return
                break;
            }
            let returned = (handler != 0 && !self.mem.peripherals.nvic.active[handler])
                || (!lr_stale && self.cpu.r[15] == ret_target);
            if returned {
                break;
            }
            if let Some(bp) = self.breakpoints.get(&self.cpu.r[15]) {
                if bp.enabled {
                    reason = format!("Breakpoint 0x{:08X}", self.cpu.r[15]);
                    break;
                }
            }
        }
        Ok(self.build_state(&reason))
    }

    fn run(&mut self, max_steps: usize, stop_on_self_loop: bool) -> Result<DebugState> {
        self.snapshot_prev_regs();
        self.mem.dirty_sram.clear();
        let mut reason = "MaxSteps".to_string();
        let mut same_pc_streak = 0u32;
        let mut prev_pc = self.cpu.r[15];

        for step_i in 0..max_steps {
            let cur_pc = self.cpu.r[15];
            if step_i > 0 {
                if let Some(bp) = self.breakpoints.get(&cur_pc) {
                    if bp.enabled {
                        reason = format!("Breakpoint 0x{:08X}", cur_pc);
                        break;
                    }
                }
            }

            let hr = self.step_one_internal();
            match hr {
                HaltReason::Bkpt(n) => {
                    reason = format!("BKPT #{}", n);
                    break;
                }
                HaltReason::Fault(msg) => {
                    reason = format!("Fault: {}", msg);
                    break;
                }
                _ => {}
            }

            if self.cpu.sleeping && !self.mem.peripherals.activity_pending() {
                reason = format!("Sleep 0x{:08X} (WFI/WFE with no interrupt source armed)", self.cpu.r[15]);
                break;
            }

            let after_pc = self.cpu.r[15];
            // A branch-to-self only ends the run when nothing (SysTick, EXTI, a counting
            // timer driving an output-compare pin, UART) can still change what happens.
            if stop_on_self_loop
                && after_pc == cur_pc
                && prev_pc == cur_pc
                && !self.mem.peripherals.activity_pending()
            {
                same_pc_streak += 1;
                if same_pc_streak >= 2 {
                    reason = format!("SelfLoop 0x{:08X}", after_pc);
                    break;
                }
            } else {
                same_pc_streak = 0;
            }
            prev_pc = cur_pc;
        }
        Ok(self.build_state(&reason))
    }

    fn run_to(&mut self, addr: u32, max_steps: usize) -> Result<DebugState> {
        self.snapshot_prev_regs();
        self.mem.dirty_sram.clear();
        let target = addr & !1;
        let mut reason = "MaxSteps".to_string();
        for _ in 0..max_steps {
            let hr = self.step_one_internal();
            if self.cpu.r[15] == target {
                reason = format!("RunTo 0x{:08X}", target);
                break;
            }
            if let Some(bp) = self.breakpoints.get(&self.cpu.r[15]) {
                if bp.enabled {
                    reason = format!("Breakpoint 0x{:08X}", self.cpu.r[15]);
                    break;
                }
            }
            if !matches!(hr, HaltReason::Running) {
                reason = format!("{:?}", hr);
                break;
            }
        }
        Ok(self.build_state(&reason))
    }

    fn pause(&mut self) -> Result<DebugState> {
        Ok(self.build_state("Paused"))
    }

    fn state(&mut self) -> Result<DebugState> {
        Ok(self.build_state("Ready"))
    }

    fn set_breakpoint_addr(
        &mut self,
        addr: u32,
        file: Option<String>,
        line: Option<u32>,
    ) -> Result<Vec<BreakpointInfo>> {
        let clean = addr & !1;
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
        self.breakpoints.remove(&(addr & !1));
        Ok(self.breakpoints.values().cloned().collect())
    }

    fn clear_breakpoints(&mut self) -> Result<()> {
        self.breakpoints.clear();
        Ok(())
    }

    fn read_memory(&mut self, addr: u32, len: usize) -> Result<MemoryReadResult> {
        let bytes = self.mem.read_slice(addr, len);
        let mut changed_offsets = Vec::new();
        for (i, &b) in bytes.iter().enumerate() {
            let a = addr.wrapping_add(i as u32);
            if self.mem.dirty_sram.contains(&a)
                || self.prev_mem_snapshot.get(&a).copied().unwrap_or(b) != b
            {
                changed_offsets.push(i);
            }
            self.prev_mem_snapshot.insert(a, b);
        }
        Ok(MemoryReadResult {
            addr,
            bytes,
            changed_offsets,
        })
    }

    fn write_memory(&mut self, addr: u32, data: &[u8]) -> Result<()> {
        // Use the widest aligned access, as a debugger does: peripheral registers replicate
        // narrow writes across the word, so byte-by-byte writes would clobber them.
        let mut i = 0usize;
        while i < data.len() {
            let a = addr.wrapping_add(i as u32);
            let left = data.len() - i;
            if a.is_multiple_of(4) && left >= 4 {
                self.mem.write32(a, u32::from_le_bytes(data[i..i + 4].try_into().unwrap()));
                i += 4;
            } else if a.is_multiple_of(2) && left >= 2 {
                self.mem.write16(a, u16::from_le_bytes([data[i], data[i + 1]]));
                i += 2;
            } else {
                self.mem.write8(a, data[i]);
                i += 1;
            }
        }
        Ok(())
    }

    fn set_register(&mut self, name: &str, value: u32) -> Result<DebugState> {
        self.snapshot_prev_regs();
        let u = name.trim().to_ascii_uppercase();
        if let Some(idx) = parse_reg_name(&u) {
            if idx == 15 {
                self.cpu.r[15] = value & !1;
            } else if idx == 13 {
                self.cpu.r[13] = value & !3;
                self.cpu.msp = self.cpu.r[13];
            } else {
                self.cpu.r[idx] = value;
            }
        } else if u == "XPSR" || u == "CPSR" {
            self.cpu.xpsr = value | (1 << 24);
        } else if u == "MSP" {
            self.cpu.msp = value & !3;
            if (self.cpu.control & 2) == 0 {
                self.cpu.r[13] = self.cpu.msp;
            }
        } else if u == "PSP" {
            self.cpu.psp = value & !3;
        } else if u == "PRIMASK" {
            self.cpu.primask = value & 1;
        } else {
            return Err(anyhow!("Unknown register '{}'", name));
        }
        Ok(self.build_state(&format!("Set {} = 0x{:08X}", name, value)))
    }

    fn eval_watch(&mut self, expr: &str) -> Result<WatchEval> {
        let trimmed = expr.trim();
        let upper = trimmed.to_ascii_uppercase();
        if let Some(idx) = parse_reg_name(&upper) {
            let v = self.cpu.r[idx];
            return Ok(WatchEval {
                expr: trimmed.to_string(),
                value_hex: format!("0x{:08X}", v),
                value_dec: format!("{} ({})", v, v as i32),
                kind: "register".to_string(),
            });
        }
        if upper == "XPSR" || upper == "CPSR" {
            let v = self.cpu.xpsr;
            return Ok(WatchEval {
                expr: trimmed.to_string(),
                value_hex: format!("0x{:08X}", v),
                value_dec: format!("{}", v),
                kind: "register".to_string(),
            });
        }
        if let Some(v) = self.mem.peripherals.eval_signal_field(&upper) {
            return Ok(WatchEval {
                expr: trimmed.to_string(),
                value_hex: format!("0x{:08X}", v),
                value_dec: format!("{}", v),
                kind: "peripheral".to_string(),
            });
        }
        if let Some(addr_expr) = trimmed.strip_prefix('*') {
            if let Some(addr) = parse_num_u32(addr_expr.trim()) {
                let v = self.mem.read32(addr);
                return Ok(WatchEval {
                    expr: trimmed.to_string(),
                    value_hex: format!("0x{:08X}", v),
                    value_dec: format!("{} ({})", v, v as i32),
                    kind: "memory32".to_string(),
                });
            }
        }
        if let Some(addr) = parse_num_u32(trimmed) {
            let v = self.mem.read32(addr);
            return Ok(WatchEval {
                expr: trimmed.to_string(),
                value_hex: format!("0x{:08X}", v),
                value_dec: format!("{} ({})", v, v as i32),
                kind: "memory32".to_string(),
            });
        }
        if let Some(clean) = self.elf.symbol_addr(trimmed) {
            let v = self.mem.read32(clean);
            return Ok(WatchEval {
                expr: trimmed.to_string(),
                value_hex: format!("0x{:08X}", clean),
                value_dec: format!("val@sym = 0x{:08X} ({})", v, v as i32),
                kind: "symbol".to_string(),
            });
        }
        Err(anyhow!("Cannot evaluate watch '{}'", trimmed))
    }

    fn disassembly(&mut self, _around_addr: Option<u32>, _count: usize) -> Result<Vec<DisasmLine>> {
        Ok(self.elf.disassembly.clone())
    }

    fn peripherals(&mut self) -> Result<Option<PeripheralSnapshot>> {
        Ok(Some(self.mem.peripherals.snapshot()))
    }

    fn set_gpio_pin(&mut self, port: &str, pin: u8, high: bool) -> Result<()> {
        let p = port.trim().to_ascii_uppercase();
        if let Some(idx) = self.mem.peripherals.gpios.iter().position(|g| g.name == p) {
            if pin > 15 {
                return Err(anyhow!("Pin {} out of range (0-15)", pin));
            }
            self.mem.peripherals.set_input_pin(idx, pin, high);
            self.record_logic_points();
            Ok(())
        } else {
            Err(anyhow!("Unknown GPIO port '{}'", port))
        }
    }

    fn connect_pins(&mut self, a: &str, b: &str, connected: bool) -> Result<()> {
        let pa = parse_pin_name(a).ok_or_else(|| anyhow!("Bad pin name '{}' (expected e.g. PA0)", a))?;
        let pb = parse_pin_name(b).ok_or_else(|| anyhow!("Bad pin name '{}' (expected e.g. PA4)", b))?;
        self.mem.peripherals.set_pin_link(pa, pb, connected);
        self.record_logic_points();
        Ok(())
    }

    fn uart_send(&mut self, text: &str) -> Result<()> {
        self.mem.peripherals.usarts[0].push_rx_bytes(text.as_bytes());
        Ok(())
    }

    fn logic_add(&mut self, signal: &str) -> Result<Vec<LogicChannel>> {
        self.logic.add_signal(signal);
        self.record_logic_points();
        Ok(self.logic.channels.clone())
    }

    fn logic_remove(&mut self, signal: &str) -> Result<Vec<LogicChannel>> {
        self.logic.remove_signal(signal);
        Ok(self.logic.channels.clone())
    }

    fn logic_samples(&mut self) -> Result<Vec<LogicChannel>> {
        Ok(self.logic.channels.clone())
    }
}

/// `"PA0"`, `"pc15"`, `"GPIOB.3"` -> (port index, pin).
pub fn parse_pin_name(s: &str) -> Option<(u8, u8)> {
    let u = s.trim().to_ascii_uppercase();
    let rest = u.strip_prefix("GPIO").or_else(|| u.strip_prefix('P'))?;
    // split by characters, not bytes, so non-ASCII input is rejected instead of panicking
    let mut chars = rest.chars();
    let port = chars.next()?;
    let tail = chars.as_str();
    let pin = tail.strip_prefix('.').unwrap_or(tail);
    if !('A'..='G').contains(&port) {
        return None;
    }
    let pin: u8 = pin.parse().ok()?;
    (pin < 16).then_some((port as u8 - b'A', pin))
}

pub fn parse_reg_name(upper: &str) -> Option<usize> {
    match upper {
        "R0" => Some(0),
        "R1" => Some(1),
        "R2" => Some(2),
        "R3" => Some(3),
        "R4" => Some(4),
        "R5" => Some(5),
        "R6" => Some(6),
        "R7" => Some(7),
        "R8" => Some(8),
        "R9" => Some(9),
        "R10" => Some(10),
        "R11" => Some(11),
        "R12" => Some(12),
        "R13" | "SP" => Some(13),
        "R14" | "LR" => Some(14),
        "R15" | "PC" => Some(15),
        _ => None,
    }
}

pub fn reg_label(idx: usize) -> &'static str {
    match idx {
        0 => "R0",
        1 => "R1",
        2 => "R2",
        3 => "R3",
        4 => "R4",
        5 => "R5",
        6 => "R6",
        7 => "R7",
        8 => "R8",
        9 => "R9",
        10 => "R10",
        11 => "R11",
        12 => "R12",
        13 => "R13 (SP)",
        14 => "R14 (LR)",
        15 => "R15 (PC)",
        _ => "?",
    }
}

pub fn parse_num_u32(s: &str) -> Option<u32> {
    let t = s.trim();
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else {
        t.parse::<u32>().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::cpu::thumb_expand_imm_c;

    #[test]
    fn test_thumb_expand_imm_c() {
        let (v, _) = thumb_expand_imm_c(0x47F, false); // MOV R1, #0xFF000000
        assert_eq!(v, 0xFF00_0000);
        let (v2, _) = thumb_expand_imm_c(0x87F, false); // ORR R1, #0xFF0000
        assert_eq!(v2, 0x00FF_0000);
        let (v3, _) = thumb_expand_imm_c(0xC7F, false); // ORR R1, #0xFF00
        assert_eq!(v3, 0x0000_FF00);
    }

    #[test]
    fn test_cpu_basic_alu_and_mem() {
        let mut mem = Memory::default();
        let mut cpu = Cpu::default();
        // Vector table at 0x08000000
        mem.flash[0..4].copy_from_slice(&0x2000_1000u32.to_le_bytes());
        mem.flash[4..8].copy_from_slice(&0x0800_0009u32.to_le_bytes());
        // Code at 0x08000008: MOVS R0, #10; ADDS R0, #5; B .
        let code: [u16; 3] = [0x200A, 0x3005, 0xE7FE];
        for (i, hw) in code.iter().enumerate() {
            mem.flash[8 + i * 2..10 + i * 2].copy_from_slice(&hw.to_le_bytes());
        }
        cpu.reset(&mut mem, None);
        assert_eq!(cpu.r[15], 0x0800_0008);
        cpu.step(&mut mem);
        assert_eq!(cpu.r[0], 10);
        cpu.step(&mut mem);
        assert_eq!(cpu.r[0], 15);
    }

    #[test]
    fn test_gpio_and_timer_peripherals() {
        let mut mem = Memory::default();
        // Enable GPIOA clock, configure PA0 output, write ODR = 0x55
        mem.write32(0x4002_1018, 1 << 2);
        mem.write32(0x4001_0800, 0x4444_4442);
        mem.write32(0x4001_080C, 0x0001);
        assert_eq!(mem.read32(0x4001_0808) & 1, 1); // IDR sees PA0 output

        // Toggle via BSRR reset
        mem.write32(0x4001_0810, 1 << 16);
        assert_eq!(mem.read32(0x4001_080C) & 1, 0);

        // External pin input on PA1 (floating input by reset configuration)
        mem.peripherals.set_input_pin(0, 1, true);
        assert_eq!(mem.read32(0x4001_0808) & 2, 2);

        // Timer 2 ticks (clock enabled in RCC_APB1ENR)
        mem.write32(0x4002_101C, 1);
        mem.write32(0x4000_0028, 0); // PSC = 0
        mem.write32(0x4000_002C, 5); // ARR = 5
        mem.write32(0x4000_0000, 1); // CEN = 1
        mem.peripherals.tick(6);
        assert_ne!(mem.read32(0x4000_0010) & 1, 0); // UIF set
    }
}
