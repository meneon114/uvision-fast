use crate::elf::DisasmLine;
use crate::sim::peripherals::{LogicChannel, PeripheralSnapshot};
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub severity: String, // "error" | "warning" | "info"
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildResult {
    pub success: bool,
    pub axf_path: Option<String>,
    pub hex_path: Option<String>,
    pub bin_path: Option<String>,
    pub elapsed_ms: u64,
    pub errors: usize,
    pub warnings: usize,
    pub diagnostics: Vec<Diagnostic>,
    pub log: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterSnapshot {
    pub r: [u32; 16],
    pub msp: u32,
    pub psp: u32,
    pub xpsr: u32,
    pub primask: u32,
    pub control: u32,
    pub cycles: u64,
    pub time_sec: f64,
    pub changed_regs: Vec<String>,
    pub n: bool,
    pub z: bool,
    pub c: bool,
    pub v: bool,
    pub q: bool,
    pub t: bool,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallFrame {
    pub index: usize,
    pub addr: u32,
    pub symbol: String,
    pub file: Option<String>,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakpointInfo {
    pub addr: u32,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugState {
    pub backend: String, // "simulator" | "hardware"
    pub running: bool,
    pub halt_reason: String,
    pub pc: u32,
    pub current_file: Option<String>,
    pub current_line: Option<u32>,
    pub registers: RegisterSnapshot,
    pub call_stack: Vec<CallFrame>,
    pub breakpoints: Vec<BreakpointInfo>,
    pub uart1_output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryReadResult {
    pub addr: u32,
    pub bytes: Vec<u8>,
    pub changed_offsets: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchEval {
    pub expr: String,
    pub value_hex: String,
    pub value_dec: String,
    pub kind: String,
}

pub trait DebugTarget {
    fn backend_name(&self) -> &str;
    fn reset(&mut self) -> Result<DebugState>;
    fn step_into(&mut self) -> Result<DebugState>;
    fn step_over(&mut self) -> Result<DebugState>;
    fn step_out(&mut self) -> Result<DebugState>;
    fn run(&mut self, max_steps: usize, stop_on_self_loop: bool) -> Result<DebugState>;
    fn run_to(&mut self, addr: u32, max_steps: usize) -> Result<DebugState>;
    fn pause(&mut self) -> Result<DebugState>;
    fn state(&mut self) -> Result<DebugState>;
    fn set_breakpoint_addr(&mut self, addr: u32, file: Option<String>, line: Option<u32>) -> Result<Vec<BreakpointInfo>>;
    fn remove_breakpoint_addr(&mut self, addr: u32) -> Result<Vec<BreakpointInfo>>;
    fn clear_breakpoints(&mut self) -> Result<()>;
    fn read_memory(&mut self, addr: u32, len: usize) -> Result<MemoryReadResult>;
    fn write_memory(&mut self, addr: u32, data: &[u8]) -> Result<()>;
    fn set_register(&mut self, name: &str, value: u32) -> Result<DebugState>;
    fn eval_watch(&mut self, expr: &str) -> Result<WatchEval>;
    fn disassembly(&mut self, around_addr: Option<u32>, count: usize) -> Result<Vec<DisasmLine>>;
    fn peripherals(&mut self) -> Result<Option<PeripheralSnapshot>>;
    fn set_gpio_pin(&mut self, port: &str, pin: u8, high: bool) -> Result<()>;
    fn uart_send(&mut self, text: &str) -> Result<()>;
    fn logic_add(&mut self, signal: &str) -> Result<Vec<LogicChannel>>;
    fn logic_remove(&mut self, signal: &str) -> Result<Vec<LogicChannel>>;
    fn logic_samples(&mut self) -> Result<Vec<LogicChannel>>;
}
