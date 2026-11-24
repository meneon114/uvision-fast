export interface ProjectFile {
  name: string;
  path: string;
  abs_path: string;
  file_type: number;
}

export interface ProjectGroup {
  name: string;
  files: ProjectFile[];
}

export interface TargetOptions {
  target_name: string;
  device: string;
  pack_id: string;
  output_name: string;
  output_dir: string;
  xtal_hz: number;
  ro_base: number;
  ro_size: number;
  rw_base: number;
  rw_size: number;
  create_hex: boolean;
  create_bin: boolean;
  asm_syntax: 'armasm' | 'gnuarmclang';
  c_defines: string[];
  c_includes: string[];
  c_misc: string;
  a_defines: string[];
  a_includes: string[];
  a_misc: string;
  scatter_file: string | null;
  ld_misc: string;
  debug_mode: 'simulator' | 'hardware';
  hw_interface: string;
  hw_target: string;
}

export interface UvProject {
  project_file: string | null;
  root_dir: string;
  is_standalone: boolean;
  options: TargetOptions;
  groups: ProjectGroup[];
}

export interface Diagnostic {
  file: string;
  line: number;
  column: number;
  severity: 'error' | 'warning' | 'info' | string;
  code: string;
  message: string;
}

export interface BuildResult {
  success: boolean;
  axf_path: string | null;
  hex_path: string | null;
  bin_path: string | null;
  elapsed_ms: number;
  errors: number;
  warnings: number;
  diagnostics: Diagnostic[];
  log: string;
}

export interface RegisterSnapshot {
  r: number[];
  msp: number;
  psp: number;
  xpsr: number;
  primask: number;
  control: number;
  cycles: number;
  time_sec: number;
  changed_regs: string[];
  n: boolean;
  z: boolean;
  c: boolean;
  v: boolean;
  q: boolean;
  t: boolean;
  mode: string;
}

export interface CallFrame {
  index: number;
  addr: number;
  symbol: string;
  file: string | null;
  line: number | null;
}

export interface BreakpointInfo {
  addr: number;
  file: string | null;
  line: number | null;
  enabled: boolean;
}

export interface DebugState {
  backend: string;
  running: boolean;
  halt_reason: string;
  pc: number;
  current_file: string | null;
  current_line: number | null;
  registers: RegisterSnapshot;
  call_stack: CallFrame[];
  breakpoints: BreakpointInfo[];
  uart1_output: string;
}

export interface DisasmLine {
  addr: number;
  opcode: string;
  mnemonic: string;
  label: string | null;
  file: string | null;
  line: number | null;
}

export interface SymbolInfo {
  addr: number;
  name: string;
  is_func: boolean;
  size: number;
}

export interface DebugStartResult {
  state: DebugState;
  disassembly: DisasmLine[];
  executable_lines: Record<string, number[]>;
  symbols?: SymbolInfo[];
  build_log?: string;
}

export interface MemoryReadResult {
  addr: number;
  bytes: number[];
  changed_offsets: number[];
}

export interface WatchEval {
  expr: string;
  value_hex: string;
  value_dec: string;
  kind: string;
}

export interface GpioPort {
  name: string;
  crl: number;
  crh: number;
  idr: number;
  odr: number;
  lckr: number;
  ext_pins: number;
  ext_mask: number;
}

export interface TimerState {
  name: string;
  irq_num: number;
  cr1: number;
  cr2: number;
  smcr: number;
  dier: number;
  sr: number;
  ccmr1: number;
  ccmr2: number;
  ccer: number;
  cnt: number;
  psc: number;
  arr: number;
  rcr: number;
  ccr: number[];
}

export interface UsartState {
  name: string;
  sr: number;
  brr: number;
  cr1: number;
  cr2: number;
  cr3: number;
}

export interface PeripheralSnapshot {
  rcc: Record<string, number>;
  gpios: GpioPort[];
  timers: TimerState[];
  usarts: UsartState[];
  systick: { ctrl: number; load: number; val: number; calib: number };
  vtor: number;
  uart1_text: string;
}

export interface LogicSample {
  cycle: number;
  value: number;
}

export interface LogicChannel {
  signal: string;
  samples: LogicSample[];
}

export interface PackInfo {
  vendor: string;
  name: string;
  version: string;
}

export interface BuildArtifacts {
  output_name: string;
  has_artifacts: boolean;
  hex_text?: string;
  bin_b64?: string;
  elf_b64?: string;
}

export interface WorkspaceEntry {
  path: string;
}

export interface ExampleInfo {
  id: string;
  title: string;
  lab: string;
  description: string;
  entryPath: string;
}

export interface SessionState {
  projectPath?: string | null;
  openDocPaths?: string[];
  activeDocPath?: string | null;
  breakpoints?: Record<string, number[]>;
  watchExprs?: string[];
  memAddrs?: string[];
  theme?: 'system' | 'light' | 'dark';
  board?: unknown;
  recent?: string[];
}

export type HostEvent =
  | { type: 'openTarget'; path: string }
  | { type: 'restoreSession'; session: SessionState | null; fallbackPath?: string }
  | { type: 'setPwaState'; canInstall: boolean; offlineReady: boolean }
  | { type: 'fileChangedOnDisk'; path: string; content: string }
  | { type: 'hostAction'; action: 'build' | 'rebuild' | 'debug' | 'flash' }
  | { type: 'flashProgress'; percent: number; log: string };
