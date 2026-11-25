import type { IdeBridge } from '../bridge';
import type { ExampleInfo, SessionState } from '../types';
import { STARTER_EXAMPLES } from './examples';
import {
  VirtualFsStore,
  clearPendingDraftSync,
  decodeBase64,
  loadSessionStateSync,
  savePendingDraftSync,
  saveSessionStateSync,
  triggerDownload,
} from './vfsStore';
import { WasmEngineClient } from './wasmClient';
import { flashStm32ViaWebSerial, isWebSerialSupported } from './webSerialFlasher';

type MsgHandler = (msg: unknown) => void;

interface BeforeInstallPromptEvent extends Event {
  prompt(): Promise<void>;
  userChoice: Promise<{ outcome: 'accepted' | 'dismissed' }>;
}

interface RpcMsg {
  type: 'rpc';
  reqId: number;
  method: string;
  params?: Record<string, unknown>;
}

interface HostMsg {
  type: 'host';
  reqId: number;
  action: string;
  payload?: Record<string, unknown>;
}

const FALLBACK_PROJECT = '/Blinky/blinky.uvprojx';
const SOURCE_RE = /\.(uvprojx?|s|asm|c|h)$/i;

// The PWA's host: the WASM engine plus an IndexedDB file store, behind the same
// message protocol the VS Code extension speaks. It renders nothing; dialogs
// for picking, exporting and flashing live in the React app.
export class WebIdeHostBridge {
  private handlers: MsgHandler[] = [];
  private initPromise: Promise<void> | null = null;
  private installPrompt: BeforeInstallPromptEvent | null = null;

  constructor(
    private readonly wasm: WasmEngineClient,
    private readonly vfs: VirtualFsStore,
    private readonly wasmUrl: string,
  ) {
    window.addEventListener('beforeinstallprompt', (e) => {
      e.preventDefault();
      this.installPrompt = e as BeforeInstallPromptEvent;
      this.pushPwaState();
    });
    window.addEventListener('appinstalled', () => {
      this.installPrompt = null;
      this.pushPwaState();
    });
  }

  install(): IdeBridge {
    const bridge: IdeBridge = {
      isWeb: true,
      postMessage: (msg) => void this.handleIncoming(msg),
      onMessage: (handler) => this.handlers.push(handler),
      getSessionState: () => loadSessionStateSync() as SessionState | null,
      setSessionState: (state) => saveSessionStateSync(state),
      saveDraftSync: (path, content) => savePendingDraftSync(path, content),
      clearDraftSync: (path) => clearPendingDraftSync(path),
    };
    globalThis.__UVISION_WEB_BRIDGE__ = bridge;
    this.setupDragAndDrop();
    return bridge;
  }

  private emit(msg: unknown): void {
    for (const h of this.handlers) h(msg);
  }

  private ensureInitialized(): Promise<void> {
    if (!this.initPromise) {
      this.initPromise = (async () => {
        await this.vfs.init();
        await this.wasm.init(this.wasmUrl);
        await this.wasm.rpc('vfs.sync', { replace: true, files: this.vfs.toRecord() });
      })();
    }
    return this.initPromise;
  }

  private pushPwaState(): void {
    this.emit({ type: 'setPwaState', canInstall: !!this.installPrompt, offlineReady: false });
  }

  private setupDragAndDrop(): void {
    window.addEventListener('dragover', (e) => e.preventDefault());
    window.addEventListener('drop', async (e) => {
      e.preventDefault();
      const files = e.dataTransfer?.files;
      if (!files || files.length === 0) return;
      await this.ensureInitialized();
      const list = await Promise.all(Array.from(files).map(async (f) => ({ name: f.name, content: await f.text() })));
      const opened = await this.importFiles(list, '/Imported');
      if (opened) this.emit({ type: 'openTarget', path: opened });
    });
  }

  private async handleIncoming(raw: unknown): Promise<void> {
    if (!raw || typeof raw !== 'object') return;
    const msg = raw as { type?: string };

    if (msg.type === 'ready') {
      try {
        await this.ensureInitialized();
        this.pushPwaState();
        const urlTarget = new URLSearchParams(location.search).get('open');
        const fallbackPath = this.vfs.has(FALLBACK_PROJECT) ? FALLBACK_PROJECT : this.vfs.listFiles().find((f) => SOURCE_RE.test(f));
        if (urlTarget && this.vfs.has(urlTarget)) {
          this.emit({ type: 'openTarget', path: urlTarget });
        } else {
          this.emit({ type: 'restoreSession', session: loadSessionStateSync(), fallbackPath });
        }
      } catch (e) {
        console.error('Initialization failed:', e);
      }
      return;
    }

    if (msg.type === 'rpc') {
      const { reqId, method, params } = raw as RpcMsg;
      try {
        await this.ensureInitialized();
        const result = await this.wasm.rpc(method, params || {});
        if (method === 'project.new' || method === 'project.addFile' || method === 'project.saveOptions') {
          const dump = await this.wasm.rpc<Record<string, string>>('vfs.dump');
          await this.vfs.syncFromDump(dump);
        }
        this.emit({ type: 'rpcResponse', reqId, result });
      } catch (err) {
        this.emit({ type: 'rpcResponse', reqId, error: errText(err) });
      }
      return;
    }

    if (msg.type === 'host') {
      const { reqId, action, payload } = raw as HostMsg;
      try {
        await this.ensureInitialized();
        const result = await this.hostAction(action, payload || {});
        this.emit({ type: 'hostResponse', reqId, result });
      } catch (err) {
        this.emit({ type: 'hostResponse', reqId, error: errText(err) });
      }
    }
  }

