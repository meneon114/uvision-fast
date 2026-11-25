import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
import { UI_RPC_METHODS, VSCODE_HOST_ACTIONS } from '../../web/src/host/protocol';
import { UvCliClient } from './cliClient';

const RPC_ALLOWED = new Set<string>(UI_RPC_METHODS);
const HOST_ALLOWED = new Set<string>(VSCODE_HOST_ACTIONS);

// RPC params that name a file or folder on disk. Each one must resolve inside a trusted root.
const PATH_PARAMS = ['path', 'dir', 'project_path', 'file_path', 'file'] as const;

// The webview edits source files only. Project files change through project.* RPCs.
const WRITABLE_EXT = /\.(s|asm|a51|inc|c|h|txt|ini|sct)$/i;

const isWin = process.platform === 'win32';
const norm = (p: string) => (isWin ? p.toLowerCase() : p);

// Resolves symlinks in the longest existing prefix, so a link inside the
// workspace can't point the check at a file outside it.
function canonical(p: string): string {
  let cur = path.resolve(p);
  const rest: string[] = [];
  for (;;) {
    try {
      return path.join(fs.realpathSync.native(cur), ...rest.reverse());
    } catch {
      const parent = path.dirname(cur);
      if (parent === cur) return path.resolve(p);
      rest.push(path.basename(cur));
      cur = parent;
    }
  }
}

function isInside(root: string, p: string): boolean {
  const rel = path.relative(norm(root), norm(p));
  return rel === '' || (!rel.startsWith('..') && !path.isAbsolute(rel));
}

// Folders and files the webview may name: the workspace folders, plus what the
// user chose on the extension side (a command target, a native dialog pick) and
// the files a trusted project lists.
class PathGuard {
  private readonly roots = new Set<string>();
  private readonly files = new Set<string>();

  trustRoot(dir: string): void {
    this.roots.add(canonical(dir));
  }

  trustFile(file: string): void {
    this.files.add(norm(canonical(file)));
  }

  trustProject(result: unknown): void {
    const proj = result as { root_dir?: unknown; groups?: { files?: { abs_path?: unknown }[] }[] } | null;
    if (!proj || typeof proj !== 'object' || !Array.isArray(proj.groups)) return;
    for (const g of proj.groups) {
      for (const f of g.files ?? []) {
        if (typeof f.abs_path === 'string' && path.isAbsolute(f.abs_path)) this.trustFile(f.abs_path);
      }
    }
  }

  check(value: unknown, what: string): string {
    if (typeof value !== 'string' || !value.trim()) throw new Error(`${what}: expected a path`);
    if (!path.isAbsolute(value)) throw new Error(`${what}: path must be absolute`);
    const p = canonical(value);
    if (this.files.has(norm(p))) return p;
    const roots = [...this.roots, ...(vscode.workspace.workspaceFolders ?? []).map((f) => canonical(f.uri.fsPath))];
    if (roots.some((r) => isInside(r, p))) return p;
    throw new Error(`${what}: ${value} is outside the workspace and the open project`);
  }
}

export class UvisionIdePanel {
  public static currentPanel: UvisionIdePanel | undefined;

  private readonly panel: vscode.WebviewPanel;
  private readonly disposables: vscode.Disposable[] = [];
  private readonly recentlySaved = new Map<string, number>();
  private readonly guard = new PathGuard();

  public static createOrShow(context: vscode.ExtensionContext, cli: UvCliClient, initialTarget?: string): UvisionIdePanel {
    if (UvisionIdePanel.currentPanel) {
      UvisionIdePanel.currentPanel.panel.reveal(vscode.ViewColumn.One);
      if (initialTarget) {
        void UvisionIdePanel.currentPanel.openTargetPath(initialTarget);
      }
      return UvisionIdePanel.currentPanel;
    }

    const panel = vscode.window.createWebviewPanel('uvisionFast.ide', 'Keil µVision 5 - uVision Fast', vscode.ViewColumn.One, {
      enableScripts: true,
      retainContextWhenHidden: true,
      localResourceRoots: [vscode.Uri.joinPath(context.extensionUri, 'dist', 'webview')],
    });

    const instance = new UvisionIdePanel(panel, context.extensionUri, cli, initialTarget);
    UvisionIdePanel.currentPanel = instance;
    return instance;
  }

