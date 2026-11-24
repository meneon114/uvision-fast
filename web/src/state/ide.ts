import { toast } from '@ui-sdk/react/toast';
import { host } from '../host/client';
import type {
  BreakpointInfo,
  BuildResult,
  DebugStartResult,
  DebugState,
  HostEvent,
  LogicChannel,
  MemoryReadResult,
  PeripheralSnapshot,
  RegisterSnapshot,
  SessionState,
  UvProject,
  WatchEval,
} from '../host/types';
import { baseName, errMsg, hex32, isSourceFile, normPath, samePath } from '../lib/format';
import { currentProjectPath, currentSearch, goProject, setSearch } from '../lib/nav';
import type { PanelId } from '../lib/search';
import { DEFAULT_BOARD, DEFAULT_MEM, DEFAULT_WATCH, docKey, getDoc, ide, isDirty, openDialog, type Theme } from './store';
import { applyBoardInputs, syncKeypad } from './board';

const AUTOSAVE_MS = 350;
const RUN_CHUNK = 20_000;
const autosaveTimers = new Map<string, number>();
let running = false;
let prevRegs: RegisterSnapshot | null = null;

// ---------- boot and host events ----------

export function boot(): void {
  const h = host();
  const s = h.loadSession();
  ide.set({
    hostKind: h.kind,
    theme: (s?.theme as Theme) || 'system',
    watchExprs: s?.watchExprs?.length ? s.watchExprs : [...DEFAULT_WATCH],
    memAddrs: s?.memAddrs?.length === 4 ? s.memAddrs : [...DEFAULT_MEM],
    breakpoints: normalizeBps(s?.breakpoints),
    board: isBoard(s?.board) ? s.board : DEFAULT_BOARD,
    recent: Array.isArray(s?.recent) ? s.recent.slice(0, 8) : [],
  });
  h.onEvent(onHostEvent);
  h.ready();
  window.addEventListener('beforeunload', flushDrafts);
  window.addEventListener('pagehide', flushDrafts);
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'hidden') flushDrafts();
  });
  ide.subscribe(schedulePersist);
}

function normalizeBps(raw: unknown): Record<string, number[]> {
  const out: Record<string, number[]> = {};
  if (raw && typeof raw === 'object') {
    for (const [k, v] of Object.entries(raw as Record<string, unknown>)) {
      if (Array.isArray(v)) {
        const lines = v.map(Number).filter((n) => Number.isInteger(n) && n > 0);
        if (lines.length) out[normPath(k)] = lines;
      }
    }
  }
  return out;
}

function isBoard(b: unknown): b is typeof DEFAULT_BOARD {
  return !!b && typeof b === 'object' && Array.isArray((b as typeof DEFAULT_BOARD).leds) && Array.isArray((b as typeof DEFAULT_BOARD).buttons);
}

function onHostEvent(ev: HostEvent): void {
  switch (ev.type) {
    case 'openTarget':
      void openPath(ev.path);
      break;
    case 'restoreSession': {
      ide.set({ booted: true });
      if (currentProjectPath()) return; // the URL already names a project
      const s = ev.session;
      const target = s?.projectPath || ev.fallbackPath;
      if (target) {
        const file = s?.projectPath && s.activeDocPath ? s.activeDocPath : undefined;
        void goProject(target, { file }, true);
      }
      break;
    }
    case 'setPwaState':
      ide.set((st) => ({ pwa: { canInstall: ev.canInstall, offlineReady: st.pwa.offlineReady || ev.offlineReady } }));
      break;
    case 'fileChangedOnDisk': {
      const d = getDoc(ev.path);
      if (d && !isDirty(d) && d.content !== ev.content) {
        setDoc(d.path, { content: ev.content, saved: ev.content });
      }
      break;
    }
    case 'hostAction':
      if (ev.action === 'build') void buildTarget(false);
      else if (ev.action === 'rebuild') void buildTarget(true);
      else if (ev.action === 'debug') toggleDebug();
      else if (ev.action === 'flash') void downloadToFlash();
      break;
    case 'flashProgress':
      ide.set((st) => ({ flashLog: { percent: ev.percent, lines: [...st.flashLog.lines, `[${ev.percent}%] ${ev.log}`] } }));
      break;
  }
}

