export interface IdeBridge {
  isWeb: boolean;
  postMessage(msg: any): void;
  onMessage(handler: (msg: any) => void): void;
}

declare function acquireVsCodeApi(): {
  postMessage(msg: any): void;
  getState(): any;
  setState(s: any): void;
};

export function getIdeBridge(): IdeBridge {
  const g = globalThis as any;
  if (typeof acquireVsCodeApi === 'function') {
    const vscode = acquireVsCodeApi();
    return {
      isWeb: false,
      postMessage: (msg: any) => vscode.postMessage(msg),
      onMessage: (handler: (msg: any) => void) => {
        window.addEventListener('message', (ev) => handler(ev.data));
      },
    };
  }
  if (g.__UVISION_WEB_BRIDGE__) {
    return g.__UVISION_WEB_BRIDGE__ as IdeBridge;
  }
  throw new Error('IDE host bridge is not initialized.');
}