  private constructor(
    panel: vscode.WebviewPanel,
    private readonly extensionUri: vscode.Uri,
    private readonly cli: UvCliClient,
    private initialTarget?: string,
  ) {
    this.panel = panel;
    if (initialTarget) this.guard.trustRoot(path.dirname(initialTarget));
    this.panel.webview.html = this.getHtml(this.panel.webview);

    this.panel.onDidDispose(() => this.dispose(), null, this.disposables);

    this.panel.webview.onDidReceiveMessage(
      async (msg) => {
        await this.handleWebviewMessage(msg);
      },
      null,
      this.disposables,
    );

    const watcher = vscode.workspace.createFileSystemWatcher('**/*.{s,S,asm,c,h,uvprojx}');
    const notifyDiskChange = async (uri: vscode.Uri) => {
      const p = uri.fsPath;
      const ts = this.recentlySaved.get(p.toLowerCase());
      if (ts && Date.now() - ts < 1500) {
        return;
      }
      try {
        const content = fs.readFileSync(p, 'utf8');
        this.panel.webview.postMessage({ type: 'fileChangedOnDisk', path: p, content });
      } catch {
        // ignore deleted/unreadable
      }
    };
    watcher.onDidChange(notifyDiskChange, null, this.disposables);
    this.disposables.push(watcher);

    const onSave = vscode.workspace.onDidSaveTextDocument((doc) => {
      const p = doc.uri.fsPath;
      if (/\.(s|asm|c|h|uvprojx)$/i.test(p)) {
        const ts = this.recentlySaved.get(p.toLowerCase());
        if (!ts || Date.now() - ts >= 1500) {
          this.panel.webview.postMessage({
            type: 'fileChangedOnDisk',
            path: p,
            content: doc.getText(),
          });
        }
      }
    });
    this.disposables.push(onSave);
  }

  public async openTargetPath(targetPath: string): Promise<void> {
    this.guard.trustRoot(path.dirname(targetPath));
    this.panel.webview.postMessage({ type: 'openTarget', path: targetPath });
  }

  public sendCommand(cmd: 'build' | 'rebuild' | 'debug' | 'flash'): void {
    this.panel.webview.postMessage({ type: 'hostAction', action: cmd });
  }

  private async handleWebviewMessage(msg: unknown): Promise<void> {
    if (!msg || typeof msg !== 'object') {
      return;
    }
    const m = msg as { type?: unknown; reqId?: unknown; method?: unknown; action?: unknown; params?: unknown; payload?: unknown };

    if (m.type === 'ready') {
      const target = this.initialTarget || this.detectWorkspaceTarget();
      this.initialTarget = undefined;
      if (target) {
        await this.openTargetPath(target);
      }
      return;
    }

    if (typeof m.reqId !== 'number') return;
    const reqId = m.reqId;
    const params = m.params && typeof m.params === 'object' ? (m.params as Record<string, unknown>) : {};
    const payload = m.payload && typeof m.payload === 'object' ? (m.payload as Record<string, unknown>) : {};

    if (m.type === 'rpc') {
      try {
        const method = String(m.method);
        if (!RPC_ALLOWED.has(method)) throw new Error(`Method not allowed from the webview: ${method}`);
        this.checkRpcPaths(method, params);
        const result = await this.cli.request(method, params);
        if (method.startsWith('project.')) this.guard.trustProject(result);
        this.panel.webview.postMessage({ type: 'rpcResponse', reqId, result });
      } catch (err) {
        this.panel.webview.postMessage({ type: 'rpcResponse', reqId, error: errText(err) });
      }
      return;
    }

    if (m.type === 'host') {
      try {
        const action = String(m.action);
        if (!HOST_ALLOWED.has(action)) throw new Error(`Host action not allowed: ${action}`);
        const result = await this.handleHostAction(action, payload);
        this.panel.webview.postMessage({ type: 'hostResponse', reqId, result });
      } catch (err) {
        this.panel.webview.postMessage({ type: 'hostResponse', reqId, error: errText(err) });
      }
    }
  }

  private checkRpcPaths(method: string, params: Record<string, unknown>): void {
    for (const key of PATH_PARAMS) {
      if (params[key] !== undefined && params[key] !== null) this.guard.check(params[key], `${method} ${key}`);
    }
    if (Array.isArray(params.breakpoints)) {
      for (const bp of params.breakpoints as { file?: unknown }[]) {
        if (bp && typeof bp === 'object' && bp.file !== undefined) this.guard.check(bp.file, `${method} breakpoint`);
      }
    }
  }