// ---------- session persistence ----------

let persistTimer = 0;
function schedulePersist(): void {
  // Throttle, not debounce: a running target changes state every chunk.
  if (persistTimer) return;
  persistTimer = window.setTimeout(() => {
    persistTimer = 0;
    persistNow();
  }, 250);
}

function persistNow(): void {
  const st = ide.get();
  const projectPath = st.projectPath;
  const search = projectPath ? currentSearch() : {};
  const session: SessionState = {
    projectPath,
    openDocPaths: st.tabs,
    activeDocPath: search.file ?? null,
    breakpoints: st.breakpoints,
    watchExprs: st.watchExprs,
    memAddrs: st.memAddrs,
    theme: st.theme,
    board: st.board,
    recent: st.recent,
  };
  host().saveSession(session);
}

function flushDrafts(): void {
  const st = ide.get();
  for (const d of Object.values(st.docs)) {
    if (isDirty(d)) host().saveDraft(d.path, d.content);
  }
  persistNow();
}

// ---------- projects and documents ----------

export async function openPath(path: string, file?: string): Promise<void> {
  await saveAll(true);
  await goProject(path, { file: file ?? (isSourceFile(path) ? path : undefined) });
}

// Called by the project route once its loader has opened the project.
export function adoptProject(path: string, project: UvProject): void {
  const st = ide.get();
  const changed = !samePath(st.projectPath, path);
  host().activeProject = path;
  const recent = [path, ...st.recent.filter((p) => !samePath(p, path))].slice(0, 8);
  ide.set({ project, projectPath: path, recent, status: `Opened ${baseName(path)}` });
  if (changed) {
    const keep = ide.get().tabs.filter((t) => projectFiles(project).some((f) => samePath(f, t)) || samePath(t, path));
    ide.set({ tabs: keep });
  }
}

export function projectFiles(p: UvProject | null): string[] {
  if (!p) return [];
  return p.groups.flatMap((g) => g.files.map((f) => f.abs_path));
}

export function firstSource(p: UvProject | null): string | null {
  if (!p) return null;
  const f = p.groups.flatMap((g) => g.files).find((x) => x.file_type === 1 || x.file_type === 2);
  return f?.abs_path ?? null;
}

const loading = new Map<string, Promise<void>>();

export function ensureDoc(path: string): Promise<void> {
  const key = docKey(path);
  if (ide.get().docs[key]) {
    addTab(ide.get().docs[key].path);
    return Promise.resolve();
  }
  const existing = loading.get(key);
  if (existing) return existing;
  const p = host()
    .readFile(path)
    .then((res) => {
      ide.set((st) => ({ docs: { ...st.docs, [key]: { path: res.path, content: res.content, saved: res.content } } }));
      addTab(res.path);
    })
    .finally(() => loading.delete(key));
  loading.set(key, p);
  return p;
}

function addTab(path: string): void {
  const st = ide.get();
  if (!st.tabs.some((t) => samePath(t, path))) ide.set({ tabs: [...st.tabs, path] });
}

export function openFile(path: string, line?: number): void {
  setSearch({ file: path, line });
}

export function closeTab(path: string): void {
  const st = ide.get();
  const d = getDoc(path);
  if (d && isDirty(d)) void saveDoc(d.path);
  const idx = st.tabs.findIndex((t) => samePath(t, path));
  const tabs = st.tabs.filter((t) => !samePath(t, path));
  ide.set({ tabs });
  if (samePath(currentSearch().file, path)) {
    const next = tabs[Math.min(idx, tabs.length - 1)];
    setSearch({ file: next, line: undefined });
  }
}

function setDoc(path: string, patch: { content?: string; saved?: string }): void {
  const key = docKey(path);
  ide.set((st) => {
    const d = st.docs[key];
    if (!d) return {};
    return { docs: { ...st.docs, [key]: { ...d, ...patch } } };
  });
}

