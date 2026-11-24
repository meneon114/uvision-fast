import { getIdeBridge, IdeBridge } from './bridge';
import { KeilEditor } from './editor';
import { ICONS } from './icons';
import { LogicAnalyzerCanvas, LogicChannel } from './logicAnalyzer';

interface ProjectFile {
  name: string;
  path: string;
  abs_path: string;
  file_type: number;
}

interface ProjectGroup {
  name: string;
  files: ProjectFile[];
}

interface TargetOptions {
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

interface UvProject {
  project_file: string | null;
  root_dir: string;
  is_standalone: boolean;
  options: TargetOptions;
  groups: ProjectGroup[];
}

interface Diagnostic {
  file: string;
  line: number;
  column: number;
  severity: string;
  code: string;
  message: string;
}

interface BuildResult {
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

interface DebugState {
  backend: string;
  running: boolean;
  halt_reason: string;
  pc: number;
  current_file: string | null;
  current_line: number | null;
  registers: {
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
  };
  call_stack: { index: number; addr: number; symbol: string; file: string | null; line: number | null }[];
  breakpoints: { addr: number; file: string | null; line: number | null; enabled: boolean }[];
  uart1_output: string;
}

interface DisasmLine {
  addr: number;
  opcode: string;
  mnemonic: string;
  label: string | null;
  file: string | null;
  line: number | null;
}

interface OpenDoc {
  path: string;
  name: string;
  content: string;
  dirty: boolean;
  isC: boolean;
}

class UvisionApp {
  private readonly bridge: IdeBridge = getIdeBridge();
  private reqSeq = 1;
  private pwaCanInstall = false;
  private pwaOfflineReady = false;
  private rpcPending = new Map<number, { res: (v: any) => void; rej: (e: any) => void }>();
  private hostPending = new Map<number, { res: (v: any) => void; rej: (e: any) => void }>();

  private project: UvProject | null = null;
  private openDocs: OpenDoc[] = [];
  private activeDocPath: string | null = null;
  private editor!: KeilEditor;

  private mode: 'edit' | 'debug' = 'edit';
  private leftTab: 'project' | 'registers' | 'books' = 'project';
  private bottomTab = 'build';
  private showDisasm = true;
  private openMenu: string | null = null;
  private regCoreExpanded = true;
  private regXpsrExpanded = true;
  private regInternalExpanded = true;

  private buildLog = 'Ready. Press F7 to Build or Ctrl+F5 to Start Debug.\n';
  private lastBuildDiags: Diagnostic[] = [];
  private cmdLog = 'Keil-compatible Command Window. Type "help" for commands.\n';

  private debugState: DebugState | null = null;
  private disasm: DisasmLine[] = [];
  private execLinesByFile: Record<string, number[]> = {};
  private savedLineBreakpoints = new Map<string, Set<number>>();

  private memAddrs = ['0x20000000', '0x08000000', '0x40010800', '0x20000FC0'];
  private memViews: { addr: number; bytes: number[]; changed_offsets: number[] }[] = [
    { addr: 0x20000000, bytes: [], changed_offsets: [] },
    { addr: 0x08000000, bytes: [], changed_offsets: [] },
    { addr: 0x40010800, bytes: [], changed_offsets: [] },
    { addr: 0x20000fc0, bytes: [], changed_offsets: [] },
  ];
  private watchExprs: string[] = ['R0', 'R1', '*0x20000000', 'GPIOA.ODR'];
  private watchVals: Record<string, { value_hex: string; value_dec: string; kind: string }> = {};
  private peripherals: any = null;
  private activePeriphTab = 'GPIOA';
  private logicChannels: LogicChannel[] = [];
  private logicCanvas: LogicAnalyzerCanvas | null = null;
  private logicHoverInfo = '';

  private cursorLine = 1;
  private cursorCol = 1;
  private statusText = 'Ready';
  private leftWidth = 248;
  private bottomHeight = 220;
  private disasmHeight = 185;

  private rootEl!: HTMLElement;

  constructor() {
    this.setupBridge();
    this.mountShell();
    this.setupKeyboardShortcuts();
    this.bridge.postMessage({ type: 'ready' });
  }

  private setupBridge(): void {
    this.bridge.onMessage((msg) => {
      if (!msg) return;
      if (msg.type === 'rpcResponse') {
        const p = this.rpcPending.get(msg.reqId);
        if (p) {
          this.rpcPending.delete(msg.reqId);
          if (msg.error) p.rej(new Error(msg.error));
          else p.res(msg.result);
        }
      } else if (msg.type === 'hostResponse') {
        const p = this.hostPending.get(msg.reqId);
        if (p) {
          this.hostPending.delete(msg.reqId);
          if (msg.error) p.rej(new Error(msg.error));
          else p.res(msg.result);
        }
      } else if (msg.type === 'openTarget' && msg.path) {
        void this.openProjectOrFile(msg.path);
      } else if (msg.type === 'setPwaState') {
        this.pwaCanInstall = !!msg.canInstall;
        this.pwaOfflineReady = !!msg.offlineReady;
        this.renderStatusBar();
        this.renderMenuBar();
      } else if (msg.type === 'fileChangedOnDisk') {
        const doc = this.openDocs.find((d) => normPath(d.path) === normPath(msg.path));
        if (doc && !doc.dirty && doc.content !== msg.content) {
          doc.content = msg.content;
          if (this.activeDocPath && normPath(this.activeDocPath) === normPath(doc.path)) {
            this.editor.setDocument(doc.content, doc.isC);
            this.syncEditorDebugOverlay();
          }
        }
      } else if (msg.type === 'hostAction') {
        if (msg.action === 'build') void this.runBuild(false);
        else if (msg.action === 'rebuild') void this.runBuild(true);
        else if (msg.action === 'debug') void this.toggleDebug();
        else if (msg.action === 'flash') void this.flashDevice();
      }
    });
  }

  private rpc<T = any>(method: string, params: Record<string, any> = {}): Promise<T> {
    const reqId = this.reqSeq++;
    return new Promise<T>((res, rej) => {
      this.rpcPending.set(reqId, { res, rej });
      this.bridge.postMessage({ type: 'rpc', reqId, method, params });
    });
  }

  private host<T = any>(action: string, payload: Record<string, any> = {}): Promise<T> {
    const reqId = this.reqSeq++;
    return new Promise<T>((res, rej) => {
      this.hostPending.set(reqId, { res, rej });
      this.bridge.postMessage({ type: 'host', reqId, action, payload });
    });
  }

  private mountShell(): void {
    this.rootEl = document.getElementById('uv-root')!;
    this.rootEl.innerHTML = `
      <div class="uv-shell">
        <div class="uv-menubar" id="uv-menubar"></div>
        <div class="uv-toolbar-row" id="uv-toolbar"></div>
        <div class="uv-body">
          <div class="uv-left-pane" id="uv-left-pane" style="width:${this.leftWidth}px">
            <div class="uv-pane-title" id="uv-left-title">Project</div>
            <div class="uv-pane-content" id="uv-left-content"></div>
            <div class="uv-bottom-tabs" id="uv-left-tabs"></div>
          </div>
          <div class="uv-splitter-v" id="uv-split-left"></div>
          <div class="uv-right-col">
            <div class="uv-center-pane">
              <div class="uv-disasm-pane" id="uv-disasm-pane" style="display:none;height:${this.disasmHeight}px">
                <div class="uv-pane-title">Disassembly</div>
                <div class="uv-disasm-list" id="uv-disasm-list"></div>
              </div>
              <div class="uv-splitter-h" id="uv-split-disasm" style="display:none"></div>
              <div class="uv-doc-tabs" id="uv-doc-tabs"></div>
              <div class="uv-editor-host" id="uv-editor-host"></div>
            </div>
            <div class="uv-splitter-h" id="uv-split-bottom"></div>
            <div class="uv-bottom-pane" id="uv-bottom-pane" style="height:${this.bottomHeight}px">
              <div class="uv-pane-title" id="uv-bottom-title">Build Output</div>
              <div class="uv-pane-content" id="uv-bottom-content"></div>
              <div class="uv-bottom-tabs" id="uv-bottom-tabs"></div>
            </div>
          </div>
        </div>
        <div class="uv-statusbar" id="uv-statusbar"></div>
      </div>
      <div id="uv-modal-host"></div>
    `;

    const editorHost = document.getElementById('uv-editor-host')!;
    this.editor = new KeilEditor(editorHost, {
      onChange: (content) => {
        const doc = this.getActiveDoc();
        if (doc) {
          doc.content = content;
          doc.dirty = true;
          this.renderDocTabs();
        }
      },
      onToggleBreakpoint: (line) => {
        void this.toggleBreakpointAtLine(line);
      },
      onCursorMove: (line, col) => {
        this.cursorLine = line;
        this.cursorCol = col;
        this.renderStatusBar();
      },
      onSave: () => {
        void this.saveCurrentDoc();
      },
    });

    this.bindSplitters();
    document.addEventListener('click', (e) => {
      const target = e.target as HTMLElement;
      if (!target.closest('#uv-menubar') && this.openMenu) {
        this.openMenu = null;
        this.renderMenuBar();
      }
    });

    this.renderAll();
  }

  private bindSplitters(): void {
    const splitLeft = document.getElementById('uv-split-left')!;
    const leftPane = document.getElementById('uv-left-pane')!;
    splitLeft.addEventListener('mousedown', (e) => {
      e.preventDefault();
      const startX = e.clientX;
      const startW = leftPane.getBoundingClientRect().width;
      const onMove = (mv: MouseEvent) => {
        this.leftWidth = Math.max(150, Math.min(520, startW + (mv.clientX - startX)));
        leftPane.style.width = `${this.leftWidth}px`;
      };
      const onUp = () => {
        window.removeEventListener('mousemove', onMove);
        window.removeEventListener('mouseup', onUp);
      };
      window.addEventListener('mousemove', onMove);
      window.addEventListener('mouseup', onUp);
    });

    const splitBottom = document.getElementById('uv-split-bottom')!;
    const bottomPane = document.getElementById('uv-bottom-pane')!;
    splitBottom.addEventListener('mousedown', (e) => {
      e.preventDefault();
      const startY = e.clientY;
      const startH = bottomPane.getBoundingClientRect().height;
      const onMove = (mv: MouseEvent) => {
        this.bottomHeight = Math.max(90, Math.min(520, startH - (mv.clientY - startY)));
        bottomPane.style.height = `${this.bottomHeight}px`;
        this.logicCanvas?.draw();
      };
      const onUp = () => {
        window.removeEventListener('mousemove', onMove);
        window.removeEventListener('mouseup', onUp);
      };
      window.addEventListener('mousemove', onMove);
      window.addEventListener('mouseup', onUp);
    });

    const splitDisasm = document.getElementById('uv-split-disasm')!;
    const disasmPane = document.getElementById('uv-disasm-pane')!;
    splitDisasm.addEventListener('mousedown', (e) => {
      e.preventDefault();
      const startY = e.clientY;
      const startH = disasmPane.getBoundingClientRect().height;
      const onMove = (mv: MouseEvent) => {
        this.disasmHeight = Math.max(70, Math.min(420, startH + (mv.clientY - startY)));
        disasmPane.style.height = `${this.disasmHeight}px`;
      };
      const onUp = () => {
        window.removeEventListener('mousemove', onMove);
        window.removeEventListener('mouseup', onUp);
      };
      window.addEventListener('mousemove', onMove);
      window.addEventListener('mouseup', onUp);
    });
  }

