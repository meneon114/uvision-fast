import { create } from 'zustand';
import type { BuildResult, DebugState, DisasmLine, LogicChannel, MemoryReadResult, PeripheralSnapshot, SymbolInfo, UvProject, WatchEval } from '../host/types';
import { normPath } from '../lib/format';

export interface Doc {
  path: string;
  content: string;
  saved: string;
}

export type Theme = 'system' | 'light' | 'dark';

export type DialogName =
  'options' | 'newProject' | 'addItem' | 'workspace' | 'examples' | 'export' | 'flash' | 'packs' | 'shortcuts' | 'about' | 'prompt' | 'palette';

export interface PromptRequest {
  title: string;
  label: string;
  value: string;
  resolve: (v: string | null) => void;
}

export interface BoardLed {
  id: string;
  pin: string;
  activeLow: boolean;
}

export interface BoardButton {
  id: string;
  pin: string;
  activeLow: boolean;
}

export interface BoardState {
  leds: BoardLed[];
  buttons: BoardButton[];
  keypad: boolean;
}

export const DEFAULT_BOARD: BoardState = {
  leds: [
    { id: 'led1', pin: 'PA5', activeLow: false },
    { id: 'led2', pin: 'PB0', activeLow: false },
    { id: 'led3', pin: 'PC15', activeLow: false },
    { id: 'led4', pin: 'PA0', activeLow: false },
  ],
  buttons: [
    { id: 'btn1', pin: 'PA0', activeLow: false },
    { id: 'btn2', pin: 'PC15', activeLow: true },
  ],
  keypad: false,
};

export const DEFAULT_WATCH = ['R0', 'R1', '*0x20000000', 'GPIOC.ODR'];
export const DEFAULT_MEM = ['0x20000000', '0x08000000', '0x40011000', '0x20004FC0'];

export interface IdeState {
  hostKind: 'web' | 'vscode';
  pwa: { canInstall: boolean; offlineReady: boolean };
  booted: boolean;

  project: UvProject | null;
  projectPath: string | null;
  recent: string[];

  docs: Record<string, Doc>;
  tabs: string[];
  cursor: { line: number; col: number };

  breakpoints: Record<string, number[]>;

  building: boolean;
  build: BuildResult | null;
  buildLog: string;

  session: 'off' | 'starting' | 'on';
  dbg: DebugState | null;
  disasm: DisasmLine[];
  symbols: SymbolInfo[];
  execLines: Record<string, number[]>;
  busy: boolean;

  memAddrs: string[];
  mem: (MemoryReadResult | null)[];
  watchExprs: string[];
  watchVals: Record<string, WatchEval | { error: string }>;
  periph: PeripheralSnapshot | null;
  logic: LogicChannel[];
  cmdLog: { kind: 'in' | 'out' | 'err'; text: string }[];
  cmdHistory: string[];

  status: string;
  theme: Theme;
  sideOpen: boolean;
  panelOpen: boolean;
  disasmOpen: boolean;

  dialog: DialogName | null;
  prompt: PromptRequest | null;
  flashLog: { percent: number; lines: string[] };

  board: BoardState;
  keyDown: string | null;
  buttonsDown: Record<string, boolean>;
}

export const useIde = create<IdeState>()(() => ({
  hostKind: 'web',
  pwa: { canInstall: false, offlineReady: false },
  booted: false,

  project: null,
  projectPath: null,
  recent: [],

  docs: {},
  tabs: [],
  cursor: { line: 1, col: 1 },

  breakpoints: {},

  building: false,
  build: null,
  buildLog: '',

  session: 'off',
  dbg: null,
  disasm: [],
  symbols: [],
  execLines: {},
  busy: false,

  memAddrs: [...DEFAULT_MEM],
  mem: [null, null, null, null],
  watchExprs: [...DEFAULT_WATCH],
  watchVals: {},
  periph: null,
  logic: [],
  cmdLog: [],
  cmdHistory: [],

  status: 'Ready',
  theme: 'system',
  sideOpen: true,
  panelOpen: true,
  disasmOpen: true,

  dialog: null,
  prompt: null,
  flashLog: { percent: 0, lines: [] },

  board: DEFAULT_BOARD,
  keyDown: null,
  buttonsDown: {},
}));

export const ide = {
  get: useIde.getState,
  set: useIde.setState,
  subscribe: useIde.subscribe,
};

export function docKey(path: string): string {
  return normPath(path);
}

export function getDoc(path: string | null | undefined): Doc | undefined {
  if (!path) return undefined;
  return useIde.getState().docs[docKey(path)];
}

export function isDirty(d: Doc | undefined): boolean {
  return !!d && d.content !== d.saved;
}

export function openDialog(name: DialogName): void {
  ide.set({ dialog: name });
}

export function closeDialog(): void {
  ide.set({ dialog: null });
}

export function askText(title: string, label: string, value = ''): Promise<string | null> {
  return new Promise((resolve) => {
    ide.set({ prompt: { title, label, value, resolve }, dialog: 'prompt' });
  });
}
