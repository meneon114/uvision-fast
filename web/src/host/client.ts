import { getIdeBridge, type IdeBridge } from './bridge';
import type { RpcMethod } from './protocol';
import type { HostEvent, SessionState, UvProject } from './types';

type Pending = { resolve: (v: unknown) => void; reject: (e: Error) => void };

// Methods that act on the engine's "current project" rather than a path they
// are given. Route preloading can open another project in the engine, so these
// re-open the active one first.
const PROJECT_SCOPED = new Set<RpcMethod>(['project.saveOptions']);

export class HostClient {
  readonly bridge: IdeBridge;
  readonly kind: 'web' | 'vscode';
  private seq = 1;
  private pending = new Map<number, Pending>();
  private listeners = new Set<(e: HostEvent) => void>();
  private queued: HostEvent[] = [];
  private engineProject: string | null = null;
  activeProject: string | null = null;

  constructor() {
    this.bridge = getIdeBridge();
    this.kind = this.bridge.isWeb ? 'web' : 'vscode';
    this.bridge.onMessage((raw) => this.receive(raw));
  }

  private receive(raw: unknown): void {
    if (!raw || typeof raw !== 'object') return;
    const msg = raw as { type?: string; reqId?: number; result?: unknown; error?: string };
    if ((msg.type === 'rpcResponse' || msg.type === 'hostResponse') && typeof msg.reqId === 'number') {
      const p = this.pending.get(msg.reqId);
      if (!p) return;
      this.pending.delete(msg.reqId);
      if (msg.error) p.reject(new Error(String(msg.error)));
      else p.resolve(msg.result);
      return;
    }
    const ev = raw as HostEvent;
    if (this.listeners.size === 0) {
      this.queued.push(ev);
      return;
    }
    for (const l of this.listeners) l(ev);
  }

  onEvent(fn: (e: HostEvent) => void): () => void {
    this.listeners.add(fn);
    if (this.queued.length) {
      const q = this.queued.splice(0);
      for (const e of q) fn(e);
    }
    return () => this.listeners.delete(fn);
  }

  ready(): void {
    this.bridge.postMessage({ type: 'ready' });
  }

  private send<T>(type: 'rpc' | 'host', name: string, params: Record<string, unknown>): Promise<T> {
    const reqId = this.seq++;
    return new Promise<T>((resolve, reject) => {
      this.pending.set(reqId, { resolve: resolve as (v: unknown) => void, reject });
      if (type === 'rpc') this.bridge.postMessage({ type, reqId, method: name, params });
      else this.bridge.postMessage({ type, reqId, action: name, payload: params });
    });
  }

  async rpc<T = unknown>(method: RpcMethod, params: Record<string, unknown> = {}): Promise<T> {
    if (PROJECT_SCOPED.has(method) && this.activeProject && this.engineProject !== this.activeProject) {
      await this.openProject(this.activeProject);
    }
    return this.send<T>('rpc', method, params);
  }

  async openProject(path: string): Promise<UvProject> {
    const proj = await this.send<UvProject>('rpc', 'project.open', { path });
    this.engineProject = path;
    return proj;
  }

  noteEngineProject(path: string | null): void {
    this.engineProject = path;
  }

  host<T = unknown>(action: string, payload: Record<string, unknown> = {}): Promise<T> {
    return this.send<T>('host', action, payload);
  }

  readFile(path: string): Promise<{ path: string; content: string }> {
    return this.host('readFile', { path });
  }

  writeFile(path: string, content: string): Promise<void> {
    return this.host('writeFile', { path, content });
  }

  loadSession(): SessionState | null {
    try {
      return this.bridge.getSessionState?.() ?? null;
    } catch {
      return null;
    }
  }

  saveSession(s: SessionState): void {
    try {
      this.bridge.setSessionState?.(s);
    } catch {
      // storage may be unavailable (private mode); session restore is best effort
    }
  }

  saveDraft(path: string, content: string): void {
    this.bridge.saveDraftSync?.(path, content);
  }

  clearDraft(path: string): void {
    this.bridge.clearDraftSync?.(path);
  }
}

let client: HostClient | null = null;

export function host(): HostClient {
  if (!client) client = new HostClient();
  return client;
}