  private setupKeyboardShortcuts(): void {
    window.addEventListener('keydown', (e) => {
      if (e.key === 'F7' && e.altKey) {
        e.preventDefault();
        this.openOptionsDialog();
      } else if (e.key === 'F7' && !e.ctrlKey && !e.shiftKey) {
        e.preventDefault();
        void this.runBuild(false);
      } else if (e.key === 'F5' && e.ctrlKey) {
        e.preventDefault();
        void this.toggleDebug();
      } else if (e.key === 'F5' && !e.ctrlKey && this.mode === 'debug') {
        e.preventDefault();
        void this.debugAction('debug.run');
      } else if (e.key === 'F11' && e.ctrlKey && this.mode === 'debug') {
        e.preventDefault();
        void this.debugAction('debug.stepOut');
      } else if (e.key === 'F11' && !e.ctrlKey && this.mode === 'debug') {
        e.preventDefault();
        void this.debugAction('debug.step');
      } else if (e.key === 'F10' && this.mode === 'debug') {
        e.preventDefault();
        void this.debugAction('debug.stepOver');
      } else if (e.key === 'F9') {
        e.preventDefault();
        void this.toggleBreakpointAtLine(this.editor.getCurrentLine());
      } else if (e.key.toLowerCase() === 's' && e.ctrlKey) {
        e.preventDefault();
        void this.saveCurrentDoc();
      }
    });
  }

  private renderAll(): void {
    this.renderMenuBar();
    this.renderToolbar();
    this.renderLeftPane();
    this.renderDisasm();
    this.renderDocTabs();
    this.renderBottomPane();
    this.renderStatusBar();
  }

  private renderMenuBar(): void {
    const menus: { id: string; label: string; items: { label?: string; shortcut?: string; sep?: boolean; disabled?: boolean; fn?: () => void }[] }[] = [
      {
        id: 'file',
        label: 'File',
        items: this.bridge.isWeb
          ? [
              { label: 'New File...', fn: () => this.openAddItemDialog() },
              { label: 'Open Workspace / Project...', fn: () => void this.pickAndOpen() },
              {
                label: 'Starter Examples...',
                fn: () => {
                  void this.host<string | null>('openExamples').then((p) => {
                    if (p) void this.openProjectOrFile(p);
                  });
                },
              },
              {
                label: 'Import .s / .uvprojx from Disk...',
                fn: () => {
                  void this.host<string | null>('importLocalFile').then((p) => {
                    if (p) void this.openProjectOrFile(p);
                  });
                },
              },
              { sep: true },
              { label: 'Save', shortcut: 'Ctrl+S', fn: () => void this.saveCurrentDoc() },
              { label: 'Save All', fn: () => void this.saveAllDocs() },
              { sep: true },
              {
                label: 'Download Active Source File',
                fn: () => {
                  const doc = this.getActiveDoc();
                  if (doc) {
                    void this.host('downloadSource', {
                      path: doc.path,
                      name: doc.name,
                      content: doc.content,
                    });
                  }
                },
              },
              {
                label: 'Export Build (.hex / .bin / .axf)...',
                fn: () => void this.downloadBuildArtifacts(),
              },
              ...(this.pwaCanInstall
                ? [
                    { sep: true },
                    {
                      label: 'Install Offline App (PWA)...',
                      fn: () => void this.host('installPwa'),
                    },
                  ]
                : []),
            ]
          : [
              { label: 'New File...', fn: () => this.openAddItemDialog() },
              { label: 'Open Project / File...', fn: () => void this.pickAndOpen() },
              { sep: true },
              { label: 'Save', shortcut: 'Ctrl+S', fn: () => void this.saveCurrentDoc() },
              { label: 'Save All', fn: () => void this.saveAllDocs() },
              { sep: true },
              {
                label: 'Open Active File in VS Code',
                fn: () => {
                  if (this.activeDocPath) void this.host('openInVsCode', { path: this.activeDocPath });
                },
              },
            ],
      },
      {
        id: 'edit',
        label: 'Edit',
        items: [
          { label: 'Find...', shortcut: 'Ctrl+F', fn: () => this.editor.openFind() },
          { label: 'Toggle Breakpoint', shortcut: 'F9', fn: () => void this.toggleBreakpointAtLine(this.editor.getCurrentLine()) },
        ],
      },
      {
        id: 'view',
        label: 'View',
        items: [
          { label: 'Project Window', fn: () => { this.leftTab = 'project'; this.renderLeftPane(); } },
          { label: 'Registers Window', fn: () => { this.leftTab = 'registers'; this.renderLeftPane(); } },
          { label: 'Disassembly Window', fn: () => { this.showDisasm = !this.showDisasm; this.renderDisasm(); } },
          { sep: true },
          { label: 'Build Output', fn: () => { this.bottomTab = 'build'; this.renderBottomPane(); } },
          { label: 'Command Window', fn: () => { this.bottomTab = 'command'; this.renderBottomPane(); } },
          { label: 'Memory 1', fn: () => { this.bottomTab = 'mem1'; this.renderBottomPane(); } },
          { label: 'Memory 2', fn: () => { this.bottomTab = 'mem2'; this.renderBottomPane(); } },
          { label: 'Watch 1', fn: () => { this.bottomTab = 'watch'; this.renderBottomPane(); } },
          { label: 'Call Stack + Locals', fn: () => { this.bottomTab = 'callstack'; this.renderBottomPane(); } },
          { label: 'UART #1', fn: () => { this.bottomTab = 'uart'; this.renderBottomPane(); } },
          { label: 'Logic Analyzer', fn: () => { this.bottomTab = 'logic'; this.renderBottomPane(); } },
        ],
      },
      {
        id: 'project',
        label: 'Project',
        items: [
          { label: 'New µVision Project...', fn: () => this.openNewProjectDialog() },
          { label: 'Open Project...', fn: () => void this.pickAndOpen() },
          { label: 'Add New Item to Group...', fn: () => this.openAddItemDialog() },
          { label: 'Add Existing File to Group...', fn: () => void this.addExistingFile() },
          { sep: true },
          { label: 'Build Target', shortcut: 'F7', fn: () => void this.runBuild(false) },
          { label: 'Rebuild all target files', fn: () => void this.runBuild(true) },
          { sep: true },
          { label: 'Options for Target...', shortcut: 'Alt+F7', fn: () => this.openOptionsDialog() },
          { label: 'Pack Installer...', fn: () => void this.openPackDialog() },
        ],
      },
      {
        id: 'flash',
        label: 'Flash',
        items: [
          {
            label: this.bridge.isWeb
              ? 'Download (WebSerial STM32 Bootloader / Export HEX)'
              : 'Download (ST-Link / OpenOCD)',
            fn: () => void this.flashDevice(),
          },
          { label: 'Configure Flash Tools...', fn: () => this.openOptionsDialog('debug') },
        ],
      },
      {
        id: 'debug',
        label: 'Debug',
        items: [
          { label: 'Start/Stop Debug Session', shortcut: 'Ctrl+F5', fn: () => void this.toggleDebug() },
          { sep: true },
          { label: 'Run', shortcut: 'F5', disabled: this.mode !== 'debug', fn: () => void this.debugAction('debug.run') },
          { label: 'Stop', disabled: this.mode !== 'debug', fn: () => void this.debugAction('debug.pause') },
          { label: 'Step', shortcut: 'F11', disabled: this.mode !== 'debug', fn: () => void this.debugAction('debug.step') },
          { label: 'Step Over', shortcut: 'F10', disabled: this.mode !== 'debug', fn: () => void this.debugAction('debug.stepOver') },
          { label: 'Step Out', shortcut: 'Ctrl+F11', disabled: this.mode !== 'debug', fn: () => void this.debugAction('debug.stepOut') },
          { label: 'Run to Cursor Line', disabled: this.mode !== 'debug', fn: () => void this.runToCursor() },
          { label: 'Reset CPU', disabled: this.mode !== 'debug', fn: () => void this.debugAction('debug.reset') },
          { sep: true },
          { label: 'Insert/Remove Breakpoint', shortcut: 'F9', fn: () => void this.toggleBreakpointAtLine(this.editor.getCurrentLine()) },
          { label: 'Kill All Breakpoints', fn: () => void this.clearAllBreakpoints() },
        ],
      },
      {
        id: 'peripherals',
        label: 'Peripherals',
        items: ['RCC', 'GPIOA', 'GPIOB', 'GPIOC', 'TIM1', 'TIM2', 'TIM3', 'TIM4', 'USART1', 'SysTick'].map((p) => ({
          label: `System Viewer → ${p}`,
          fn: () => {
            this.activePeriphTab = p;
            this.bottomTab = 'peripherals';
            this.renderBottomPane();
          },
        })),
      },
      {
        id: 'tools',
        label: 'Tools',
        items: [
          { label: 'Pack Installer', fn: () => void this.openPackDialog() },
          { label: 'Check Toolchain Info (uvcli info)', fn: () => void this.showToolchainInfo() },
        ],
      },
      {
        id: 'help',
        label: 'Help',
        items: [
          { label: 'uVision Fast Shortcuts & CLI Guide', fn: () => this.showHelpModal() },
        ],
      },
    ];

    const bar = document.getElementById('uv-menubar')!;
    bar.innerHTML = '';
    for (const m of menus) {
      const item = document.createElement('div');
      item.className = `uv-menu-item${this.openMenu === m.id ? ' open' : ''}`;
      item.textContent = m.label;
      item.addEventListener('click', (e) => {
        e.stopPropagation();
        this.openMenu = this.openMenu === m.id ? null : m.id;
        this.renderMenuBar();
      });

      if (this.openMenu === m.id) {
        const dd = document.createElement('div');
        dd.className = 'uv-dropdown';
        for (const it of m.items) {
          if (it.sep) {
            const sep = document.createElement('div');
            sep.className = 'uv-dd-sep';
            dd.appendChild(sep);
          } else {
            const row = document.createElement('div');
            row.className = `uv-dd-item${it.disabled ? ' disabled' : ''}`;
            const lbl = document.createElement('span');
            lbl.textContent = it.label || '';
            const sc = document.createElement('span');
            sc.className = 'uv-dd-shortcut';
            sc.textContent = it.shortcut || '';
            row.append(lbl, sc);
            if (!it.disabled && it.fn) {
              row.addEventListener('click', (ev) => {
                ev.stopPropagation();
                this.openMenu = null;
                this.renderMenuBar();
                it.fn!();
              });
            }
            dd.appendChild(row);
          }
        }
        item.appendChild(dd);
      }
      bar.appendChild(item);
    }
  }