export function editDoc(path: string, content: string): void {
  setDoc(path, { content });
  if (ide.get().hostKind !== 'web') return;
  const key = docKey(path);
  window.clearTimeout(autosaveTimers.get(key));
  autosaveTimers.set(
    key,
    window.setTimeout(() => void saveDoc(path, true), AUTOSAVE_MS),
  );
}

export async function saveDoc(path: string, quiet = false): Promise<void> {
  const d = getDoc(path);
  if (!d || !isDirty(d)) return;
  const content = d.content;
  try {
    await host().writeFile(d.path, content);
    setDoc(d.path, { saved: content });
    host().clearDraft(d.path);
    if (!quiet) ide.set({ status: `Saved ${baseName(d.path)}` });
  } catch (e) {
    ide.set({ status: `Save failed: ${errMsg(e)}` });
    if (!quiet) toast.error('Save failed', { description: errMsg(e) });
  }
}

export async function saveAll(quiet = false): Promise<void> {
  const docs = Object.values(ide.get().docs).filter(isDirty);
  for (const d of docs) await saveDoc(d.path, quiet);
}

export function saveActive(): void {
  const f = currentSearch().file;
  if (f) void saveDoc(f);
}

// ---------- build ----------

function buildTargetPath(): string | null {
  const st = ide.get();
  return st.project?.project_file || st.projectPath || currentSearch().file || null;
}

export async function buildTarget(rebuild: boolean, onlyFile?: string): Promise<boolean> {
  const st = ide.get();
  if (st.building) return false;
  if (st.session === 'on') {
    toast('Stop the debug session first', { description: 'Ctrl+F5 stops debugging.' });
    return false;
  }
  await saveAll(true);
  const target = onlyFile || buildTargetPath();
  if (!target) {
    ide.set({ status: 'No project or file to build' });
    return false;
  }
  ide.set({ building: true, status: onlyFile ? `Translating ${baseName(onlyFile)}...` : rebuild ? 'Rebuilding target...' : 'Building target...' });
  showPanel('build');
  try {
    const res = await host().rpc<BuildResult>('build.run', { path: target, rebuild });
    ide.set({
      build: res,
      buildLog: res.log,
      building: false,
      status: res.success ? `Build OK: ${res.errors} Error(s), ${res.warnings} Warning(s)` : `Build failed: ${res.errors} Error(s), ${res.warnings} Warning(s)`,
    });
    return res.success;
  } catch (e) {
    ide.set({ building: false, build: null, buildLog: `*** Build error: ${errMsg(e)}`, status: 'Build failed' });
    return false;
  }
}

export function translateActive(): void {
  const f = currentSearch().file;
  if (f) void buildTarget(false, f);
}

// ---------- debug session ----------

export function toggleDebug(): void {
  const st = ide.get();
  if (st.session === 'off') setSearch({ mode: 'debug', view: 'debug', panel: 'command' });
  else setSearch({ mode: 'edit', view: 'project', panel: 'build' });
}

export async function startDebug(): Promise<boolean> {
  const st = ide.get();
  if (st.session !== 'off') return true;
  ide.set({ session: 'starting', status: 'Starting debug session...' });
  const ok = await buildTarget(false);
  if (!ok) {
    ide.set({ session: 'off' });
    return false;
  }
  const o = ide.get().project?.options;
  const bps = Object.entries(ide.get().breakpoints).flatMap(([file, lines]) => lines.map((line) => ({ file: realPath(file), line })));
  try {
    const res = await host().rpc<DebugStartResult>('debug.start', {
      path: buildTargetPath(),
      mode: o?.debug_mode || 'simulator',
      interface: o?.hw_interface || 'stlink',
      target: o?.hw_target || 'stm32f1x',
      clock_hz: o?.xtal_hz || 8_000_000,
      breakpoints: bps,
    });
    prevRegs = null;
    ide.set({
      session: 'on',
      disasm: res.disassembly,
      symbols: res.symbols ?? [],
      execLines: res.executable_lines ?? {},
      cmdLog: [{ kind: 'out', text: `Load "${baseName(buildTargetPath() || '')}"\n*** ${res.state.backend === 'hardware' ? 'Hardware' : 'Simulation'} mode` }],
      logic: [],
    });
    await applyBoardInputs();
    await applyState(res.state);
    showPanel('command');
    return true;
  } catch (e) {
    ide.set((s) => ({ session: 'off', status: `Debug failed: ${errMsg(e)}`, buildLog: `${s.buildLog}\n*** Debug start error: ${errMsg(e)}` }));
    showPanel('build');
    return false;
  }
}

