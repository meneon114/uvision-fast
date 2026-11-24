import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
import { UvCliClient } from './cliClient';

export class UvisionIdePanel {
  public static currentPanel: UvisionIdePanel | undefined;

  private readonly panel: vscode.WebviewPanel;
  private readonly disposables: vscode.Disposable[] = [];
  private readonly recentlySaved = new Map<string, number>();

  public static createOrShow(
    context: vscode.ExtensionContext,
    cli: UvCliClient,
    initialTarget?: string
  ): UvisionIdePanel {
    if (UvisionIdePanel.currentPanel) {
      UvisionIdePanel.currentPanel.panel.reveal(vscode.ViewColumn.One);
      if (initialTarget) {
        void UvisionIdePanel.currentPanel.openTargetPath(initialTarget);
      }
      return UvisionIdePanel.currentPanel;
    }

    const panel = vscode.window.createWebviewPanel(
      'uvisionFast.ide',
      'Keil µVision 5 — uVision Fast',
      vscode.ViewColumn.One,
      {
        enableScripts: true,
        retainContextWhenHidden: true,
        localResourceRoots: [vscode.Uri.joinPath(context.extensionUri, 'dist')],
      }
    );

    const instance = new UvisionIdePanel(panel, context.extensionUri, cli, initialTarget);
    UvisionIdePanel.currentPanel = instance;
    return instance;
  }

  private constructor(
    panel: vscode.WebviewPanel,
    private readonly extensionUri: vscode.Uri,
    private readonly cli: UvCliClient,
    private initialTarget?: string
  ) {
    this.panel = panel;
    this.panel.webview.html = this.getHtml(this.panel.webview);

    this.panel.onDidDispose(() => this.dispose(), null, this.disablesArray());

    this.panel.webview.onDidReceiveMessage(
      async (msg) => {
        await this.handleWebviewMessage(msg);
      },
      null,
      this.disposables
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

  private disablesArray() {
    return this.disposables;
  }

  public async openTargetPath(targetPath: string): Promise<void> {
    this.panel.webview.postMessage({ type: 'openTarget', path: targetPath });
  }

  public sendCommand(cmd: 'build' | 'rebuild' | 'debug' | 'flash'): void {
    this.panel.webview.postMessage({ type: 'hostAction', action: cmd });
  }

  private async handleWebviewMessage(msg: any): Promise<void> {
    if (!msg || typeof msg !== 'object') {
      return;
    }

    if (msg.type === 'ready') {
      const target = this.initialTarget || this.detectWorkspaceTarget();
      this.initialTarget = undefined;
      if (target) {
        this.panel.webview.postMessage({ type: 'openTarget', path: target });
      }
      return;
    }

    if (msg.type === 'rpc') {
      const { reqId, method, params } = msg;
      try {
        const result = await this.cli.request(method, params || {});
        this.panel.webview.postMessage({ type: 'rpcResponse', reqId, result });
      } catch (err: any) {
        this.panel.webview.postMessage({
          type: 'rpcResponse',
          reqId,
          error: err?.message || String(err),
        });
      }
      return;
    }

    if (msg.type === 'host') {
      const { reqId, action, payload } = msg;
      try {
        const result = await this.handleHostAction(action, payload || {});
        this.panel.webview.postMessage({ type: 'hostResponse', reqId, result });
      } catch (err: any) {
        this.panel.webview.postMessage({
          type: 'hostResponse',
          reqId,
          error: err?.message || String(err),
        });
      }
    }
  }

  private async handleHostAction(action: string, payload: any): Promise<any> {
    switch (action) {
      case 'readFile': {
        const filePath = String(payload.path || '');
        const content = fs.readFileSync(filePath, 'utf8');
        return { path: filePath, content };
      }
      case 'writeFile': {
        const filePath = String(payload.path || '');
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
        return uris?.[0]?.fsPath || null;
      }
      case 'pickNewProjectDir': {
        const uris = await vscode.window.showOpenDialog({
          canSelectFiles: false,
          canSelectFolders: true,
          canSelectMany: false,
          openLabel: 'Select Project Folder',
          defaultUri: vscode.workspace.workspaceFolders?.[0]?.uri,
        });
        return uris?.[0]?.fsPath || null;
      }
      case 'pickExistingSourceFile': {
        const uris = await vscode.window.showOpenDialog({
          canSelectMany: false,
          filters: {
            'ARM Assembly & C': ['s', 'asm', 'c', 'h'],
            'All Files': ['*'],
          },
          defaultUri: payload.rootDir ? vscode.Uri.file(payload.rootDir) : undefined,
        });
        return uris?.[0]?.fsPath || null;
      }
      case 'pickPackFile': {
        const uris = await vscode.window.showOpenDialog({
          canSelectMany: false,
          filters: { 'CMSIS Pack': ['pack'] },
        });
        return uris?.[0]?.fsPath || null;
      }
      case 'promptText': {
        return await vscode.window.showInputBox({
          prompt: payload.prompt || 'Input',
          value: payload.value || '',
          placeHolder: payload.placeHolder || '',
        });
      }
      case 'openInVsCode': {
        const p = String(payload.path || '');
        const doc = await vscode.workspace.openTextDocument(vscode.Uri.file(p));
        await vscode.window.showTextDocument(doc, vscode.ViewColumn.Beside);
        return { ok: true };
      }
      case 'scanWorkspace': {
        const root = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || '';
        const uvFiles = await vscode.workspace.findFiles('**/*.uvprojx', '**/node_modules/**', 20);
        const asmFiles = await vscode.workspace.findFiles('**/*.{s,c}', '**/Software/**', 50);
        return {
          workspaceRoot: root,
          projects: uvFiles.map((u) => u.fsPath),
          sources: asmFiles.map((u) => u.fsPath),
        };
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

  private getHtml(webview: vscode.Webview): string {
    const cssUri = webview.asWebviewUri(vscode.Uri.joinPath(this.extensionUri, 'dist', 'keil.css'));
    const jsUri = webview.asWebviewUri(vscode.Uri.joinPath(this.extensionUri, 'dist', 'webview.js'));

    return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${webview.cspSource} 'unsafe-inline'; script-src ${webview.cspSource}; img-src ${webview.cspSource} data:; font-src ${webview.cspSource};" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>Keil µVision 5 — uVision Fast</title>
  <link rel="stylesheet" href="${cssUri}" />
</head>
<body>
  <div id="uv-root"></div>
  <script src="${jsUri}"></script>
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