  private renderToolbar(): void {
    const tb = document.getElementById('uv-toolbar')!;
    tb.innerHTML = '';

    const addBtn = (icon: string, title: string, fn: () => void, active = false, disabled = false) => {
      const b = document.createElement('button');
      b.className = `uv-tbtn${active ? ' active' : ''}`;
      b.title = title;
      b.disabled = disabled;
      b.innerHTML = icon;
      b.addEventListener('click', fn);
      tb.appendChild(b);
    };
    const addSep = () => {
      const s = document.createElement('div');
      s.className = 'uv-tsep';
      tb.appendChild(s);
    };

    if (this.mode === 'edit') {
      addBtn(ICONS.newFile, 'New File', () => this.openAddItemDialog());
      addBtn(ICONS.open, 'Open Project / File', () => void this.pickAndOpen());
      addBtn(ICONS.save, 'Save (Ctrl+S)', () => void this.saveCurrentDoc());
      addBtn(ICONS.saveAll, 'Save All', () => void this.saveAllDocs());
      addSep();
      addBtn(ICONS.find, 'Find (Ctrl+F)', () => this.editor.openFind());
      addSep();

      const sel = document.createElement('select');
      sel.className = 'uv-target-select';
      const opt = document.createElement('option');
      opt.textContent = this.project?.options.target_name || 'Target 1';
      sel.appendChild(opt);
      tb.appendChild(sel);

      addBtn(ICONS.translate, 'Translate / Build Current', () => void this.runBuild(false));
      addBtn(ICONS.build, 'Build Target (F7)', () => void this.runBuild(false));
      addBtn(ICONS.rebuild, 'Rebuild All Target Files', () => void this.runBuild(true));
      addBtn(
        ICONS.load,
        this.bridge.isWeb
          ? 'Flash / Download to STM32 (WebSerial / HEX)'
          : 'Flash / Download to STM32 (OpenOCD)',
        () => void this.flashDevice()
      );
      addSep();
      addBtn(ICONS.options, 'Options for Target (Alt+F7)', () => this.openOptionsDialog());
      addBtn(ICONS.pack, 'Pack Installer', () => void this.openPackDialog());
      addSep();
      addBtn(ICONS.debugToggle, 'Start/Stop Debug Session (Ctrl+F5)', () => void this.toggleDebug(), false);
    } else {
      addBtn(ICONS.reset, 'Reset CPU (RST)', () => void this.debugAction('debug.reset'));
      addBtn(ICONS.run, 'Run (F5)', () => void this.debugAction('debug.run'));
      addBtn(ICONS.stop, 'Stop / Halt', () => void this.debugAction('debug.pause'));
      addSep();
      addBtn(ICONS.step, 'Step Into (F11)', () => void this.debugAction('debug.step'));
      addBtn(ICONS.stepOver, 'Step Over (F10)', () => void this.debugAction('debug.stepOver'));
      addBtn(ICONS.stepOut, 'Step Out (Ctrl+F11)', () => void this.debugAction('debug.stepOut'));
      addBtn(ICONS.runToCursor, 'Run to Cursor Line', () => void this.runToCursor());
      addSep();
      addBtn(ICONS.disasm, 'Toggle Disassembly Window', () => { this.showDisasm = !this.showDisasm; this.renderDisasm(); }, this.showDisasm);
      addBtn(ICONS.memory, 'Memory 1', () => { this.bottomTab = 'mem1'; this.renderBottomPane(); }, this.bottomTab.startsWith('mem'));
      addBtn(ICONS.watch, 'Watch 1', () => { this.bottomTab = 'watch'; this.renderBottomPane(); }, this.bottomTab === 'watch');
      addBtn(ICONS.uart, 'Serial UART #1', () => { this.bottomTab = 'uart'; this.renderBottomPane(); }, this.bottomTab === 'uart');
      addBtn(ICONS.logic, 'Logic Analyzer', () => { this.bottomTab = 'logic'; this.renderBottomPane(); }, this.bottomTab === 'logic');
      addBtn(ICONS.periph, 'System Viewer (Peripherals)', () => { this.bottomTab = 'peripherals'; this.renderBottomPane(); }, this.bottomTab === 'peripherals');
      addSep();
      addBtn(ICONS.debugToggle, 'Stop Debug Session (Ctrl+F5)', () => void this.toggleDebug(), true);
    }

    const pill = document.createElement('span');
    pill.className = `uv-mode-pill${this.mode === 'debug' ? ' debug' : ''}`;
    pill.textContent =
      this.mode === 'debug'
        ? `Debug: ${this.debugState?.backend === 'hardware' ? 'ST-Link SWD' : 'Simulator'}`
        : `Edit: ${this.project?.options.device || 'STM32F100RB'}`;
    tb.appendChild(pill);
  }

  private renderLeftPane(): void {
    const titleEl = document.getElementById('uv-left-title')!;
    const contentEl = document.getElementById('uv-left-content')!;
    const tabsEl = document.getElementById('uv-left-tabs')!;

    tabsEl.innerHTML = '';
    const leftTabs =
      this.mode === 'debug'
        ? [
            { id: 'registers', label: 'Registers' },
            { id: 'project', label: 'Project' },
          ]
        : [
            { id: 'project', label: 'Project' },
            { id: 'books', label: 'Books' },
          ];
    for (const t of leftTabs) {
      const b = document.createElement('button');
      b.className = `uv-tab-btn${this.leftTab === t.id ? ' active' : ''}`;
      b.textContent = t.label;
      b.addEventListener('click', () => {
        this.leftTab = t.id as any;
        this.renderLeftPane();
      });
      tabsEl.appendChild(b);
    }

    if (this.leftTab === 'registers') {
      titleEl.innerHTML = '<span>Registers</span>';
      this.renderRegistersTree(contentEl);
      return;
    }

    if (this.leftTab === 'books') {
      titleEl.innerHTML = '<span>Books & Guides</span>';
      contentEl.innerHTML = `<div class="uv-tree">
        <div class="uv-tree-row"><b>STM32F100xx Reference Manual</b></div>
        <div class="uv-tree-row">Cortex-M3 Generic User Guide</div>
        <div class="uv-tree-row">Keil MDK 5.39 (${escHtml(this.project?.options.device || 'STM32F100RB')})</div>
        <div class="uv-tree-row">CSE 3108 ARM Assembly Lab</div>
      </div>`;
      return;
    }

    // Project Tree
    titleEl.innerHTML = `<span>Project</span><span class="uv-tree-actions">
      <button class="uv-mini-btn" id="uv-btn-newproj" title="New Project">New</button>
      <button class="uv-mini-btn" id="uv-btn-addfile" title="Add New File">+File</button>
    </span>`;
    document.getElementById('uv-btn-newproj')?.addEventListener('click', () => this.openNewProjectDialog());
    document.getElementById('uv-btn-addfile')?.addEventListener('click', () => this.openAddItemDialog());

    if (!this.project) {
      contentEl.innerHTML = `<div style="padding:10px;color:#555;">
        <p style="margin-bottom:8px;">No project open.</p>
        <button class="uv-btn" id="uv-empty-open">Open .uvprojx / .s / .c</button>
        <button class="uv-btn" id="uv-empty-new" style="margin-left:4px;">New Project</button>
      </div>`;
      document.getElementById('uv-empty-open')?.addEventListener('click', () => void this.pickAndOpen());
      document.getElementById('uv-empty-new')?.addEventListener('click', () => this.openNewProjectDialog());
      return;
    }

    const tree = document.createElement('div');
    tree.className = 'uv-tree';

    const targetRow = document.createElement('div');
    targetRow.className = 'uv-tree-row';
    targetRow.innerHTML = `<span style="width:14px;height:14px;display:inline-block">${ICONS.target}</span> <b>${escHtml(this.project.options.target_name)}</b> (${escHtml(this.project.options.device)})`;
    tree.appendChild(targetRow);

    for (const grp of this.project.groups) {
      const gRow = document.createElement('div');
      gRow.className = 'uv-tree-row';
      gRow.style.paddingLeft = '14px';
      gRow.innerHTML = `<span style="width:14px;height:14px;display:inline-block">${ICONS.folder}</span> ${escHtml(grp.name)}`;
      tree.appendChild(gRow);

      for (const f of grp.files) {
        const fRow = document.createElement('div');
        const isSel = this.activeDocPath && normPath(this.activeDocPath) === normPath(f.abs_path);
        fRow.className = `uv-tree-row${isSel ? ' selected' : ''}`;
        fRow.style.paddingLeft = '28px';
        const ico = f.file_type === 1 ? ICONS.fileC : ICONS.fileAsm;
        fRow.innerHTML = `<span style="width:14px;height:14px;display:inline-block">${ico}</span> ${escHtml(f.name)}`;
        fRow.addEventListener('click', () => void this.openFileInEditor(f.abs_path));
        tree.appendChild(fRow);
      }
    }

    contentEl.innerHTML = '';
    contentEl.appendChild(tree);
  }

  private renderRegistersTree(container: HTMLElement): void {
    const r = this.debugState?.registers;
    if (!r) {
      container.innerHTML = '<div style="padding:8px;color:#666;">Start Debug (Ctrl+F5) to inspect registers.</div>';
      return;
    }
    const changed = new Set(r.changed_regs);
    const names = [
      'R0', 'R1', 'R2', 'R3', 'R4', 'R5', 'R6', 'R7',
      'R8', 'R9', 'R10', 'R11', 'R12', 'R13 (SP)', 'R14 (LR)', 'R15 (PC)',
    ];

    const table = document.createElement('table');
    table.className = 'uv-reg-table';
    table.innerHTML = '<thead><tr><th>Register</th><th>Value</th></tr></thead><tbody></tbody>';
    const tbody = table.querySelector('tbody')!;

    const addGroupRow = (label: string, expanded: boolean, toggleFn: () => void) => {
      const tr = document.createElement('tr');
      tr.className = 'uv-reg-group';
      tr.innerHTML = `<td colspan="2">${expanded ? '▾' : '▸'} ${label}</td>`;
      tr.addEventListener('click', toggleFn);
      tbody.appendChild(tr);
    };

    const addRegRow = (label: string, regEditName: string | null, valueStr: string, isChanged: boolean, indent = 8) => {
      const tr = document.createElement('tr');
      const tdN = document.createElement('td');
      tdN.style.paddingLeft = `${indent}px`;
      tdN.textContent = label;
      const tdV = document.createElement('td');
      tdV.className = `uv-reg-val${isChanged ? ' changed' : ''}`;
      tdV.textContent = valueStr;
      if (regEditName) {
        tdV.title = 'Double-click to edit register';
        tdV.addEventListener('dblclick', () => void this.promptEditRegister(regEditName, valueStr));
      }
      tr.append(tdN, tdV);
      tbody.appendChild(tr);
    };

    addGroupRow('Core', this.regCoreExpanded, () => {
      this.regCoreExpanded = !this.regCoreExpanded;
      this.renderLeftPane();
    });
    if (this.regCoreExpanded) {
      for (let i = 0; i < 16; i++) {
        const editKey = i === 13 ? 'SP' : i === 14 ? 'LR' : i === 15 ? 'PC' : `R${i}`;
        addRegRow(names[i], editKey, hex32(r.r[i]), changed.has(names[i]), 14);
      }
      addRegRow('xPSR', 'xPSR', hex32(r.xpsr), changed.has('xPSR'), 14);
      if (this.regXpsrExpanded) {
        addRegRow('N (Negative)', null, r.n ? '1' : '0', changed.has('xPSR'), 24);
        addRegRow('Z (Zero)', null, r.z ? '1' : '0', changed.has('xPSR'), 24);
        addRegRow('C (Carry)', null, r.c ? '1' : '0', changed.has('xPSR'), 24);
        addRegRow('V (Overflow)', null, r.v ? '1' : '0', changed.has('xPSR'), 24);
        addRegRow('Q (Saturate)', null, r.q ? '1' : '0', changed.has('xPSR'), 24);
        addRegRow('T (Thumb)', null, r.t ? '1' : '0', false, 24);
      }
      addRegRow('MSP', 'MSP', hex32(r.msp), changed.has('MSP'), 14);
      addRegRow('PSP', 'PSP', hex32(r.psp), changed.has('PSP'), 14);
      addRegRow('PRIMASK', 'PRIMASK', hex32(r.primask), changed.has('PRIMASK'), 14);
    }

    addGroupRow('Internal', this.regInternalExpanded, () => {
      this.regInternalExpanded = !this.regInternalExpanded;
      this.renderLeftPane();
    });
    if (this.regInternalExpanded) {
      addRegRow('Mode', null, r.mode, false, 14);
      addRegRow('States', null, `${r.cycles}`, false, 14);
      addRegRow('Sec', null, r.time_sec.toFixed(8), false, 14);
    }

    container.innerHTML = '';
    container.appendChild(table);
  }

