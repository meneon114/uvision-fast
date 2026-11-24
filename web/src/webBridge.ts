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

type MsgHandler = (msg: any) => void;

function esc(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c] || c));
}

export class WebIdeHostBridge {
  private handlers: MsgHandler[] = [];
  private initPromise: Promise<void> | null = null;
  private deferredInstallPrompt: any = null;
  private swReady = false;
  private modalHost: HTMLElement | null = null;
  private currentProjectRoot = '/Starter_Lab';

  constructor(
    private readonly wasm: WasmEngineClient,
    private readonly vfs: VirtualFsStore
  ) {
    this.setupInstallAndSwListeners();
  }

  public installAsGlobal(): void {
    const self = this;
    (globalThis as any).__UVISION_WEB_BRIDGE__ = {
      isWeb: true,
      postMessage(msg: any) {
        void self.handleIncoming(msg);
      },
      onMessage(handler: MsgHandler) {
        self.handlers.push(handler);
      },
      getSessionState() {
        return loadSessionStateSync();
      },
      setSessionState(state: any) {
        saveSessionStateSync(state);
      },
      saveDraftSync(path: string, content: string) {
        savePendingDraftSync(path, content);
      },
      clearDraftSync(path: string) {
        clearPendingDraftSync(path);
      },
    };
    this.setupDragAndDrop();
  }

  private emit(msg: any): void {
    for (const h of this.handlers) {
      h(msg);
    }
  }

  public async ensureInitialized(): Promise<void> {
    if (this.initPromise) return this.initPromise;
    this.initPromise = (async () => {
      await this.vfs.init();
      await this.wasm.init('./uvision_core.wasm');
      await this.wasm.rpc('vfs.sync', {
        replace: true,
        files: this.vfs.toRecord(),
      });
    })();
    return this.initPromise;
  }

  private ensureModalHost(): HTMLElement {
    if (!this.modalHost) {
      this.modalHost = document.createElement('div');
      this.modalHost.id = 'uv-web-modal-host';
      document.body.appendChild(this.modalHost);
    }
    return this.modalHost;
  }

  private clearModal(): void {
    if (this.modalHost) {
      this.modalHost.innerHTML = '';
    }
  }

  private pushPwaState(): void {
    const canInstall = !!this.deferredInstallPrompt;
    const offlineReady = this.swReady;
    this.emit({ type: 'setPwaState', canInstall, offlineReady });
  }

  private setupInstallAndSwListeners(): void {
    window.addEventListener('beforeinstallprompt', (e: Event) => {
      e.preventDefault();
      this.deferredInstallPrompt = e;
      this.pushPwaState();
    });

    window.addEventListener('appinstalled', () => {
      this.deferredInstallPrompt = null;
      this.pushPwaState();
    });

    if ('serviceWorker' in navigator) {
      navigator.serviceWorker.addEventListener('controllerchange', () => {
        this.swReady = true;
        this.pushPwaState();
      });
      window.addEventListener('load', async () => {
        try {
          const reg = await navigator.serviceWorker.register('./sw.js', { scope: './' });
          if (reg.active) {
            this.swReady = true;
            this.pushPwaState();
          }
          await navigator.serviceWorker.ready;
          this.swReady = true;
          this.pushPwaState();
        } catch {
          // Dev server (vite dev) doesn't serve sw.js until built
        }
      });
    }
  }

  private setupDragAndDrop(): void {
    window.addEventListener('dragover', (e) => {
      e.preventDefault();
    });
    window.addEventListener('drop', async (e) => {
      e.preventDefault();
      const files = e.dataTransfer?.files;
      if (!files || files.length === 0) return;
      await this.ensureInitialized();
      const opened = await this.importFileList(files);
      if (opened) {
        this.emit({ type: 'openTarget', path: opened });
      }
    });
  }

