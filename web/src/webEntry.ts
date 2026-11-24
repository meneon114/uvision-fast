import '../../extension/webview/keil.css';
import { VirtualFsStore } from './vfsStore';
import { WasmEngineClient } from './wasmClient';
import { WebIdeHostBridge } from './webBridge';

const wasm = new WasmEngineClient();
const vfs = new VirtualFsStore();
const bridge = new WebIdeHostBridge(wasm, vfs);
bridge.installAsGlobal();

// Dynamically import the shared Keil uVision 5 UI after __UVISION_WEB_BRIDGE__ is registered
void import('../../extension/webview/main');