function realPath(key: string): string {
  const st = ide.get();
  const all = [...projectFiles(st.project), ...st.tabs];
  return all.find((p) => normPath(p) === key) ?? key;
}

export async function stopDebug(): Promise<void> {
  running = false;
  if (ide.get().session === 'off') return;
  try {
    await host().rpc('debug.stop');
  } catch {
    // the session may already be gone on the host side
  }
  ide.set({ session: 'off', dbg: null, busy: false, periph: null, mem: [null, null, null, null], watchVals: {}, status: 'Debug session stopped' });
}

async function withState(method: 'debug.step' | 'debug.stepOver' | 'debug.stepOut' | 'debug.reset', label: string): Promise<void> {
  const st = ide.get();
  if (st.session !== 'on' || st.busy || running) return;
  ide.set({ busy: true });
  prevRegs = st.dbg?.registers ?? null;
  try {
    const s = await host().rpc<DebugState>(method);
    await applyState(s);
  } catch (e) {
    ide.set({ status: `${label}: ${errMsg(e)}` });
  } finally {
    ide.set({ busy: false });
  }
}

export const stepInto = () => withState('debug.step', 'Step');
export const stepOver = () => withState('debug.stepOver', 'Step Over');
export const stepOut = () => withState('debug.stepOut', 'Step Out');
export const resetCpu = () => withState('debug.reset', 'Reset');

export async function run(): Promise<void> {
  const st = ide.get();
  if (st.session !== 'on' || running) return;
  running = true;
  prevRegs = st.dbg?.registers ?? null;
  ide.set({ busy: true, status: 'Running...', dbg: st.dbg ? { ...st.dbg, running: true } : st.dbg });
  try {
    if (st.dbg?.backend === 'hardware') {
      const s = await host().rpc<DebugState>('debug.run');
      await applyState(s);
      return;
    }
    let lastPc = -1;
    let lastRefresh = 0;
    for (;;) {
      const s = await host().rpc<DebugState>('debug.run', { max_steps: RUN_CHUNK, stop_on_self_loop: false });
      const halted = !s.halt_reason.startsWith('MaxSteps');
      if (halted || !running) {
        await applyState(s);
        break;
      }
      const now = performance.now();
      if (now - lastRefresh > 120) {
        lastRefresh = now;
        await applyState({ ...s, running: true }, true);
      }
      await syncKeypad();
      // An idle loop (B .) needs no busy polling; inputs are still applied between chunks.
      await new Promise((r) => setTimeout(r, s.pc === lastPc ? 60 : 0));
      lastPc = s.pc;
    }
  } catch (e) {
    ide.set({ status: `Run: ${errMsg(e)}` });
  } finally {
    running = false;
    ide.set({ busy: false });
  }
}

export async function stopRun(): Promise<void> {
  if (!running) return;
  running = false;
  if (ide.get().dbg?.backend === 'hardware') {
    try {
      await applyState(await host().rpc<DebugState>('debug.pause'));
    } catch (e) {
      ide.set({ status: `Stop: ${errMsg(e)}` });
    }
  }
}

export function isRunning(): boolean {
  return running;
}

export async function runToCursor(): Promise<void> {
  const st = ide.get();
  const file = currentSearch().file;
  if (st.session !== 'on' || !file || running) return;
  prevRegs = st.dbg?.registers ?? null;
  ide.set({ busy: true });
  try {
    await applyState(await host().rpc<DebugState>('debug.runTo', { file, line: st.cursor.line }));
  } catch (e) {
    ide.set({ status: `Run to Cursor: ${errMsg(e)}` });
  } finally {
    ide.set({ busy: false });
  }
}

