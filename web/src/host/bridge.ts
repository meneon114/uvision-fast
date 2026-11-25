import type { SessionState } from './types';

// The message channel between the UI and its host. The VS Code webview gets
// it from acquireVsCodeApi; the PWA installs an in-page bridge on globalThis.
export interface IdeBridge {
  isWeb: boolean;
  postMessage(msg: unknown): void;
  onMessage(handler: (msg: unknown) => void): void;
  getSessionState?(): SessionState | null;
  setSessionState?(state: SessionState): void;
  saveDraftSync?(path: string, content: string): void;
  clearDraftSync?(path: string): void;
}

declare function acquireVsCodeApi(): {
  postMessage(msg: unknown): void;
  getState(): unknown;
  setState(s: unknown): void;
};

declare global {
  // eslint-disable-next-line no-var
  var __UVISION_WEB_BRIDGE__: IdeBridge | undefined;
}

let cached: IdeBridge | null = null;

export function getIdeBridge(): IdeBridge {
  if (cached) return cached;
  if (typeof acquireVsCodeApi === 'function') {
    const vscode = acquireVsCodeApi();
    cached = {
      isWeb: false,
      postMessage: (msg) => vscode.postMessage(msg),
      onMessage: (handler) => window.addEventListener('message', (ev) => handler(ev.data)),
      getSessionState: () => (vscode.getState() as SessionState | null) ?? null,
      setSessionState: (state) => vscode.setState(state),
    };
    return cached;
  }
  if (globalThis.__UVISION_WEB_BRIDGE__) {
    cached = globalThis.__UVISION_WEB_BRIDGE__;
    return cached;
  }
  throw new Error('IDE host bridge is not initialized.');
}