  private async handleIncoming(msg: any): Promise<void> {
    if (!msg || typeof msg !== 'object') return;

    if (msg.type === 'ready') {
      try {
        await this.ensureInitialized();
        this.pushPwaState();
        const urlTarget = new URLSearchParams(location.search).get('open');
        const fallbackPath = this.vfs.has('/Starter_Lab/Starter_Lab.uvprojx')
          ? '/Starter_Lab/Starter_Lab.uvprojx'
          : this.vfs.listFiles()[0];

        if (urlTarget && this.vfs.has(urlTarget)) {
          this.emit({ type: 'openTarget', path: urlTarget });
        } else {
          const savedSession = loadSessionStateSync();
          if (savedSession && typeof savedSession === 'object') {
            this.emit({ type: 'restoreSession', session: savedSession, fallbackPath });
          } else if (fallbackPath) {
            this.emit({ type: 'openTarget', path: fallbackPath });
          }
        }
      } catch (e: any) {
        console.error('Initialization failed:', e);
      }
      return;
    }

    if (msg.type === 'rpc') {
      const { reqId, method, params } = msg;
      try {
        await this.ensureInitialized();
        const result = await this.wasm.rpc(method, params || {});
        if (method === 'project.open' && result?.root_dir) {
          this.currentProjectRoot = result.root_dir;
        }
        if (method === 'project.new' || method === 'project.addFile' || method === 'project.saveOptions') {
          const dump = await this.wasm.rpc<Record<string, string>>('vfs.dump');
          await this.vfs.syncFromDump(dump);
          if (method === 'project.new' && result?.root_dir) {
            this.currentProjectRoot = result.root_dir;
          }
        }
        this.emit({ type: 'rpcResponse', reqId, result });
      } catch (err: any) {
        this.emit({
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
        await this.ensureInitialized();
        const result = await this.handleHostAction(action, payload || {});
        this.emit({ type: 'hostResponse', reqId, result });
      } catch (err: any) {
        this.emit({
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
        const p = String(payload.path || '');
        return this.vfs.readFile(p);
      }

      case 'writeFile': {
        const p = String(payload.path || '');
        const content = String(payload.content ?? '');
        const disp = await this.vfs.writeFile(p, content);
        await this.wasm.rpc('vfs.write', { path: disp, content });
        return { ok: true };
      }

      case 'pickOpenProjectOrFile': {
        return await this.showWorkspacePickerModal();
      }

      case 'openExamples': {
        return await this.showExamplesModal();
      }

      case 'importLocalFile': {
        return await this.promptLocalFileImport();
      }

      case 'pickExistingSourceFile': {
        return await this.promptLocalFileImport(payload.rootDir || this.currentProjectRoot);
      }

      case 'pickNewProjectDir': {
        return '/Workspace/LabProject';
      }

      case 'promptText': {
        return await this.showPromptModal(
          String(payload.prompt || 'Input'),
          String(payload.value || ''),
          String(payload.placeHolder || '')
        );
      }

      case 'downloadSource': {
        const name = String(payload.name || 'main.s');
        const content = String(payload.content ?? '');
        triggerDownload(name, content, 'text/plain;charset=utf-8');
        return { ok: true };
      }

      case 'downloadFirmware': {
        return await this.showExportModal(String(payload.outputName || 'firmware'));
      }

      case 'flashWebHardware': {
        return await this.showFlashModal(
          String(payload.outputName || 'firmware'),
          String(payload.hwTarget || 'stm32f1x')
        );
      }

      case 'installPwa': {
        if (!this.deferredInstallPrompt) return { installed: false };
        this.deferredInstallPrompt.prompt();
        const choice = await this.deferredInstallPrompt.userChoice;
        if (choice?.outcome === 'accepted') {
          this.deferredInstallPrompt = null;
          this.pushPwaState();
          return { installed: true };
        }
        return { installed: false };
      }

      case 'pickPackFile':
        return null;

      default:
        throw new Error(`Unsupported web host action: ${action}`);
    }
  }

  private async importFileList(files: FileList, targetDir = '/Imported'): Promise<string | null> {
    const cleanDir = targetDir.replace(/\\/g, '/').replace(/\/$/, '') || '/Imported';
    let firstProj: string | null = null;
    let firstSrc: string | null = null;

    for (const f of Array.from(files)) {
      const content = await f.text();
      const vpath = `${cleanDir}/${f.name}`;
      const disp = await this.vfs.writeFile(vpath, content);
      await this.wasm.rpc('vfs.write', { path: disp, content });
      if (/\.uvprojx?$/i.test(f.name) && !firstProj) {
        firstProj = disp;
      } else if (/\.(s|asm|c)$/i.test(f.name) && !firstSrc) {
        firstSrc = disp;
      }
    }
    return firstProj || firstSrc;
  }

  private promptLocalFileImport(targetDir = '/Imported'): Promise<string | null> {
    return new Promise((resolve) => {
      const inp = document.createElement('input');
      inp.type = 'file';
      inp.multiple = true;
      inp.accept = '.s,.asm,.c,.h,.uvprojx,.uvproj';
      inp.style.display = 'none';
      document.body.appendChild(inp);

      let settled = false;
      const done = (val: string | null) => {
        if (settled) return;
        settled = true;
        inp.remove();
        resolve(val);
      };

      inp.addEventListener('change', async () => {
        if (inp.files && inp.files.length > 0) {
          const picked = await this.importFileList(inp.files, targetDir);
          done(picked);
        } else {
          done(null);
        }
      });
      inp.addEventListener('cancel', () => done(null));
      inp.click();
    });
  }

  private showPromptModal(promptText: string, defaultValue: string, placeholder: string): Promise<string | undefined> {
    return new Promise((resolve) => {
      const host = this.ensureModalHost();
      host.innerHTML = `<div class="uv-modal-backdrop"><div class="uv-modal" style="width:420px">
        <div class="uv-modal-head"><span>uVision Fast</span><button class="uv-mini-btn" id="wp-x">×</button></div>
        <div class="uv-modal-body">
          <div style="font-size:11.5px;">${esc(promptText)}</div>
          <input id="wp-val" type="text" value="${esc(defaultValue)}" placeholder="${esc(placeholder)}" style="padding:4px 6px;border:1px solid #888;font-family:Consolas,monospace;font-size:12px;" />
        </div>
        <div class="uv-modal-foot">
          <button class="uv-btn primary" id="wp-ok">OK</button>
          <button class="uv-btn" id="wp-cancel">Cancel</button>
        </div>
      </div></div>`;
      const inp = document.getElementById('wp-val') as HTMLInputElement;
      inp.focus();
      inp.select();
      const finish = (v: string | undefined) => {
        this.clearModal();
        resolve(v);
      };
      document.getElementById('wp-x')!.onclick = () => finish(undefined);
      document.getElementById('wp-cancel')!.onclick = () => finish(undefined);
      document.getElementById('wp-ok')!.onclick = () => finish(inp.value);
      inp.addEventListener('keydown', (e) => {
        if (e.key === 'Enter') finish(inp.value);
        if (e.key === 'Escape') finish(undefined);
      });
    });
  }

  private showWorkspacePickerModal(): Promise<string | null> {
    return new Promise((resolve) => {
      const host = this.ensureModalHost();
      const renderList = () => {
        const files = this.vfs.listFiles().filter((f) => /\.(uvprojx?|s|asm|c|h)$/i.test(f));
        const rows = files
          .map(
            (f) => `<tr class="wb-pick-row" data-path="${esc(f)}" style="cursor:pointer;">
              <td>${esc(f)}</td>
              <td style="width:65px;text-align:center;">
                <button class="uv-mini-btn wb-open" data-path="${esc(f)}">Open</button>
                <button class="uv-mini-btn wb-del" data-path="${esc(f)}" title="Delete from IndexedDB">×</button>
              </td>
            </tr>`
          )
          .join('');
        host.innerHTML = `<div class="uv-modal-backdrop"><div class="uv-modal" style="width:580px">
          <div class="uv-modal-head"><span>Open Workspace Project / File (Offline IndexedDB)</span><button class="uv-mini-btn" id="wb-x">×</button></div>
          <div class="uv-modal-body">
            <div style="font-size:11px;color:#444;">Saved locally in your browser (IndexedDB). Drag &amp; drop any <code>.s</code> or <code>.uvprojx</code> file anywhere onto the IDE to import.</div>
            <div style="max-height:260px;overflow:auto;border:1px solid #c0c0c0;background:#fff;">
              <table class="uv-watch-table">
                <thead><tr><th>Virtual Path</th><th style="width:65px;">Action</th></tr></thead>
                <tbody>${rows || '<tr><td colspan="2">Workspace is empty.</td></tr>'}</tbody>
              </table>
            </div>
          </div>
          <div class="uv-modal-foot">
            <button class="uv-btn" id="wb-import">Import from Disk...</button>
            <button class="uv-btn" id="wb-reset">Restore Examples</button>
            <button class="uv-btn" id="wb-cancel">Cancel</button>
          </div>
        </div></div>`;

        document.getElementById('wb-x')!.onclick = () => { this.clearModal(); resolve(null); };
        document.getElementById('wb-cancel')!.onclick = () => { this.clearModal(); resolve(null); };
        document.getElementById('wb-import')!.onclick = async () => {
          const picked = await this.promptLocalFileImport();
          if (picked) {
            this.clearModal();
            resolve(picked);
          }
        };
        document.getElementById('wb-reset')!.onclick = async () => {
          await this.vfs.resetExamples();
          await this.wasm.rpc('vfs.sync', { files: this.vfs.toRecord() });
          renderList();
        };

        host.querySelectorAll('.wb-open').forEach((b) => {
          b.addEventListener('click', (e) => {
            e.stopPropagation();
            const p = (b as HTMLElement).dataset.path!;
            this.clearModal();
            resolve(p);
          });
        });
        host.querySelectorAll('.wb-pick-row').forEach((r) => {
          r.addEventListener('dblclick', () => {
            const p = (r as HTMLElement).dataset.path!;
            this.clearModal();
            resolve(p);
          });
        });
        host.querySelectorAll('.wb-del').forEach((b) => {
          b.addEventListener('click', async (e) => {
            e.stopPropagation();
            const p = (b as HTMLElement).dataset.path!;
            await this.vfs.deleteFile(p);
            await this.wasm.rpc('vfs.delete', { path: p });
            renderList();
          });
        });
      };
      renderList();
    });
  }

  private showExamplesModal(): Promise<string | null> {
    return new Promise((resolve) => {
      const host = this.ensureModalHost();
      const rows = STARTER_EXAMPLES.map(
        (ex) => `<tr>
          <td><b>${esc(ex.title)}</b><br/><span style="color:#555;font-family:Segoe UI,sans-serif;">${esc(ex.description)}</span></td>
          <td style="width:72px;text-align:center;"><button class="uv-btn primary ex-open" data-id="${esc(ex.id)}">Open</button></td>
        </tr>`
      ).join('');
      host.innerHTML = `<div class="uv-modal-backdrop"><div class="uv-modal" style="width:590px">
        <div class="uv-modal-head"><span>CSE 3108 Starter Examples (ARM Cortex-M3 / STM32F1)</span><button class="uv-mini-btn" id="ex-x">×</button></div>
        <div class="uv-modal-body">
          <table class="uv-watch-table"><thead><tr><th>Example Project</th><th>Action</th></tr></thead><tbody>${rows}</tbody></table>
        </div>
        <div class="uv-modal-foot"><button class="uv-btn" id="ex-cancel">Close</button></div>
      </div></div>`;
      const finish = (v: string | null) => {
        this.clearModal();
        resolve(v);
      };
      document.getElementById('ex-x')!.onclick = () => finish(null);
      document.getElementById('ex-cancel')!.onclick = () => finish(null);
      host.querySelectorAll('.ex-open').forEach((btn) => {
        btn.addEventListener('click', async () => {
          const id = (btn as HTMLElement).dataset.id!;
          const ex = STARTER_EXAMPLES.find((x) => x.id === id);
          if (!ex) return finish(null);
          for (const [p, content] of Object.entries(ex.files)) {
            await this.vfs.writeFile(p, content);
            await this.wasm.rpc('vfs.write', { path: p, content });
          }
          finish(ex.entryPath);
        });
      });
    });
  }

  private async showExportModal(outputName: string): Promise<{ ok: boolean }> {
    const arts = await this.wasm.rpc<any>('build.artifacts');
    if (!arts?.has_artifacts) {
      throw new Error('Build the project first (F7) before exporting artifacts.');
    }
    const base = arts.output_name || outputName || 'firmware';
    return new Promise((resolve) => {
      const host = this.ensureModalHost();
      host.innerHTML = `<div class="uv-modal-backdrop"><div class="uv-modal" style="width:460px">
        <div class="uv-modal-head"><span>Export Built Firmware Artifacts</span><button class="uv-mini-btn" id="exp-x">×</button></div>
        <div class="uv-modal-body">
          <div style="font-size:11.5px;">Target output: <b>${esc(base)}</b></div>
          <div style="display:flex;gap:8px;flex-wrap:wrap;margin-top:4px;">
            <button class="uv-btn primary" id="exp-hex">Download ${esc(base)}.hex</button>
            <button class="uv-btn" id="exp-bin">Download ${esc(base)}.bin</button>
            <button class="uv-btn" id="exp-axf">Download ${esc(base)}.axf (ELF)</button>
          </div>
        </div>
        <div class="uv-modal-foot"><button class="uv-btn" id="exp-close">Close</button></div>
      </div></div>`;
      document.getElementById('exp-x')!.onclick = () => { this.clearModal(); resolve({ ok: true }); };
      document.getElementById('exp-close')!.onclick = () => { this.clearModal(); resolve({ ok: true }); };
      document.getElementById('exp-hex')!.onclick = () => {
        if (arts.hex_text) triggerDownload(`${base}.hex`, arts.hex_text, 'text/plain;charset=utf-8');
      };
      document.getElementById('exp-bin')!.onclick = () => {
        if (arts.bin_b64) triggerDownload(`${base}.bin`, decodeBase64(arts.bin_b64));
      };
      document.getElementById('exp-axf')!.onclick = () => {
        if (arts.elf_b64) triggerDownload(`${base}.axf`, decodeBase64(arts.elf_b64));
      };
    });
  }

  private async showFlashModal(outputName: string, hwTarget: string): Promise<{ ok: boolean }> {
    const arts = await this.wasm.rpc<any>('build.artifacts');
    if (!arts?.has_artifacts || !arts.bin_b64) {
      throw new Error('Build the project first (F7) before flashing hardware.');
    }
    const base = arts.output_name || outputName || 'firmware';
    const binBytes = decodeBase64(arts.bin_b64);
    const serialOk = isWebSerialSupported();

    return new Promise((resolve) => {
      const host = this.ensureModalHost();
      host.innerHTML = `<div class="uv-modal-backdrop"><div class="uv-modal" style="width:560px">
        <div class="uv-modal-head"><span>Flash / Download to STM32 (${esc(hwTarget)})</span><button class="uv-mini-btn" id="fl-x">×</button></div>
        <div class="uv-modal-body">
          <div style="font-size:11.5px;line-height:1.45;">
            <b>Option 1: WebSerial STM32 UART Bootloader (AN3155):</b><br/>
            Connect a USB-UART adapter to <b>USART1 (TX→PA10, RX→PA9)</b>, set <b>BOOT0 = 1</b>, press <b>RESET</b>, then click Flash below.<br/>
            <span style="color:#555;">(${binBytes.length} bytes → Flash 0x08000000 • ${serialOk ? 'WebSerial ready in this browser' : 'Use Chrome/Edge over HTTPS or localhost for WebSerial'})</span>
          </div>
          <div style="display:flex;gap:8px;align-items:center;">
            <button class="uv-btn primary" id="fl-go" ${serialOk ? '' : 'disabled'}>Connect Serial &amp; Flash STM32</button>
            <button class="uv-btn" id="fl-hex">Save .hex</button>
            <button class="uv-btn" id="fl-bin">Save .bin</button>
            <button class="uv-btn" id="fl-axf">Save .axf</button>
          </div>
          <div id="fl-bar-wrap" style="height:10px;border:1px solid #999;background:#fff;">
            <div id="fl-bar" style="width:0%;height:100%;background:#3399ff;"></div>
          </div>
          <div id="fl-log" class="uv-output-pre" style="max-height:140px;overflow:auto;border:1px solid #c0c0c0;background:#fff;">Ready. For ST-Link SWD flashing from desktop without UART bootloader, save .hex/.axf above or run the desktop CLI ("uvcli flash") / VS Code extension.</div>
        </div>
        <div class="uv-modal-foot"><button class="uv-btn" id="fl-close">Close</button></div>
      </div></div>`;

      const logEl = document.getElementById('fl-log')!;
      const barEl = document.getElementById('fl-bar')!;
      const appendLog = (line: string) => {
        logEl.textContent += `\n${line}`;
        logEl.scrollTop = logEl.scrollHeight;
      };

      document.getElementById('fl-x')!.onclick = () => { this.clearModal(); resolve({ ok: true }); };
      document.getElementById('fl-close')!.onclick = () => { this.clearModal(); resolve({ ok: true }); };
      document.getElementById('fl-hex')!.onclick = () => {
        if (arts.hex_text) triggerDownload(`${base}.hex`, arts.hex_text, 'text/plain;charset=utf-8');
      };
      document.getElementById('fl-bin')!.onclick = () => triggerDownload(`${base}.bin`, binBytes);
      document.getElementById('fl-axf')!.onclick = () => {
        if (arts.elf_b64) triggerDownload(`${base}.axf`, decodeBase64(arts.elf_b64));
      };

      document.getElementById('fl-go')!.onclick = async () => {
        const goBtn = document.getElementById('fl-go') as HTMLButtonElement;
        goBtn.disabled = true;
        logEl.textContent = 'Starting STM32 WebSerial Bootloader sequence...';
        barEl.style.background = '#3399ff';
        barEl.style.width = '2%';
        try {
          await flashStm32ViaWebSerial(binBytes, 0x0800_0000, (p) => {
            barEl.style.width = `${p.percent}%`;
            appendLog(`[${p.percent}%] ${p.log}`);
          });
          barEl.style.background = '#2e8b57';
        } catch (e: any) {
          barEl.style.background = '#cc0000';
          appendLog(`Error: ${e.message || String(e)}`);
        } finally {
          goBtn.disabled = !serialOk;
        }
      };
    });
  }
}
