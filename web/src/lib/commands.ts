import { redo, undo } from '@codemirror/commands';
import { focusEditor, getActiveView, openFindInEditor } from '../components/editor/CodeEditor';
import {
  addExistingFile,
  buildTarget,
  closeTab,
  cycleTheme,
  downloadToFlash,
  exportFirmware,
  isRunning,
  killAllBreakpoints,
  pickAndOpen,
  resetCpu,
  run,
  runToCursor,
  saveActive,
  saveAll,
  showPanel,
  showToolchainInfo,
  stepInto,
  stepOut,
  stepOver,
  stopRun,
  toggleBreakpointAtCursor,
  toggleDebug,
  translateActive,
} from '../state/ide';
import { ide, openDialog } from '../state/store';
import { currentSearch, setSearch } from './nav';
import type { SideView } from './search';

export interface Command {
  id: string;
  title: string;
  category: string;
  icon?: string;
  keys?: string;
  run: () => unknown;
  enabled?: () => boolean;
}

const hasProject = () => !!ide.get().projectPath;
const inDebug = () => ide.get().session === 'on';
const halted = () => inDebug() && !isRunning() && !ide.get().busy;
const editing = () => ide.get().session === 'off' && hasProject();
const hasFile = () => !!currentSearch().file;

export function showView(view: SideView): void {
  ide.set({ sideOpen: true });
  setSearch({ view }, true);
}

