import { VirtualFsStore } from './vfsStore';
import { WasmEngineClient } from './wasmClient';
import { WebIdeHostBridge } from './webBridge';

export function installWebHost(): void {
  const wasmUrl = new URL('./uvision_core.wasm', document.baseURI).href;
  new WebIdeHostBridge(new WasmEngineClient(), new VirtualFsStore(), wasmUrl).install();
}