  private async handleHostAction(action: string, payload: Record<string, unknown>): Promise<unknown> {
    switch (action) {
      case 'readFile': {
        const filePath = this.guard.check(payload.path, 'readFile');
        const content = fs.readFileSync(filePath, 'utf8');
        return { path: String(payload.path), content };
      }
      case 'writeFile': {
        const filePath = this.guard.check(payload.path, 'writeFile');
        if (!WRITABLE_EXT.test(filePath)) throw new Error(`writeFile: ${path.basename(filePath)} is not a source file`);
        const content = String(payload.content ?? '');
        fs.mkdirSync(path.dirname(filePath), { recursive: true });
        this.recentlySaved.set(filePath.toLowerCase(), Date.now());
        fs.writeFileSync(filePath, content, 'utf8');
        return { ok: true };
      }
      case 'pickOpenProjectOrFile': {
        const uris = await vscode.window.showOpenDialog({
          canSelectMany: false,
          filters: {
            'Keil & ARM Source': ['uvprojx', 's', 'asm', 'c', 'h'],
            'All Files': ['*'],
          },
          defaultUri: vscode.workspace.workspaceFolders?.[0]?.uri,
        });
        const picked = uris?.[0]?.fsPath;
        if (picked) this.guard.trustRoot(path.dirname(picked));
        return picked || null;
      }
      case 'pickNewProjectDir': {
        const uris = await vscode.window.showOpenDialog({
          canSelectFiles: false,
          canSelectFolders: true,
          canSelectMany: false,
          openLabel: 'Select Project Folder',
          defaultUri: vscode.workspace.workspaceFolders?.[0]?.uri,
        });
        const picked = uris?.[0]?.fsPath;
        if (picked) this.guard.trustRoot(picked);
        return picked || null;
      }
      case 'pickExistingSourceFile': {
        let defaultUri: vscode.Uri | undefined;
        try {
          defaultUri = vscode.Uri.file(this.guard.check(payload.rootDir, 'rootDir'));
        } catch {
          defaultUri = vscode.workspace.workspaceFolders?.[0]?.uri;
        }
        const uris = await vscode.window.showOpenDialog({
          canSelectMany: false,
          filters: {
            'ARM Assembly & C': ['s', 'asm', 'c', 'h'],
            'All Files': ['*'],
          },
          defaultUri,
        });
        const picked = uris?.[0]?.fsPath;
        if (picked) this.guard.trustFile(picked);
        return picked || null;
      }
      case 'installPack': {
        // The pack path comes from the native dialog, never from the webview.
        const uris = await vscode.window.showOpenDialog({
          canSelectMany: false,
          filters: { 'CMSIS Pack': ['pack'] },
          openLabel: 'Install Pack',
        });
        const picked = uris?.[0]?.fsPath;
        if (!picked) return null;
        return await vscode.window.withProgress(
          { location: vscode.ProgressLocation.Notification, title: `Installing ${path.basename(picked)}` },
          () => this.cli.request('pack.add', { spec: picked }),
        );
      }
      case 'promptText': {
        return await vscode.window.showInputBox({
          prompt: String(payload.prompt ?? 'Input'),
          value: String(payload.value ?? ''),
          placeHolder: String(payload.placeHolder ?? ''),
        });
      }
      default:
        throw new Error(`Unknown host action: ${action}`);
    }
  }

  private detectWorkspaceTarget(): string | undefined {
    const active = vscode.window.activeTextEditor?.document.uri.fsPath;
    if (active && /\.(uvprojx|s|asm|c)$/i.test(active)) {
      return active;
    }
    const ws = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
    if (!ws) {
      return undefined;
    }
    const sampleUv = path.join(ws, 'Lab Projects & Practice', 'Keil', 'E 1', 'Test.uvprojx');
    if (fs.existsSync(sampleUv)) {
      return sampleUv;
    }
    return undefined;
  }

  // The webview is the shared React app built by `vite build --mode vscode`.
  // Its Vite manifest names the hashed entry script and stylesheets.
  private getHtml(webview: vscode.Webview): string {
    const root = vscode.Uri.joinPath(this.extensionUri, 'dist', 'webview');
    const manifestPath = path.join(root.fsPath, '.vite', 'manifest.json');
    const csp = `default-src 'none'; style-src ${webview.cspSource} 'unsafe-inline'; script-src ${webview.cspSource}; img-src ${webview.cspSource} data:; font-src ${webview.cspSource};`;

    type Chunk = { file: string; css?: string[]; imports?: string[]; isEntry?: boolean };
    let manifest: Record<string, Chunk>;
    try {
      manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8')) as Record<string, Chunk>;
    } catch {
      return `<!DOCTYPE html><html lang="en"><head><meta charset="UTF-8" /><meta http-equiv="Content-Security-Policy" content="${csp}" /></head>
<body style="font-family: var(--vscode-font-family); padding: 24px">
<h2>uVision Fast: the webview is not built</h2>
<p>Run <code>npm run build</code> in the extension folder, then reopen this panel.</p>
</body></html>`;
    }

    const entry = manifest['src/main.tsx'] ?? Object.values(manifest).find((c) => c.isEntry);
    if (!entry) throw new Error('uVision Fast: the webview manifest has no entry chunk');

    // Stylesheets of the entry and of every chunk it imports statically.
    const css = new Set<string>();
    const seen = new Set<string>();
    const walk = (c: Chunk) => {
      for (const f of c.css ?? []) css.add(f);
      for (const key of c.imports ?? []) {
        if (seen.has(key) || !manifest[key]) continue;
        seen.add(key);
        walk(manifest[key]);
      }
    };
    walk(entry);

    const asset = (f: string) => webview.asWebviewUri(vscode.Uri.joinPath(root, f)).toString();
    const links = [...css].map((f) => `  <link rel="stylesheet" href="${asset(f)}" />`).join('\n');

    return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta http-equiv="Content-Security-Policy" content="${csp}" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>Keil µVision 5 - uVision Fast</title>
${links}
</head>
<body>
  <div id="app"></div>
  <script type="module" src="${asset(entry.file)}"></script>
</body>
</html>`;
  }

  public dispose(): void {
    UvisionIdePanel.currentPanel = undefined;
    this.panel.dispose();
    while (this.disposables.length) {
      const d = this.disposables.pop();
      d?.dispose();
    }
  }
}

function errText(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