export const COMMANDS: Command[] = [
  // File
  { id: 'file.new', title: 'New File...', category: 'File', icon: 'new-file', keys: 'mod+alt+n', run: () => openDialog('addItem'), enabled: hasProject },
  { id: 'file.open', title: 'Open Project or File...', category: 'File', icon: 'folder-opened', keys: 'mod+o', run: pickAndOpen },
  { id: 'file.examples', title: 'Lab Examples...', category: 'File', icon: 'beaker', run: () => openDialog('examples') },
  { id: 'file.save', title: 'Save', category: 'File', icon: 'save', keys: 'mod+s', run: saveActive, enabled: hasFile },
  { id: 'file.saveAll', title: 'Save All', category: 'File', icon: 'save-all', keys: 'mod+alt+s', run: () => saveAll() },
  {
    id: 'file.close',
    title: 'Close File',
    category: 'File',
    icon: 'close',
    keys: 'mod+alt+w',
    run: () => {
      const f = currentSearch().file;
      if (f) closeTab(f);
    },
    enabled: hasFile,
  },
  { id: 'file.export', title: 'Export Firmware (HEX/BIN/AXF)...', category: 'File', icon: 'export', run: exportFirmware, enabled: editing },

  // Edit
  {
    id: 'edit.undo',
    title: 'Undo',
    category: 'Edit',
    icon: 'discard',
    run: () => {
      const v = getActiveView();
      if (v) undo(v);
    },
    enabled: hasFile,
  },
  {
    id: 'edit.redo',
    title: 'Redo',
    category: 'Edit',
    icon: 'redo',
    run: () => {
      const v = getActiveView();
      if (v) redo(v);
    },
    enabled: hasFile,
  },
  { id: 'edit.find', title: 'Find in File', category: 'Edit', icon: 'search', run: () => openFindInEditor(), enabled: hasFile },
  { id: 'edit.findInFiles', title: 'Find in Files', category: 'Edit', icon: 'search', keys: 'mod+shift+f', run: () => showView('find'), enabled: hasProject },
  {
    id: 'edit.toggleBp',
    title: 'Insert/Remove Breakpoint',
    category: 'Edit',
    icon: 'circle-filled',
    keys: 'f9',
    run: toggleBreakpointAtCursor,
    enabled: hasFile,
  },
  { id: 'edit.killBps', title: 'Kill All Breakpoints', category: 'Edit', icon: 'circle-slash', keys: 'mod+shift+f9', run: killAllBreakpoints },

  // View
  { id: 'view.project', title: 'Project Window', category: 'View', icon: 'files', run: () => showView('project') },
  { id: 'view.find', title: 'Find Window', category: 'View', icon: 'search', run: () => showView('find') },
  { id: 'view.debug', title: 'Registers, Watch and Call Stack', category: 'View', icon: 'debug-alt', run: () => showView('debug') },
  { id: 'view.peripherals', title: 'System Viewer and Lab Board', category: 'View', icon: 'circuit-board', run: () => showView('peripherals') },
  { id: 'view.flash', title: 'Flash Window', category: 'View', icon: 'desktop-download', run: () => showView('flash') },
  {
    id: 'view.sidebar',
    title: 'Toggle Side Bar',
    category: 'View',
    icon: 'layout-sidebar-left',
    keys: 'mod+b',
    run: () => ide.set((s) => ({ sideOpen: !s.sideOpen })),
  },
  { id: 'view.panel', title: 'Toggle Panel', category: 'View', icon: 'layout-panel', keys: 'mod+j', run: () => ide.set((s) => ({ panelOpen: !s.panelOpen })) },
  { id: 'view.build', title: 'Build Output Window', category: 'View', icon: 'output', run: () => showPanel('build') },
  { id: 'view.command', title: 'Command Window', category: 'View', icon: 'terminal', run: () => showPanel('command') },
  { id: 'view.mem1', title: 'Memory 1', category: 'View', icon: 'symbol-array', run: () => showPanel('mem1') },
  { id: 'view.mem2', title: 'Memory 2', category: 'View', icon: 'symbol-array', run: () => showPanel('mem2') },
  { id: 'view.mem3', title: 'Memory 3', category: 'View', icon: 'symbol-array', run: () => showPanel('mem3') },
  { id: 'view.mem4', title: 'Memory 4', category: 'View', icon: 'symbol-array', run: () => showPanel('mem4') },
  { id: 'view.uart', title: 'UART #1', category: 'View', icon: 'output', run: () => showPanel('uart') },
  { id: 'view.logic', title: 'Logic Analyzer', category: 'View', icon: 'pulse', run: () => showPanel('logic') },
  { id: 'view.disasm', title: 'Toggle Disassembly Window', category: 'View', icon: 'file-binary', run: () => ide.set((s) => ({ disasmOpen: !s.disasmOpen })) },
  { id: 'view.theme', title: 'Cycle Theme (System, Light, Dark)', category: 'View', icon: 'color-mode', run: cycleTheme },

  // Project
  { id: 'project.new', title: 'New µVision Project...', category: 'Project', icon: 'new-folder', run: () => openDialog('newProject') },
  { id: 'project.addItem', title: 'Add New Item to Group...', category: 'Project', icon: 'new-file', run: () => openDialog('addItem'), enabled: hasProject },
  { id: 'project.addExisting', title: 'Add Existing Files to Group...', category: 'Project', icon: 'file-add', run: addExistingFile, enabled: hasProject },
  {
    id: 'project.options',
    title: 'Options for Target...',
    category: 'Project',
    icon: 'settings',
    keys: 'alt+f7',
    run: () => openDialog('options'),
    enabled: hasProject,
  },
  {
    id: 'project.translate',
    title: 'Translate Current File',
    category: 'Project',
    icon: 'file-binary',
    keys: 'mod+f7',
    run: translateActive,
    enabled: () => editing() && hasFile(),
  },
  { id: 'project.build', title: 'Build Target', category: 'Project', icon: 'package', keys: 'f7', run: () => buildTarget(false), enabled: editing },
  { id: 'project.rebuild', title: 'Rebuild All Target Files', category: 'Project', icon: 'refresh', run: () => buildTarget(true), enabled: editing },
  { id: 'project.packs', title: 'Manage Packs...', category: 'Project', icon: 'extensions', run: () => openDialog('packs') },

  // Flash
  { id: 'flash.download', title: 'Download', category: 'Flash', icon: 'desktop-download', keys: 'f8', run: downloadToFlash, enabled: editing },
  { id: 'flash.export', title: 'Export HEX/BIN/AXF...', category: 'Flash', icon: 'export', run: exportFirmware, enabled: editing },

  // Debug
  {
    id: 'debug.toggle',
    title: 'Start/Stop Debug Session',
    category: 'Debug',
    icon: 'debug-alt',
    keys: 'mod+f5',
    run: toggleDebug,
    enabled: () => hasProject() && ide.get().session !== 'starting',
  },
  { id: 'debug.reset', title: 'Reset CPU', category: 'Debug', icon: 'debug-restart', run: resetCpu, enabled: halted },
  { id: 'debug.run', title: 'Run', category: 'Debug', icon: 'debug-continue', keys: 'f5', run, enabled: halted },
  { id: 'debug.stop', title: 'Stop', category: 'Debug', icon: 'debug-pause', keys: 'shift+f5', run: stopRun, enabled: () => inDebug() && isRunning() },
  { id: 'debug.step', title: 'Step', category: 'Debug', icon: 'debug-step-into', keys: 'f11', run: stepInto, enabled: halted },
  { id: 'debug.stepOver', title: 'Step Over', category: 'Debug', icon: 'debug-step-over', keys: 'f10', run: stepOver, enabled: halted },
  { id: 'debug.stepOut', title: 'Step Out', category: 'Debug', icon: 'debug-step-out', keys: 'mod+f11', run: stepOut, enabled: halted },
  {
    id: 'debug.runToCursor',
    title: 'Run to Cursor Line',
    category: 'Debug',
    icon: 'run-above',
    keys: 'mod+f10',
    run: runToCursor,
    enabled: () => halted() && hasFile(),
  },

  // Tools and Help
  { id: 'tools.palette', title: 'Command Palette', category: 'Tools', icon: 'search', keys: 'mod+shift+p', run: () => openDialog('palette') },
  { id: 'tools.toolchain', title: 'Toolchain Information', category: 'Tools', icon: 'tools', run: showToolchainInfo },
  { id: 'tools.workspace', title: 'Manage Workspace Files...', category: 'Tools', icon: 'database', run: () => openDialog('workspace') },
  { id: 'help.shortcuts', title: 'Keyboard Shortcuts', category: 'Help', icon: 'keyboard', run: () => openDialog('shortcuts') },
  { id: 'help.about', title: 'About µVision Fast', category: 'Help', icon: 'info', run: () => openDialog('about') },
  { id: 'editor.focus', title: 'Focus Editor', category: 'View', icon: 'edit', run: focusEditor },
];

const byId = new Map(COMMANDS.map((c) => [c.id, c]));

export function cmd(id: string): Command {
  const c = byId.get(id);
  if (!c) throw new Error(`Unknown command ${id}`);
  return c;
}

export function exec(id: string): void {
  const c = cmd(id);
  if (c.enabled && !c.enabled()) return;
  void c.run();
}

export function isEnabled(id: string): boolean {
  const c = cmd(id);
  return !c.enabled || c.enabled();
}