  private renderDisasm(): void {
    const pane = document.getElementById('uv-disasm-pane')!;
    const split = document.getElementById('uv-split-disasm')!;
    const list = document.getElementById('uv-disasm-list')!;

    if (this.mode !== 'debug' || !this.showDisasm) {
      pane.style.display = 'none';
      split.style.display = 'none';
      return;
    }

    pane.style.display = 'flex';
    pane.style.height = `${this.disasmHeight}px`;
    split.style.display = 'block';

    const pc = this.debugState?.pc ?? 0;
    const bpSet = new Set((this.debugState?.breakpoints || []).map((b) => b.addr));
    list.innerHTML = '';

    let pcRowEl: HTMLElement | null = null;
    for (const d of this.disasm) {
      if (d.label) {
        const lbl = document.createElement('div');
        lbl.className = 'uv-disasm-label';
        lbl.textContent = `${d.label}:`;
        list.appendChild(lbl);
      }
      const row = document.createElement('div');
      const isPc = d.addr === pc;
      const isBp = bpSet.has(d.addr);
      row.className = `uv-disasm-row${isPc ? ' pc' : ''}`;

      const gutter = document.createElement('span');
      gutter.className = 'uv-disasm-gutter';
      if (isBp) {
        const dot = document.createElement('span');
        dot.className = 'uv-bp-dot';
        gutter.appendChild(dot);
      }
      if (isPc) {
        const arr = document.createElement('span');
        arr.style.width = '12px';
        arr.style.height = '12px';
        arr.style.display = 'inline-block';
        arr.innerHTML = ICONS.pcArrow;
        gutter.appendChild(arr);
      }

      const addr = document.createElement('span');
      addr.className = 'uv-disasm-addr';
      addr.textContent = hex32(d.addr) + ':';

      const op = document.createElement('span');
      op.className = 'uv-disasm-op';
      op.textContent = d.opcode;

      const mnem = document.createElement('span');
      mnem.className = 'uv-disasm-mnem';
      mnem.textContent = d.mnemonic;

      row.append(gutter, addr, op, mnem);
      row.addEventListener('click', () => void this.toggleBreakpointByAddr(d.addr));
      if (isPc) pcRowEl = row;
      list.appendChild(row);
    }

    if (pcRowEl) {
      pcRowEl.scrollIntoView({ block: 'nearest' });
    }
  }

  private renderDocTabs(): void {
    const bar = document.getElementById('uv-doc-tabs')!;
    bar.innerHTML = '';
    for (const doc of this.openDocs) {
      const tab = document.createElement('div');
      const active = this.activeDocPath && normPath(this.activeDocPath) === normPath(doc.path);
      tab.className = `uv-doc-tab${active ? ' active' : ''}`;

      const lbl = document.createElement('span');
      lbl.textContent = `${doc.name}${doc.dirty ? ' *' : ''}`;
      lbl.addEventListener('click', () => this.activateDoc(doc.path));

      const cls = document.createElement('span');
      cls.className = 'uv-doc-close';
      cls.textContent = '×';
      cls.addEventListener('click', (e) => {
        e.stopPropagation();
        this.closeDoc(doc.path);
      });

      tab.append(lbl, cls);
      bar.appendChild(tab);
    }
  }

  private renderBottomPane(): void {
    const titleEl = document.getElementById('uv-bottom-title')!;
    const contentEl = document.getElementById('uv-bottom-content')!;
    const tabsEl = document.getElementById('uv-bottom-tabs')!;

    const tabs =
      this.mode === 'debug'
        ? [
            { id: 'command', label: 'Command' },
            { id: 'mem1', label: 'Memory 1' },
            { id: 'mem2', label: 'Memory 2' },
            { id: 'mem3', label: 'Memory 3' },
            { id: 'mem4', label: 'Memory 4' },
            { id: 'watch', label: 'Watch 1' },
            { id: 'callstack', label: 'Call Stack' },
            { id: 'uart', label: 'UART #1' },
            { id: 'logic', label: 'Logic Analyzer' },
            { id: 'peripherals', label: 'System Viewer' },
            { id: 'build', label: 'Build Output' },
          ]
        : [
            { id: 'build', label: 'Build Output' },
            { id: 'peripherals', label: 'System Viewer' },
          ];

    if (!tabs.some((t) => t.id === this.bottomTab)) {
      this.bottomTab = tabs[0].id;
    }

    tabsEl.innerHTML = '';
    for (const t of tabs) {
      const b = document.createElement('button');
      b.className = `uv-tab-btn${this.bottomTab === t.id ? ' active' : ''}`;
      b.textContent = t.label;
      b.addEventListener('click', () => {
        this.bottomTab = t.id;
        this.renderBottomPane();
      });
      tabsEl.appendChild(b);
    }

    const activeLabel = tabs.find((t) => t.id === this.bottomTab)?.label || 'Output';
    titleEl.textContent = activeLabel;

    if (this.bottomTab === 'build') {
      this.renderBuildOutput(contentEl);
    } else if (this.bottomTab === 'command') {
      this.renderCommandTab(contentEl);
    } else if (this.bottomTab.startsWith('mem')) {
      const idx = parseInt(this.bottomTab.slice(3), 10) - 1;
      this.renderMemoryTab(contentEl, idx);
    } else if (this.bottomTab === 'watch') {
      this.renderWatchTab(contentEl);
    } else if (this.bottomTab === 'callstack') {
      this.renderCallStackTab(contentEl);
    } else if (this.bottomTab === 'uart') {
      this.renderUartTab(contentEl);
    } else if (this.bottomTab === 'logic') {
      this.renderLogicTab(contentEl);
    } else if (this.bottomTab === 'peripherals') {
      this.renderPeripheralsTab(contentEl);
    }
  }

  private renderBuildOutput(container: HTMLElement): void {
    const pre = document.createElement('div');
    pre.className = 'uv-output-pre';

    if (this.lastBuildDiags.length > 0) {
      const diagBox = document.createElement('div');
      diagBox.style.marginBottom = '6px';
      for (const d of this.lastBuildDiags) {
        const row = document.createElement('div');
        if (d.file && d.line > 0) {
          const link = document.createElement('span');
          link.className = 'uv-diag-link';
          link.textContent = `${d.file}(${d.line})`;
          link.addEventListener('click', () => void this.jumpToFileLine(d.file, d.line));
          const rest = document.createElement('span');
          rest.className = d.severity === 'error' ? 'uv-diag-err' : '';
          rest.textContent = `: ${d.severity} ${d.code}: ${d.message}`;
          row.append(link, rest);
        } else {
          row.className = d.severity === 'error' ? 'uv-diag-err' : '';
          row.textContent = `${d.severity} ${d.code}: ${d.message}`;
        }
        diagBox.appendChild(row);
      }
      pre.appendChild(diagBox);
    }

    const raw = document.createElement('div');
    raw.textContent = this.buildLog;
    pre.appendChild(raw);
    container.innerHTML = '';
    container.appendChild(pre);
    container.scrollTop = container.scrollHeight;
  }