const REG_NAMES = ['R0', 'R1', 'R2', 'R3', 'R4', 'R5', 'R6', 'R7', 'R8', 'R9', 'R10', 'R11', 'R12', 'R13 (SP)', 'R14 (LR)', 'R15 (PC)'];

function diffRegs(a: RegisterSnapshot | null, b: RegisterSnapshot, fallback: string[]): string[] {
  if (!a) return fallback;
  const out: string[] = [];
  b.r.forEach((v, i) => {
    if (a.r[i] !== v) out.push(REG_NAMES[i]);
  });
  if (a.xpsr !== b.xpsr) out.push('xPSR');
  if (a.msp !== b.msp) out.push('MSP');
  if (a.psp !== b.psp) out.push('PSP');
  if (a.primask !== b.primask) out.push('PRIMASK');
  return out;
}

export async function applyState(s: DebugState, partial = false): Promise<void> {
  const changed = diffRegs(prevRegs, s.registers, s.registers.changed_regs);
  const next: DebugState = { ...s, registers: { ...s.registers, changed_regs: changed } };
  ide.set({
    dbg: next,
    status: s.running ? 'Running...' : `${s.halt_reason} | PC=${hex32(s.pc)}`,
  });
  if (!partial && s.current_file) {
    const cur = currentSearch().file;
    if (!samePath(cur, s.current_file) || currentSearch().line !== s.current_line) {
      setSearch({ file: s.current_file, line: s.current_line ?? undefined }, true);
    }
  }
  await refreshViews();
  if (!partial) await syncKeypad();
}

export async function refreshViews(): Promise<void> {
  if (ide.get().session !== 'on') return;
  const h = host();
  const st = ide.get();
  const memIdx = memPanelIndex();
  const jobs: Promise<unknown>[] = [
    h
      .rpc<PeripheralSnapshot>('debug.peripherals')
      .then((p) => ide.set({ periph: p }))
      .catch(() => {}),
    h
      .rpc<LogicChannel[]>('debug.logicSamples')
      .then((c) => ide.set({ logic: c }))
      .catch(() => {}),
    refreshWatch(),
  ];
  for (let i = 0; i < 4; i++) if (i === memIdx || st.mem[i]) jobs.push(refreshMemory(i));
  await Promise.all(jobs);
}

function memPanelIndex(): number {
  const p = currentSearch().panel;
  return p && p.startsWith('mem') ? Number(p.slice(3)) - 1 : -1;
}

export async function refreshMemory(idx: number): Promise<void> {
  const st = ide.get();
  if (st.session !== 'on') return;
  try {
    const res = await host().rpc<MemoryReadResult>('debug.memoryRead', { addr: st.memAddrs[idx], len: 256 });
    ide.set((s) => {
      const mem = [...s.mem];
      mem[idx] = res;
      return { mem };
    });
  } catch (e) {
    ide.set({ status: `Memory ${idx + 1}: ${errMsg(e)}` });
  }
}

export function setMemAddr(idx: number, addr: string): void {
  ide.set((s) => {
    const memAddrs = [...s.memAddrs];
    memAddrs[idx] = addr;
    return { memAddrs };
  });
  if (idx === 0) setSearch({ mem: addr.replace(/^0x/i, '') }, true);
  void refreshMemory(idx);
}

export async function writeMemory(addr: number, bytes: number[], idx: number): Promise<void> {
  try {
    await host().rpc('debug.memoryWrite', { addr, bytes });
    await refreshMemory(idx);
    await refreshWatch();
  } catch (e) {
    toast.error('Memory write failed', { description: errMsg(e) });
  }
}

export async function refreshWatch(): Promise<void> {
  const st = ide.get();
  if (st.session !== 'on') return;
  const vals: Record<string, WatchEval | { error: string }> = {};
  for (const ex of st.watchExprs) {
    try {
      vals[ex] = await host().rpc<WatchEval>('debug.evalWatch', { expr: ex });
    } catch (e) {
      vals[ex] = { error: errMsg(e) };
    }
  }
  ide.set({ watchVals: vals });
}

export function setWatchExprs(exprs: string[]): void {
  ide.set({ watchExprs: exprs });
  void refreshWatch();
}