  private async hostAction(action: string, p: Record<string, unknown>): Promise<unknown> {
    switch (action) {
      case 'readFile':
        return this.vfs.readFile(String(p.path || ''));

      case 'writeFile': {
        const content = String(p.content ?? '');
        const disp = await this.vfs.writeFile(String(p.path || ''), content);
        await this.wasm.rpc('vfs.write', { path: disp, content });
        return { ok: true };
      }

      case 'listWorkspace':
        return this.vfs.listFiles().filter((f) => SOURCE_RE.test(f));

      case 'deleteWorkspaceFile': {
        const path = String(p.path || '');
        await this.vfs.deleteFile(path);
        await this.wasm.rpc('vfs.delete', { path });
        return { ok: true };
      }

      case 'resetExamples':
        await this.vfs.resetExamples();
        await this.wasm.rpc('vfs.sync', { files: this.vfs.toRecord() });
        return { ok: true };

      case 'listExamples':
        return STARTER_EXAMPLES.map<ExampleInfo>(({ id, title, lab, description, entryPath }) => ({ id, title, lab, description, entryPath }));

      case 'installExample': {
        const ex = STARTER_EXAMPLES.find((x) => x.id === p.id);
        if (!ex) throw new Error(`Unknown example: ${String(p.id)}`);
        for (const [path, content] of Object.entries(ex.files)) {
          if (this.vfs.has(path) && p.overwrite !== true) continue;
          await this.vfs.writeFile(path, content);
          await this.wasm.rpc('vfs.write', { path, content });
        }
        return ex.entryPath;
      }

      case 'importFiles': {
        const files = Array.isArray(p.files) ? (p.files as { name: string; content: string }[]) : [];
        return this.importFiles(files, typeof p.dir === 'string' ? p.dir : '/Imported');
      }

      case 'pickNewProjectDir':
        return '/Workspace/LabProject';

      case 'downloadFile': {
        const name = String(p.name || 'download.bin');
        if (typeof p.text === 'string') triggerDownload(name, p.text, String(p.mime || 'text/plain;charset=utf-8'));
        else if (typeof p.b64 === 'string') triggerDownload(name, decodeBase64(p.b64), String(p.mime || 'application/octet-stream'));
        return { ok: true };
      }

      case 'webSerialSupported':
        return isWebSerialSupported();

      case 'flashWebSerial': {
        const arts = await this.wasm.rpc<{ has_artifacts: boolean; bin_b64?: string }>('build.artifacts');
        if (!arts?.has_artifacts || !arts.bin_b64) throw new Error('Build the target (F7) before downloading to the board.');
        await flashStm32ViaWebSerial(decodeBase64(arts.bin_b64), 0x0800_0000, (prog) => {
          this.emit({ type: 'flashProgress', percent: prog.percent, log: prog.log });
        });
        return { ok: true };
      }

      case 'installPwa': {
        if (!this.installPrompt) return { installed: false };
        await this.installPrompt.prompt();
        const choice = await this.installPrompt.userChoice;
        if (choice.outcome === 'accepted') {
          this.installPrompt = null;
          this.pushPwaState();
          return { installed: true };
        }
        return { installed: false };
      }

      default:
        throw new Error(`Unsupported web host action: ${action}`);
    }
  }

  private async importFiles(files: { name: string; content: string }[], targetDir: string): Promise<string | null> {
    const dir = targetDir.replace(/\\/g, '/').replace(/\/$/, '') || '/Imported';
    let firstProj: string | null = null;
    let firstSrc: string | null = null;
    for (const f of files) {
      const base = f.name.replace(/\\/g, '/').split('/').pop() || f.name;
      const disp = await this.vfs.writeFile(`${dir}/${base}`, f.content);
      await this.wasm.rpc('vfs.write', { path: disp, content: f.content });
      if (/\.uvprojx?$/i.test(base)) firstProj ??= disp;
      else if (/\.(s|asm|c)$/i.test(base)) firstSrc ??= disp;
    }
    return firstProj || firstSrc;
  }
}

function errText(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