  private renderCommandTab(container: HTMLElement): void {
    container.innerHTML = `<div class="uv-cmd-wrap">
      <div class="uv-cmd-log" id="uv-cmd-log">${escHtml(this.cmdLog)}</div>
      <div class="uv-cmd-input-row">
        <span>&gt;</span>
        <input id="uv-cmd-input" type="text" placeholder="Enter Keil debug command (e.g. r, t, g, d 0x20000000, bs 0x08000010, periph)..." />
      </div>
    </div>`;
    const logEl = document.getElementById('uv-cmd-log')!;
    logEl.scrollTop = logEl.scrollHeight;
    const inp = document.getElementById('uv-cmd-input') as HTMLInputElement;
    inp.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && inp.value.trim()) {
        const cmd = inp.value.trim();
        inp.value = '';
        void this.runCmdInput(cmd);
      }
    });
  }

  private renderMemoryTab(container: HTMLElement, idx: number): void {
    const view = this.memViews[idx];
    const changedSet = new Set(view.changed_offsets);
    container.innerHTML = `<div class="uv-mem-wrap">
      <div class="uv-mem-bar">
        <span>Address:</span>
        <input id="uv-mem-addr-${idx}" type="text" value="${escHtml(this.memAddrs[idx])}" />
        <button class="uv-btn" id="uv-mem-go-${idx}">Go</button>
        <span style="color:#666;font-size:10px;">(Click any byte to edit in place; blue = changed)</span>
      </div>
      <div class="uv-mem-grid" id="uv-mem-grid-${idx}"></div>
    </div>`;

    const addrInput = document.getElementById(`uv-mem-addr-${idx}`) as HTMLInputElement;
    const triggerRead = () => {
      this.memAddrs[idx] = addrInput.value.trim() || '0x20000000';
      void this.refreshMemoryIdx(idx);
    };
    addrInput.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') triggerRead();
    });
    document.getElementById(`uv-mem-go-${idx}`)!.addEventListener('click', triggerRead);

    const grid = document.getElementById(`uv-mem-grid-${idx}`)!;
    if (!view.bytes.length) {
      grid.textContent = 'No data (start Debug mode or click Go).';
      return;
    }

    for (let rowOff = 0; rowOff < view.bytes.length; rowOff += 16) {
      const rowAddr = (view.addr + rowOff) >>> 0;
      const rowEl = document.createElement('div');
      rowEl.className = 'uv-mem-row';

      const addrSpan = document.createElement('span');
      addrSpan.className = 'uv-mem-addr';
      addrSpan.textContent = `${hex32(rowAddr)}:`;
      rowEl.appendChild(addrSpan);

      let ascii = '';
      for (let c = 0; c < 16; c++) {
        const off = rowOff + c;
        if (off < view.bytes.length) {
          const b = view.bytes[off];
          const bSpan = document.createElement('span');
          bSpan.className = `uv-mem-byte${changedSet.has(off) ? ' changed' : ''}`;
          bSpan.textContent = b.toString(16).toUpperCase().padStart(2, '0');
          bSpan.title = `Click to edit 0x${(rowAddr + c).toString(16).toUpperCase()}`;
          bSpan.addEventListener('click', () => void this.promptEditMemoryByte((rowAddr + c) >>> 0, b, idx));
          rowEl.appendChild(bSpan);
          ascii += b >= 0x20 && b <= 0x7e ? String.fromCharCode(b) : '.';
        }
      }
      const ascSpan = document.createElement('span');
      ascSpan.className = 'uv-mem-ascii';
      ascSpan.textContent = `|${ascii}|`;
      rowEl.appendChild(ascSpan);
      grid.appendChild(rowEl);
    }
  }

  private renderWatchTab(container: HTMLElement): void {
    container.innerHTML = `<div class="uv-mem-wrap">
      <div class="uv-mem-bar">
        <span>Add Watch:</span>
        <input id="uv-watch-input" type="text" placeholder="R0, *0x20000000, GPIOA.ODR..." />
        <button class="uv-btn" id="uv-watch-add">Add</button>
      </div>
      <div style="flex:1;overflow:auto;padding:4px;">
        <table class="uv-watch-table">
          <thead><tr><th>Expression</th><th>Hex Value</th><th>Decimal / Info</th><th>Type</th><th></th></tr></thead>
          <tbody id="uv-watch-body"></tbody>
        </table>
      </div>
    </div>`;

    const inp = document.getElementById('uv-watch-input') as HTMLInputElement;
    const addFn = () => {
      const ex = inp.value.trim();
      if (ex && !this.watchExprs.includes(ex)) {
        this.watchExprs.push(ex);
        inp.value = '';
        void this.refreshWatch();
      }
    };
    document.getElementById('uv-watch-add')!.addEventListener('click', addFn);
    inp.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') addFn();
    });

    const tbody = document.getElementById('uv-watch-body')!;
    for (const ex of this.watchExprs) {
      const v = this.watchVals[ex];
      const tr = document.createElement('tr');
      tr.innerHTML = `<td>${escHtml(ex)}</td><td>${escHtml(v?.value_hex || '—')}</td><td>${escHtml(v?.value_dec || '')}</td><td>${escHtml(v?.kind || '')}</td>`;
      const tdDel = document.createElement('td');
      const btn = document.createElement('button');
      btn.className = 'uv-mini-btn';
      btn.textContent = '×';
      btn.addEventListener('click', () => {
        this.watchExprs = this.watchExprs.filter((x) => x !== ex);
        this.renderBottomPane();
      });
      tdDel.appendChild(btn);
      tr.appendChild(tdDel);
      tbody.appendChild(tr);
    }
  }

  private renderCallStackTab(container: HTMLElement): void {
    const frames = this.debugState?.call_stack || [];
    const rows = frames
      .map(
        (f) => `<tr>
          <td>#${f.index}</td>
          <td>${escHtml(f.symbol)}</td>
          <td>${hex32(f.addr)}</td>
          <td>${f.file ? `${escHtml(baseName(f.file))}:${f.line ?? ''}` : ''}</td>
        </tr>`
      )
      .join('');
    container.innerHTML = `<div style="padding:4px;">
      <table class="uv-watch-table">
        <thead><tr><th>Frame</th><th>Symbol</th><th>Address</th><th>Location</th></tr></thead>
        <tbody>${rows || '<tr><td colspan="4">No active call stack.</td></tr>'}</tbody>
      </table>
    </div>`;
  }

  private renderUartTab(container: HTMLElement): void {
    const txt = this.debugState?.uart1_output || '';
    container.innerHTML = `<div class="uv-cmd-wrap">
      <div class="uv-cmd-log" style="background:#ffffff;color:#000000;">${escHtml(txt || '(USART1 TX output will appear here)')}</div>
      <div class="uv-cmd-input-row">
        <span>RX&gt;</span>
        <input id="uv-uart-input" type="text" placeholder="Type bytes to send to USART1 RX (Enter to send)..." />
      </div>
    </div>`;
    const inp = document.getElementById('uv-uart-input') as HTMLInputElement;
    inp.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && inp.value.length > 0 && this.mode === 'debug') {
        const text = inp.value + '\n';
        inp.value = '';
        void this.rpc('debug.uartSend', { text }).then(() => this.statusText = 'Sent to USART1 RX');
      }
    });
  }

  private renderLogicTab(container: HTMLElement): void {
    container.innerHTML = `<div class="uv-logic-wrap">
      <div class="uv-mem-bar">
        <span>Signal:</span>
        <input id="uv-logic-sig" type="text" placeholder="R0, GPIOA.ODR, GPIOA.0, TIM2.CNT..." />
        <button class="uv-btn" id="uv-logic-add">Setup / Add</button>
        <button class="uv-btn" id="uv-logic-zin">Zoom +</button>
        <button class="uv-btn" id="uv-logic-zout">Zoom -</button>
        <button class="uv-btn" id="uv-logic-fit">Fit</button>
        <span id="uv-logic-hover" style="margin-left:8px;font-family:Consolas,monospace;">${escHtml(this.logicHoverInfo)}</span>
      </div>
      <div style="flex:1;min-height:0;position:relative;">
        <canvas id="uv-logic-canvas" class="uv-logic-canvas"></canvas>
      </div>
    </div>`;

    const canvas = document.getElementById('uv-logic-canvas') as HTMLCanvasElement;
    this.logicCanvas = new LogicAnalyzerCanvas(canvas, (info) => {
      this.logicHoverInfo = info;
      const el = document.getElementById('uv-logic-hover');
      if (el) el.textContent = info;
    });
    this.logicCanvas.setChannels(this.logicChannels, this.debugState?.registers.cycles ?? 100);

    document.getElementById('uv-logic-zin')!.addEventListener('click', () => this.logicCanvas?.zoomIn());
    document.getElementById('uv-logic-zout')!.addEventListener('click', () => this.logicCanvas?.zoomOut());
    document.getElementById('uv-logic-fit')!.addEventListener('click', () => this.logicCanvas?.zoomFit());
    document.getElementById('uv-logic-add')!.addEventListener('click', () => {
      const inp = document.getElementById('uv-logic-sig') as HTMLInputElement;
      const sig = inp.value.trim();
      if (sig && this.mode === 'debug') {
        inp.value = '';
        void this.rpc<LogicChannel[]>('debug.logicAdd', { signal: sig }).then((ch) => {
          this.logicChannels = ch;
          this.logicCanvas?.setChannels(ch, this.debugState?.registers.cycles ?? 100);
        });
      }
    });
  }

  private renderPeripheralsTab(container: HTMLElement): void {
    if (!this.peripherals) {
      container.innerHTML = '<div style="padding:10px;color:#666;">Start Debug in Simulator mode (Ctrl+F5) to view &amp; toggle live STM32F1 peripherals.</div>';
      return;
    }

    const items = ['RCC', 'GPIOA', 'GPIOB', 'GPIOC', 'GPIOD', 'TIM1', 'TIM2', 'TIM3', 'TIM4', 'USART1', 'SysTick'];
    container.innerHTML = `<div class="uv-periph-wrap">
      <div class="uv-periph-nav" id="uv-p-nav"></div>
      <div class="uv-periph-body" id="uv-p-body"></div>
    </div>`;

    const nav = document.getElementById('uv-p-nav')!;
    for (const it of items) {
      const div = document.createElement('div');
      div.className = `uv-periph-item${this.activePeriphTab === it ? ' active' : ''}`;
      div.textContent = it;
      div.addEventListener('click', () => {
        this.activePeriphTab = it;
        this.renderPeripheralsTab(container);
      });
      nav.appendChild(div);
    }

    const body = document.getElementById('uv-p-body')!;
    const p = this.peripherals;

    if (this.activePeriphTab === 'RCC') {
      body.innerHTML = `<b>Reset &amp; Clock Control (RCC)</b><br/><br/>
        CR: ${hex32(p.rcc.cr)} &nbsp; CFGR: ${hex32(p.rcc.cfgr)}<br/>
        APB2ENR: ${hex32(p.rcc.apb2enr)} &nbsp; APB1ENR: ${hex32(p.rcc.apb1enr)} &nbsp; AHBENR: ${hex32(p.rcc.ahbenr)}`;
    } else if (this.activePeriphTab.startsWith('GPIO')) {
      const gpio = p.gpios.find((g: any) => g.name === this.activePeriphTab);
      if (gpio) {
        body.innerHTML = `<b>${gpio.name}</b> &nbsp; CRL=${hex32(gpio.crl)} &nbsp; CRH=${hex32(gpio.crh)} &nbsp; IDR=${hex16(gpio.idr)} &nbsp; ODR=${hex16(gpio.odr)}
          <div style="margin-top:6px;font-family:Segoe UI,sans-serif;font-size:11px;">Pins (LED = ODR output bit, Checkbox = external IDR input stimulus):</div>
          <div class="uv-pin-grid" id="uv-pin-grid"></div>`;
        const grid = document.getElementById('uv-pin-grid')!;
        for (let pin = 0; pin < 16; pin++) {
          const odrOn = ((gpio.odr >> pin) & 1) !== 0;
          const idrOn = ((gpio.idr >> pin) & 1) !== 0;
          const box = document.createElement('label');
          box.className = 'uv-pin-box';
          box.innerHTML = `<span>P${gpio.name.slice(4)}${pin}</span>
            <span class="uv-pin-led${odrOn ? ' on' : ''}" title="ODR=${odrOn ? 1 : 0}"></span>
            <input type="checkbox" ${idrOn ? 'checked' : ''} title="Drive IDR pin ${pin}" />`;
          const cb = box.querySelector('input')!;
          cb.addEventListener('change', () => {
            void this.rpc('debug.setGpioPin', { port: gpio.name, pin, high: cb.checked }).then((snap) => {
              this.peripherals = snap;
              this.renderPeripheralsTab(container);
            });
          });
          grid.appendChild(box);
        }
      }
    } else if (this.activePeriphTab.startsWith('TIM')) {
      const tim = p.timers.find((t: any) => t.name === this.activePeriphTab);
      if (tim) {
        body.innerHTML = `<b>${tim.name}</b><br/><br/>
          CR1: ${hex16(tim.cr1)} &nbsp; DIER: ${hex16(tim.dier)} &nbsp; SR: ${hex16(tim.sr)}<br/>
          CNT: ${tim.cnt} (${hex16(tim.cnt)}) &nbsp; PSC: ${tim.psc} &nbsp; ARR: ${tim.arr}`;
      }
    } else if (this.activePeriphTab.startsWith('USART')) {
      const u = p.usarts.find((x: any) => x.name === this.activePeriphTab);
      if (u) {
        body.innerHTML = `<b>${u.name}</b><br/><br/>
          SR: ${hex16(u.sr)} &nbsp; BRR: ${hex16(u.brr)} &nbsp; CR1: ${hex16(u.cr1)}`;
      }
    } else if (this.activePeriphTab === 'SysTick') {
      body.innerHTML = `<b>SysTick</b><br/><br/>
        CTRL: ${hex32(p.systick.ctrl)} &nbsp; LOAD: ${hex32(p.systick.load)} &nbsp; VAL: ${hex32(p.systick.val)}`;
    }
  }

  private renderStatusBar(): void {
    const sb = document.getElementById('uv-statusbar')!;
    const st = this.debugState;
    const statesCell = st ? `States: ${st.registers.cycles}` : 'States: 0';
    const secCell = st ? `Sec: ${st.registers.time_sec.toFixed(6)}` : 'Sec: 0.000000';
    if (this.bridge.isWeb) {
      const wasmCell = this.pwaOfflineReady ? 'WASM • Offline Ready' : 'WASM Engine';
      sb.innerHTML = `
        <div class="uv-sb-cell uv-sb-grow">${escHtml(this.statusText)}</div>
        <div class="uv-sb-cell">${escHtml(this.project?.options.device || 'STM32F100RB')}</div>
        <div class="uv-sb-cell">Ln ${this.cursorLine}, Col ${this.cursorCol}</div>
        <div class="uv-sb-cell">${statesCell}</div>
        <div class="uv-sb-cell">${secCell}</div>
        <div class="uv-sb-cell">${wasmCell}</div>
        ${
          this.pwaCanInstall
            ? '<div class="uv-sb-cell"><button class="uv-mini-btn" id="uv-sb-install-pwa" title="Install Offline Desktop/Mobile App">Install App</button></div>'
            : ''
        }
      `;
      document.getElementById('uv-sb-install-pwa')?.addEventListener('click', () => {
        void this.host('installPwa');
      });
    } else {
      sb.innerHTML = `
        <div class="uv-sb-cell uv-sb-grow">${escHtml(this.statusText)}</div>
        <div class="uv-sb-cell">${escHtml(this.project?.options.device || 'STM32F100RB')}</div>
        <div class="uv-sb-cell">Ln ${this.cursorLine}, Col ${this.cursorCol}</div>
        <div class="uv-sb-cell">${statesCell}</div>
        <div class="uv-sb-cell">${secCell}</div>
        <div class="uv-sb-cell">CAP</div>
        <div class="uv-sb-cell">NUM</div>
      `;
    }
  }

  // ---- Actions ----

  private async openProjectOrFile(targetPath: string): Promise<void> {
    try {
      this.statusText = `Opening ${baseName(targetPath)}...`;
      this.renderStatusBar();
      const proj = await this.rpc<UvProject>('project.open', { path: targetPath });
      this.project = proj;

      const lower = targetPath.toLowerCase();
      if (/\.(s|asm|c|h)$/i.test(lower)) {
        await this.openFileInEditor(targetPath);
      } else {
        const first = proj.groups.flatMap((g) => g.files).find((f) => f.file_type === 1 || f.file_type === 2);
        if (first) {
          await this.openFileInEditor(first.abs_path);
        }
      }
      this.statusText = `Opened ${baseName(targetPath)}`;
      this.renderAll();
    } catch (e: any) {
      this.statusText = `Open failed: ${e.message}`;
      this.renderStatusBar();
    }
  }

  private async pickAndOpen(): Promise<void> {
    const p = await this.host<string | null>('pickOpenProjectOrFile');
    if (p) await this.openProjectOrFile(p);
  }

  private async openFileInEditor(filePath: string): Promise<void> {
    const existing = this.openDocs.find((d) => normPath(d.path) === normPath(filePath));
    if (existing) {
      this.activateDoc(existing.path);
      return;
    }
    const res = await this.host<{ path: string; content: string }>('readFile', { path: filePath });
    const doc: OpenDoc = {
      path: res.path,
      name: baseName(res.path),
      content: res.content,
      dirty: false,
      isC: /\.(c|h)$/i.test(res.path),
    };
    this.openDocs.push(doc);
    this.activateDoc(doc.path);
  }

  private activateDoc(filePath: string): void {
    const doc = this.openDocs.find((d) => normPath(d.path) === normPath(filePath));
    if (!doc) return;
    this.activeDocPath = doc.path;
    this.editor.setDocument(doc.content, doc.isC);
    this.syncEditorDebugOverlay();
    this.renderDocTabs();
    this.renderLeftPane();
  }

  private closeDoc(filePath: string): void {
    this.openDocs = this.openDocs.filter((d) => normPath(d.path) !== normPath(filePath));
    if (this.activeDocPath && normPath(this.activeDocPath) === normPath(filePath)) {
      const next = this.openDocs[0];
      this.activeDocPath = next ? next.path : null;
      if (next) this.editor.setDocument(next.content, next.isC);
      else this.editor.setDocument('', false);
    }
    this.renderDocTabs();
  }

  private getActiveDoc(): OpenDoc | undefined {
    const active = this.activeDocPath;
    if (!active) return undefined;
    return this.openDocs.find((d) => normPath(d.path) === normPath(active));
  }

  private async saveCurrentDoc(): Promise<void> {
    const doc = this.getActiveDoc();
    if (!doc) return;
    await this.host('writeFile', { path: doc.path, content: doc.content });
    doc.dirty = false;
    this.statusText = `Saved ${doc.name}`;
    this.renderDocTabs();
    this.renderStatusBar();
  }

  private async saveAllDocs(): Promise<void> {
    for (const doc of this.openDocs) {
      if (doc.dirty) {
        await this.host('writeFile', { path: doc.path, content: doc.content });
        doc.dirty = false;
      }
    }
    this.renderDocTabs();
  }

  private async runBuild(rebuild: boolean): Promise<boolean> {
    await this.saveAllDocs();
    const targetPath = this.project?.project_file || this.activeDocPath;
    if (!targetPath) {
      this.statusText = 'No project or file open to build.';
      this.renderStatusBar();
      return false;
    }
    this.statusText = rebuild ? 'Rebuilding target...' : 'Building target...';
    this.renderStatusBar();
    try {
      const res = await this.rpc<BuildResult>('build.run', { path: targetPath, rebuild });
      this.buildLog = res.log;
      this.lastBuildDiags = res.diagnostics;
      this.bottomTab = 'build';
      this.statusText = res.success
        ? `Build succeeded: 0 Error(s), ${res.warnings} Warning(s) [${res.elapsed_ms} ms]`
        : `Build failed: ${res.errors} Error(s), ${res.warnings} Warning(s)`;
      this.renderBottomPane();
      this.renderStatusBar();
      return res.success;
    } catch (e: any) {
      this.buildLog = `Build error: ${e.message}`;
      this.bottomTab = 'build';
      this.renderBottomPane();
      return false;
    }
  }

  private async toggleDebug(): Promise<void> {
    if (this.mode === 'debug') {
      await this.rpc('debug.stop');
      this.mode = 'edit';
      this.debugState = null;
      this.leftTab = 'project';
      this.bottomTab = 'build';
      this.statusText = 'Debug session stopped.';
      this.syncEditorDebugOverlay();
      this.renderAll();
      return;
    }

    const ok = await this.runBuild(false);
    if (!ok) return;

    const initialBps: { file: string; line: number }[] = [];
    this.savedLineBreakpoints.forEach((lines, file) => {
      lines.forEach((line) => initialBps.push({ file, line }));
    });

    try {
      this.statusText = 'Starting Debug session...';
      this.renderStatusBar();
      const res = await this.rpc<{
        state: DebugState;
        disassembly: DisasmLine[];
        executable_lines: Record<string, number[]>;
      }>('debug.start', {
        path: this.project?.project_file || this.activeDocPath,
        mode: this.project?.options.debug_mode || 'simulator',
        interface: this.project?.options.hw_interface || 'stlink',
        target: this.project?.options.hw_target || 'stm32f1x',
        clock_hz: this.project?.options.xtal_hz || 8000000,
        breakpoints: initialBps,
      });

      this.mode = 'debug';
      this.leftTab = 'registers';
      this.bottomTab = 'command';
      this.disasm = res.disassembly;
      this.execLinesByFile = res.executable_lines;
      await this.applyDebugState(res.state);
      this.renderAll();
    } catch (e: any) {
      this.buildLog += `\nDebug start error: ${e.message}\n`;
      this.bottomTab = 'build';
      this.statusText = `Debug failed: ${e.message}`;
      this.renderBottomPane();
      this.renderStatusBar();
    }
  }

  private async debugAction(method: string): Promise<void> {
    if (this.mode !== 'debug') return;
    try {
      const st = await this.rpc<DebugState>(method);
      await this.applyDebugState(st);
      this.renderLeftPane();
      this.renderDisasm();
      this.renderBottomPane();
      this.renderStatusBar();
    } catch (e: any) {
      this.statusText = `Debug error: ${e.message}`;
      this.renderStatusBar();
    }
  }

  private async runToCursor(): Promise<void> {
    if (this.mode !== 'debug' || !this.activeDocPath) return;
    const line = this.editor.getCurrentLine();
    try {
      const st = await this.rpc<DebugState>('debug.runTo', { file: this.activeDocPath, line });
      await this.applyDebugState(st);
      this.renderLeftPane();
      this.renderDisasm();
      this.renderBottomPane();
      this.renderStatusBar();
    } catch (e: any) {
      this.statusText = `Run to cursor: ${e.message}`;
      this.renderStatusBar();
    }
  }

  private async applyDebugState(st: DebugState): Promise<void> {
    this.debugState = st;
    this.statusText = `${st.halt_reason} | PC=${hex32(st.pc)} | Cycles=${st.registers.cycles}`;

    if (st.current_file && (!this.activeDocPath || normPath(this.activeDocPath) !== normPath(st.current_file))) {
      try {
        await this.openFileInEditor(st.current_file);
      } catch {
        // ignore if source path outside workspace
      }
    }
    this.syncEditorDebugOverlay();

    // Refresh active debug views cleanly
    if (this.mode === 'debug') {
      await Promise.all([
        this.refreshMemoryIdx(0, true),
        this.refreshWatch(true),
        this.rpc('debug.peripherals').then((p) => { this.peripherals = p; }).catch(() => {}),
        this.rpc<LogicChannel[]>('debug.logicSamples').then((c) => { this.logicChannels = c; }).catch(() => {}),
      ]);
    }
  }

  private syncEditorDebugOverlay(): void {
    const doc = this.getActiveDoc();
    if (!doc) return;
    const docNorm = normPath(doc.path);

    let pcLine: number | null = null;
    if (this.mode === 'debug' && this.debugState?.current_file && normPath(this.debugState.current_file) === docNorm) {
      pcLine = this.debugState.current_line;
    }

    const bpLines = new Set<number>(this.savedLineBreakpoints.get(docNorm) || []);
    if (this.mode === 'debug' && this.debugState) {
      for (const b of this.debugState.breakpoints) {
        if (b.file && normPath(b.file) === docNorm && b.line) {
          bpLines.add(b.line);
        }
      }
    }

    const execLines = new Set<number>();
    if (this.mode === 'debug') {
      for (const [f, lines] of Object.entries(this.execLinesByFile)) {
        if (normPath(f) === docNorm || baseName(f).toLowerCase() === doc.name.toLowerCase()) {
          lines.forEach((l) => execLines.add(l));
        }
      }
    }

    this.editor.setDebugState({ pcLine, breakpoints: bpLines, execLines });
  }

  private async toggleBreakpointAtLine(line: number): Promise<void> {
    const doc = this.getActiveDoc();
    if (!doc) return;
    const key = normPath(doc.path);
    const set = this.savedLineBreakpoints.get(key) || new Set<number>();
    if (set.has(line)) set.delete(line);
    else set.add(line);
    this.savedLineBreakpoints.set(key, set);

    if (this.mode === 'debug') {
      try {
        const res = await this.rpc<{ breakpoints: any[] }>('debug.setBreakpoint', {
          file: doc.path,
          line,
          enabled: set.has(line),
        });
        if (this.debugState) this.debugState.breakpoints = res.breakpoints;
        this.renderDisasm();
      } catch (e: any) {
        this.statusText = `Breakpoint: ${e.message}`;
        this.renderStatusBar();
      }
    }
    this.syncEditorDebugOverlay();
  }

  private async toggleBreakpointByAddr(addr: number): Promise<void> {
    if (this.mode !== 'debug') return;
    const res = await this.rpc<{ breakpoints: any[] }>('debug.setBreakpoint', { addr });
    if (this.debugState) this.debugState.breakpoints = res.breakpoints;
    this.renderDisasm();
    this.syncEditorDebugOverlay();
  }

  private async clearAllBreakpoints(): Promise<void> {
    this.savedLineBreakpoints.clear();
    if (this.mode === 'debug') {
      await this.rpc('debug.clearBreakpoints');
      if (this.debugState) this.debugState.breakpoints = [];
      this.renderDisasm();
    }
    this.syncEditorDebugOverlay();
  }

  private async refreshMemoryIdx(idx: number, quiet = false): Promise<void> {
    if (this.mode !== 'debug') return;
    try {
      const res = await this.rpc<{ addr: number; bytes: number[]; changed_offsets: number[] }>('debug.memoryRead', {
        addr: this.memAddrs[idx],
        len: 256,
      });
      this.memViews[idx] = res;
      if (!quiet && this.bottomTab === `mem${idx + 1}`) {
        this.renderBottomPane();
      }
    } catch (e: any) {
      if (!quiet) {
        this.statusText = `Memory read failed: ${e.message}`;
        this.renderStatusBar();
      }
    }
  }

  private async refreshWatch(quiet = false): Promise<void> {
    if (this.mode !== 'debug') return;
    for (const ex of this.watchExprs) {
      try {
        const r = await this.rpc<{ value_hex: string; value_dec: string; kind: string }>('debug.evalWatch', { expr: ex });
        this.watchVals[ex] = r;
      } catch {
        this.watchVals[ex] = { value_hex: '<unavailable>', value_dec: '', kind: '' };
      }
    }
    if (!quiet && this.bottomTab === 'watch') {
      this.renderBottomPane();
    }
  }

  private async runCmdInput(cmd: string): Promise<void> {
    if (this.mode !== 'debug') return;
    this.cmdLog += `> ${cmd}\n`;
    try {
      const res = await this.rpc<{ output: string; state: DebugState }>('debug.command', { command: cmd });
      if (res.output) this.cmdLog += `${res.output}\n`;
      await this.applyDebugState(res.state);
      this.renderLeftPane();
      this.renderDisasm();
    } catch (e: any) {
      this.cmdLog += `Error: ${e.message}\n`;
    }
    this.renderBottomPane();
  }

  private async promptEditRegister(name: string, current: string): Promise<void> {
    const val = await this.host<string | undefined>('promptText', {
      prompt: `Set register ${name} (hex e.g. 0x10 or decimal)`,
      value: current,
    });
    if (val !== undefined && val.trim() !== '') {
      const st = await this.rpc<DebugState>('debug.setRegister', { name, value: val.trim() });
      await this.applyDebugState(st);
      this.renderLeftPane();
      this.renderDisasm();
      this.renderBottomPane();
    }
  }

  private async promptEditMemoryByte(addr: number, cur: number, memIdx: number): Promise<void> {
    const val = await this.host<string | undefined>('promptText', {
      prompt: `Write byte at ${hex32(addr)} (hex e.g. 0xFF)`,
      value: `0x${cur.toString(16).toUpperCase().padStart(2, '0')}`,
    });
    if (val !== undefined && val.trim() !== '') {
      const parsed = parseInt(val.trim(), val.trim().toLowerCase().startsWith('0x') ? 16 : 10);
      if (!Number.isNaN(parsed)) {
        await this.rpc('debug.memoryWrite', { addr, bytes: [parsed & 0xff] });
        await this.refreshMemoryIdx(memIdx);
      }
    }
  }

  private async jumpToFileLine(file: string, line: number): Promise<void> {
    await this.openFileInEditor(file);
    this.editor.scrollToLine(line);
  }

  private async downloadBuildArtifacts(): Promise<void> {
    const ok = await this.runBuild(false);
    if (!ok) return;
    await this.host('downloadFirmware', {
      outputName: this.project?.options.output_name || 'firmware',
    });
  }

  private async flashDevice(): Promise<void> {
    const ok = await this.runBuild(false);
    if (!ok) return;
    if (this.bridge.isWeb) {
      this.statusText = 'Opening WebSerial STM32 Flasher...';
      this.renderStatusBar();
      try {
        await this.host('flashWebHardware', {
          outputName: this.project?.options.output_name || 'firmware',
          hwTarget: this.project?.options.hw_target || 'stm32f1x',
        });
        this.statusText = 'Ready';
        this.renderStatusBar();
      } catch (e: any) {
        this.buildLog += `\nFlash Error: ${e.message}\n`;
        this.statusText = `Flash failed: ${e.message}`;
        this.renderBottomPane();
        this.renderStatusBar();
      }
      return;
    }
    this.statusText = 'Flashing STM32 via OpenOCD...';
    this.renderStatusBar();
    try {
      const res = await this.rpc<{ ok: boolean; log: string }>('flash.program', {
        path: this.project?.project_file || this.activeDocPath,
        interface: this.project?.options.hw_interface || 'stlink',
        target: this.project?.options.hw_target || 'stm32f1x',
      });
      this.buildLog += `\n--- Flash Output ---\n${res.log}`;
      this.statusText = 'Flash & Verify completed.';
      this.renderBottomPane();
      this.renderStatusBar();
    } catch (e: any) {
      this.buildLog += `\nFlash Error: ${e.message}\n`;
      this.statusText = `Flash failed: ${e.message}`;
      this.renderBottomPane();
      this.renderStatusBar();
    }
  }

  private async addExistingFile(): Promise<void> {
    if (!this.project) return;
    const p = await this.host<string | null>('pickExistingSourceFile', { rootDir: this.project.root_dir });
    if (!p) return;
    const res = await this.rpc<{ project: UvProject }>('project.addFile', {
      project_path: this.project.project_file,
      file_path: p,
    });
    this.project = res.project;
    await this.openFileInEditor(p);
    this.renderLeftPane();
  }

  private async showToolchainInfo(): Promise<void> {
    const info = await this.rpc<any>('toolchain.info');
    this.buildLog = `Toolchain Info:\n${JSON.stringify(info, null, 2)}`;
    this.bottomTab = 'build';
    this.renderBottomPane();
  }

  // ---- Modals ----

  private openNewProjectDialog(): void {
    const host = document.getElementById('uv-modal-host')!;
    const defaultFolder = this.bridge.isWeb ? '/Workspace/LabProject' : '';
    host.innerHTML = `<div class="uv-modal-backdrop"><div class="uv-modal">
      <div class="uv-modal-head"><span>Create New µVision Project</span><button class="uv-mini-btn" id="uv-m-x">×</button></div>
      <div class="uv-modal-body">
        <div class="uv-form-row"><label>Folder:</label><div style="display:flex;gap:4px;"><input id="np-dir" style="flex:1" value="${escHtml(defaultFolder)}" placeholder="Select folder..." />${this.bridge.isWeb ? '' : '<button class="uv-btn" id="np-browse">Browse</button>'}</div></div>
        <div class="uv-form-row"><label>Project Name:</label><input id="np-name" value="LabProject" /></div>
        <div class="uv-form-row"><label>Device:</label><select id="np-dev"><option>STM32F100RB</option><option>STM32F103RB</option><option>STM32F103C8</option><option>ARMCM3</option></select></div>
        <div class="uv-form-row"><label>Template:</label><select id="np-tpl"><option value="asm">ARM Assembly (.s)</option><option value="c">STM32 C (.c)</option></select></div>
      </div>
      <div class="uv-modal-foot"><button class="uv-btn primary" id="np-ok">Create</button><button class="uv-btn" id="np-cancel">Cancel</button></div>
    </div></div>`;

    const close = () => (host.innerHTML = '');
    document.getElementById('uv-m-x')!.onclick = close;
    document.getElementById('np-cancel')!.onclick = close;
    const browseBtn = document.getElementById('np-browse');
    if (browseBtn) {
      browseBtn.onclick = async () => {
        const dir = await this.host<string | null>('pickNewProjectDir');
        if (dir) (document.getElementById('np-dir') as HTMLInputElement).value = dir;
      };
    } else if (this.bridge.isWeb) {
      const dirInp = document.getElementById('np-dir') as HTMLInputElement;
      const nameInp = document.getElementById('np-name') as HTMLInputElement;
      let dirtyDir = false;
      dirInp.addEventListener('input', () => { dirtyDir = true; });
      nameInp.addEventListener('input', () => {
        if (!dirtyDir) dirInp.value = `/Workspace/${nameInp.value.trim() || 'LabProject'}`;
      });
    }
    document.getElementById('np-ok')!.onclick = async () => {
      const dir = (document.getElementById('np-dir') as HTMLInputElement).value.trim();
      const name = (document.getElementById('np-name') as HTMLInputElement).value.trim() || 'LabProject';
      const device = (document.getElementById('np-dev') as HTMLSelectElement).value;
      const template = (document.getElementById('np-tpl') as HTMLSelectElement).value;
      if (!dir) return;
      const proj = await this.rpc<UvProject>('project.new', { dir, name, device, template });
      this.project = proj;
      close();
      const first = proj.groups[0]?.files[0];
      if (first) await this.openFileInEditor(first.abs_path);
      this.renderAll();
    };
  }

  private openAddItemDialog(): void {
    const host = document.getElementById('uv-modal-host')!;
    host.innerHTML = `<div class="uv-modal-backdrop"><div class="uv-modal">
      <div class="uv-modal-head"><span>Add New Item to Group 'Source Group 1'</span><button class="uv-mini-btn" id="uv-m-x">×</button></div>
      <div class="uv-modal-body">
        <div class="uv-form-row"><label>File Name:</label><input id="af-name" value="main.s" /></div>
        <div class="uv-form-row"><label>Type:</label><select id="af-type"><option value="asm">Asm File (.s) with Course Skeleton</option><option value="c">C File (.c)</option><option value="empty">Empty Header / File</option></select></div>
      </div>
      <div class="uv-modal-foot"><button class="uv-btn primary" id="af-ok">Add</button><button class="uv-btn" id="af-cancel">Cancel</button></div>
    </div></div>`;

    const close = () => (host.innerHTML = '');
    document.getElementById('uv-m-x')!.onclick = close;
    document.getElementById('af-cancel')!.onclick = close;
    document.getElementById('af-ok')!.onclick = async () => {
      const name = (document.getElementById('af-name') as HTMLInputElement).value.trim();
      const tpl = (document.getElementById('af-type') as HTMLSelectElement).value;
      if (!name) return;
      if (this.project) {
        const res = await this.rpc<{ added: ProjectFile; project: UvProject }>('project.addFile', {
          project_path: this.project.project_file,
          file_path: name,
          template: tpl,
        });
        this.project = res.project;
        close();
        await this.openFileInEditor(res.added.abs_path);
        this.renderLeftPane();
      }
    };
  }

  private openOptionsDialog(initialTab = 'target'): void {
    if (!this.project) return;
    const o = { ...this.project.options };
    const host = document.getElementById('uv-modal-host')!;
    let tab = initialTab;

    const render = () => {
      host.innerHTML = `<div class="uv-modal-backdrop"><div class="uv-modal">
        <div class="uv-modal-head"><span>Options for Target '${escHtml(o.target_name)}'</span><button class="uv-mini-btn" id="uv-m-x">×</button></div>
        <div class="uv-modal-tabs">
          ${['target', 'output', 'asm', 'c', 'debug'].map((t) => `<button class="uv-tab-btn${tab === t ? ' active' : ''}" data-tab="${t}">${t.toUpperCase()}</button>`).join('')}
        </div>
        <div class="uv-modal-body" id="opt-body"></div>
        <div class="uv-modal-foot"><button class="uv-btn primary" id="opt-ok">OK</button><button class="uv-btn" id="opt-cancel">Cancel</button></div>
      </div></div>`;

      host.querySelectorAll('[data-tab]').forEach((b) => {
        b.addEventListener('click', () => {
          captureFields();
          tab = (b as HTMLElement).dataset.tab!;
          render();
        });
      });

      const body = document.getElementById('opt-body')!;
      if (tab === 'target') {
        body.innerHTML = `
          <div class="uv-form-row"><label>Device:</label><input id="opt-device" value="${escHtml(o.device)}" /></div>
          <div class="uv-form-row"><label>Xtal (Hz):</label><input id="opt-xtal" value="${o.xtal_hz}" /></div>
          <div class="uv-form-row"><label>IROM1 Start:</label><input id="opt-ro" value="${hex32(o.ro_base)}" /></div>
          <div class="uv-form-row"><label>IRAM1 Start:</label><input id="opt-rw" value="${hex32(o.rw_base)}" /></div>
        `;
      } else if (tab === 'output') {
        body.innerHTML = `
          <div class="uv-form-row"><label>Name of Executable:</label><input id="opt-outname" value="${escHtml(o.output_name)}" /></div>
          <div class="uv-form-row"><label>Output Folder:</label><input id="opt-outdir" value="${escHtml(o.output_dir)}" /></div>
          <div class="uv-form-row"><label>Create HEX File:</label><input id="opt-hex" type="checkbox" ${o.create_hex ? 'checked' : ''} /></div>
          <div class="uv-form-row"><label>Create BIN File:</label><input id="opt-bin" type="checkbox" ${o.create_bin ? 'checked' : ''} /></div>
        `;
      } else if (tab === 'asm') {
        body.innerHTML = `
          <div class="uv-form-row"><label>Assembler Syntax:</label><select id="opt-asm">
            <option value="armasm" ${o.asm_syntax === 'armasm' ? 'selected' : ''}>armasm (Arm Syntax — AREA/DCD/THUMB)</option>
            <option value="gnuarmclang" ${o.asm_syntax === 'gnuarmclang' ? 'selected' : ''}>armclang (GNU Syntax)</option>
          </select></div>
          <div class="uv-form-row"><label>Misc Controls:</label><input id="opt-amisc" value="${escHtml(o.a_misc)}" /></div>
        `;
      } else if (tab === 'c') {
        body.innerHTML = `
          <div class="uv-form-row"><label>Define:</label><input id="opt-cdef" value="${escHtml(o.c_defines.join(', '))}" /></div>
          <div class="uv-form-row"><label>Include Paths:</label><input id="opt-cinc" value="${escHtml(o.c_includes.join('; '))}" /></div>
          <div class="uv-form-row"><label>Misc Controls:</label><input id="opt-cmisc" value="${escHtml(o.c_misc)}" /></div>
        `;
      } else if (tab === 'debug') {
        body.innerHTML = `
          <div class="uv-form-row"><label>Debug Backend:</label><select id="opt-dbg">
            <option value="simulator" ${o.debug_mode === 'simulator' ? 'selected' : ''}>Use Simulator (Fast Built-in Cortex-M3 + STM32F1)</option>
            <option value="hardware" ${o.debug_mode === 'hardware' ? 'selected' : ''}>Use Hardware ST-Link / OpenOCD (SWD)</option>
          </select></div>
          <div class="uv-form-row"><label>OpenOCD Interface:</label><input id="opt-iface" value="${escHtml(o.hw_interface)}" /></div>
          <div class="uv-form-row"><label>OpenOCD Target:</label><input id="opt-tgt" value="${escHtml(o.hw_target)}" /></div>
        `;
      }

      document.getElementById('uv-m-x')!.onclick = () => (host.innerHTML = '');
      document.getElementById('opt-cancel')!.onclick = () => (host.innerHTML = '');
      document.getElementById('opt-ok')!.onclick = async () => {
        captureFields();
        const saved = await this.rpc<UvProject>('project.saveOptions', { options: o });
        this.project = saved;
        host.innerHTML = '';
        this.renderToolbar();
        this.renderStatusBar();
      };
    };

    const captureFields = () => {
      const val = (id: string) => (document.getElementById(id) as HTMLInputElement | HTMLSelectElement | null)?.value;
      const chk = (id: string) => (document.getElementById(id) as HTMLInputElement | null)?.checked;
      if (val('opt-device') !== undefined) o.device = val('opt-device')!;
      if (val('opt-xtal') !== undefined) o.xtal_hz = parseInt(val('opt-xtal')!, 10) || 8000000;
      if (val('opt-ro') !== undefined) o.ro_base = parseInt(val('opt-ro')!, 16) || 0x08000000;
      if (val('opt-rw') !== undefined) o.rw_base = parseInt(val('opt-rw')!, 16) || 0x20000000;
      if (val('opt-outname') !== undefined) o.output_name = val('opt-outname')!;
      if (val('opt-outdir') !== undefined) o.output_dir = val('opt-outdir')!;
      if (chk('opt-hex') !== undefined) o.create_hex = chk('opt-hex')!;
      if (chk('opt-bin') !== undefined) o.create_bin = chk('opt-bin')!;
      if (val('opt-asm') !== undefined) o.asm_syntax = val('opt-asm') as any;
      if (val('opt-amisc') !== undefined) o.a_misc = val('opt-amisc')!;
      if (val('opt-cdef') !== undefined) o.c_defines = val('opt-cdef')!.split(',').map((s) => s.trim()).filter(Boolean);
      if (val('opt-cinc') !== undefined) o.c_includes = val('opt-cinc')!.split(';').map((s) => s.trim()).filter(Boolean);
      if (val('opt-cmisc') !== undefined) o.c_misc = val('opt-cmisc')!;
      if (val('opt-dbg') !== undefined) o.debug_mode = val('opt-dbg') as any;
      if (val('opt-iface') !== undefined) o.hw_interface = val('opt-iface')!;
      if (val('opt-tgt') !== undefined) o.hw_target = val('opt-tgt')!;
    };

    render();
  }

  private async openPackDialog(): Promise<void> {
    const packs = await this.rpc<any[]>('pack.list');
    const host = document.getElementById('uv-modal-host')!;
    const rows = packs.map((p) => `<tr><td>${escHtml(p.vendor)}</td><td>${escHtml(p.name)}</td><td>${escHtml(p.version)}</td></tr>`).join('');
    host.innerHTML = `<div class="uv-modal-backdrop"><div class="uv-modal">
      <div class="uv-modal-head"><span>Pack Installer</span><button class="uv-mini-btn" id="uv-m-x">×</button></div>
      <div class="uv-modal-body">
        <table class="uv-watch-table"><thead><tr><th>Vendor</th><th>Pack</th><th>Version</th></tr></thead><tbody>${rows}</tbody></table>
      </div>
      <div class="uv-modal-foot">
        <button class="uv-btn" id="pk-add">Install .pack File...</button>
        <button class="uv-btn primary" id="pk-close">Close</button>
      </div>
    </div></div>`;
    const close = () => (host.innerHTML = '');
    document.getElementById('uv-m-x')!.onclick = close;
    document.getElementById('pk-close')!.onclick = close;
    document.getElementById('pk-add')!.onclick = async () => {
      const f = await this.host<string | null>('pickPackFile');
      if (f) {
        await this.rpc('pack.add', { spec: f });
        await this.openPackDialog();
      }
    };
  }

  private showHelpModal(): void {
    const host = document.getElementById('uv-modal-host')!;
    host.innerHTML = `<div class="uv-modal-backdrop"><div class="uv-modal">
      <div class="uv-modal-head"><span>uVision Fast — Keyboard Shortcuts &amp; CLI</span><button class="uv-mini-btn" id="uv-m-x">×</button></div>
      <div class="uv-modal-body" style="font-size:11.5px;line-height:1.5;">
        <b>IDE Shortcuts:</b><br/>
        • <b>F7</b>: Build Target &nbsp; • <b>Ctrl+F5</b>: Start/Stop Debug Session<br/>
        • <b>F5</b>: Run &nbsp; • <b>F11</b>: Step Into &nbsp; • <b>F10</b>: Step Over &nbsp; • <b>Ctrl+F11</b>: Step Out<br/>
        • <b>F9</b>: Toggle Breakpoint &nbsp; • <b>Alt+F7</b>: Options for Target &nbsp; • <b>Ctrl+S</b>: Save<br/><br/>
        <b>CLI Commands (uvcli):</b><br/>
        <code>uvcli build &lt;file.s|.uvprojx&gt;</code><br/>
        <code>uvcli run &lt;file.s|.uvprojx&gt; --dump-addr 0x20000000</code><br/>
        <code>uvcli debug &lt;file.s|.uvprojx&gt; [--hw]</code><br/>
        <code>uvcli flash &lt;file.s|.uvprojx&gt;</code>
      </div>
      <div class="uv-modal-foot"><button class="uv-btn primary" id="hp-ok">OK</button></div>
    </div></div>`;
    document.getElementById('uv-m-x')!.onclick = () => (host.innerHTML = '');
    document.getElementById('hp-ok')!.onclick = () => (host.innerHTML = '');
  }
}

function hex32(n: number): string {
  return '0x' + (n >>> 0).toString(16).toUpperCase().padStart(8, '0');
}

function hex16(n: number): string {
  return '0x' + (n & 0xffff).toString(16).toUpperCase().padStart(4, '0');
}

function normPath(p: string): string {
  return p.replace(/\\/g, '/').toLowerCase();
}

function baseName(p: string): string {
  return p.replace(/\\/g, '/').split('/').pop() || p;
}

function escHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c] || c));
}

new UvisionApp();