export async function setRegister(name: string, value: string): Promise<void> {
  try {
    prevRegs = ide.get().dbg?.registers ?? null;
    await applyState(await host().rpc<DebugState>('debug.setRegister', { name, value }), true);
  } catch (e) {
    toast.error(`Could not set ${name}`, { description: errMsg(e) });
  }
}

export async function runCommand(cmd: string): Promise<void> {
  const text = cmd.trim();
  if (!text) return;
  ide.set((s) => ({ cmdLog: [...s.cmdLog, { kind: 'in', text }], cmdHistory: [...s.cmdHistory.filter((c) => c !== text), text].slice(-50) }));
  if (ide.get().session !== 'on') {
    ide.set((s) => ({ cmdLog: [...s.cmdLog, { kind: 'err', text: '*** error: no debug session (start one with Ctrl+F5)' }] }));
    return;
  }
  try {
    prevRegs = ide.get().dbg?.registers ?? null;
    const res = await host().rpc<{ output: string; state: DebugState }>('debug.command', { command: text });
    if (res.output) ide.set((s) => ({ cmdLog: [...s.cmdLog, { kind: 'out', text: res.output.replace(/\n$/, '') }] }));
    await applyState(res.state);
  } catch (e) {
    ide.set((s) => ({ cmdLog: [...s.cmdLog, { kind: 'err', text: `*** error: ${errMsg(e)}` }] }));
  }
}

export async function uartSend(text: string): Promise<void> {
  try {
    await host().rpc('debug.uartSend', { text });
  } catch (e) {
    toast.error('UART send failed', { description: errMsg(e) });
  }
}

export async function logicAdd(signal: string): Promise<void> {
  try {
    const ch = await host().rpc<LogicChannel[]>('debug.logicAdd', { signal });
    ide.set({ logic: ch });
  } catch (e) {
    toast.error('Logic Analyzer', { description: errMsg(e) });
  }
}

export async function logicRemove(signal: string): Promise<void> {
  try {
    const ch = await host().rpc<LogicChannel[]>('debug.logicRemove', { signal });
    ide.set({ logic: ch });
  } catch (e) {
    toast.error('Logic Analyzer', { description: errMsg(e) });
  }
}

// ---------- breakpoints ----------

export async function toggleBreakpoint(path: string, line: number): Promise<void> {
  const key = normPath(path);
  const cur = ide.get().breakpoints[key] ?? [];
  const on = !cur.includes(line);
  const lines = on ? [...cur, line].sort((a, b) => a - b) : cur.filter((l) => l !== line);
  ide.set((s) => {
    const bp = { ...s.breakpoints };
    if (lines.length) bp[key] = lines;
    else delete bp[key];
    return { breakpoints: bp };
  });
  if (ide.get().session === 'on') {
    try {
      const res = await host().rpc<{ breakpoints: BreakpointInfo[] }>('debug.setBreakpoint', { file: path, line, enabled: on });
      const dbg = ide.get().dbg;
      if (dbg) ide.set({ dbg: { ...dbg, breakpoints: res.breakpoints } });
    } catch (e) {
      ide.set({ status: `Breakpoint: ${errMsg(e)}` });
    }
  }
}

export function toggleBreakpointAtCursor(): void {
  const f = currentSearch().file;
  if (f) void toggleBreakpoint(f, ide.get().cursor.line);
}

export async function toggleBreakpointAddr(addr: number): Promise<void> {
  if (ide.get().session !== 'on') return;
  const st = ide.get();
  const line = st.disasm.find((d) => d.addr === addr);
  if (line?.file && line.line) {
    await toggleBreakpoint(line.file, line.line);
    return;
  }
  try {
    const res = await host().rpc<{ breakpoints: BreakpointInfo[] }>('debug.setBreakpoint', { addr });
    const dbg = ide.get().dbg;
    if (dbg) ide.set({ dbg: { ...dbg, breakpoints: res.breakpoints } });
  } catch (e) {
    ide.set({ status: `Breakpoint: ${errMsg(e)}` });
  }
}

export async function killAllBreakpoints(): Promise<void> {
  ide.set({ breakpoints: {} });
  if (ide.get().session === 'on') {
    await host()
      .rpc('debug.clearBreakpoints')
      .catch(() => {});
    const dbg = ide.get().dbg;
    if (dbg) ide.set({ dbg: { ...dbg, breakpoints: [] } });
  }
}

// ---------- panels, layout, theme ----------

export function showPanel(panel: PanelId): void {
  ide.set({ panelOpen: true });
  if (currentSearch().panel !== panel) setSearch({ panel }, true);
}

export function setTheme(theme: Theme): void {
  ide.set({ theme });
}

export function cycleTheme(): void {
  const order: Theme[] = ['system', 'light', 'dark'];
  const t = ide.get().theme;
  setTheme(order[(order.indexOf(t) + 1) % order.length]);
}

// ---------- flash and download ----------

export async function downloadToFlash(): Promise<void> {
  const ok = await buildTarget(false);
  if (!ok) return;
  const st = ide.get();
  if (st.hostKind === 'web') {
    ide.set({ flashLog: { percent: 0, lines: [] } });
    openDialog('flash');
    return;
  }
  const o = st.project?.options;
  ide.set({ status: 'Downloading to flash via OpenOCD...' });
  showPanel('build');
  try {
    const res = await host().rpc<{ ok: boolean; log: string }>('flash.program', {
      path: buildTargetPath(),
      interface: o?.hw_interface || 'stlink',
      target: o?.hw_target || 'stm32f1x',
    });
    ide.set((s) => ({ buildLog: `${s.buildLog}\n${res.log}`, status: 'Flash Load finished' }));
  } catch (e) {
    ide.set((s) => ({ buildLog: `${s.buildLog}\nError: Flash Download failed - ${errMsg(e)}`, status: 'Flash Download failed' }));
  }
}

export async function exportFirmware(): Promise<void> {
  const ok = await buildTarget(false);
  if (ok) openDialog('export');
}

export async function showToolchainInfo(): Promise<void> {
  try {
    const info = await host().rpc<unknown>('toolchain.info');
    ide.set({ buildLog: `Toolchain information:\n${JSON.stringify(info, null, 2)}` });
    showPanel('build');
  } catch (e) {
    toast.error('Toolchain info', { description: errMsg(e) });
  }
}

// ---------- project edits ----------

export async function saveOptions(options: UvProject['options']): Promise<void> {
  try {
    const p = await host().rpc<UvProject>('project.saveOptions', { options });
    ide.set({ project: p, status: `Options for Target '${options.target_name}' saved` });
  } catch (e) {
    toast.error('Options not saved', { description: errMsg(e) });
    throw e;
  }
}

export async function newProject(dir: string, name: string, device: string, template: string): Promise<void> {
  const p = await host().rpc<UvProject>('project.new', { dir, name, device, template });
  const path = p.project_file || dir;
  host().noteEngineProject(path);
  await openPath(path, firstSource(p) ?? undefined);
}

export async function addFileToProject(filePath: string, template?: string): Promise<void> {
  const st = ide.get();
  if (!st.project?.project_file) {
    toast('Open a µVision project first', { description: 'Single source files have no project groups.' });
    return;
  }
  try {
    const res = await host().rpc<{ added: { abs_path: string }; project: UvProject }>('project.addFile', {
      project_path: st.project.project_file,
      file_path: filePath,
      ...(template ? { template } : {}),
    });
    ide.set({ project: res.project });
    openFile(res.added.abs_path);
  } catch (e) {
    toast.error('Could not add file', { description: errMsg(e) });
  }
}

export async function addExistingFile(): Promise<void> {
  const st = ide.get();
  if (!st.project) return;
  const p = await host().host<string | null>('pickExistingSourceFile', { rootDir: st.project.root_dir });
  if (p) await addFileToProject(p);
}

export async function pickAndOpen(): Promise<void> {
  if (ide.get().hostKind === 'web') {
    openDialog('workspace');
    return;
  }
  const p = await host().host<string | null>('pickOpenProjectOrFile');
  if (p) await openPath(p);
}
