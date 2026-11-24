export class WasmEngineClient {
  private instance: WebAssembly.Instance | null = null;
  private encoder = new TextEncoder();
  private decoder = new TextDecoder();
  private initPromise: Promise<void> | null = null;

  async init(wasmUrl = './uvision_core.wasm'): Promise<void> {
    if (this.instance) return;
    if (this.initPromise) return this.initPromise;

    this.initPromise = (async () => {
      const resp = await fetch(wasmUrl);
      if (!resp.ok) {
        throw new Error(`Failed loading ${wasmUrl}: HTTP ${resp.status}`);
      }
      const bytes = await resp.arrayBuffer();
      const { instance } = await WebAssembly.instantiate(bytes, {});
      this.instance = instance;
    })();

    return this.initPromise;
  }

  async rpc<T = any>(method: string, params: Record<string, any> = {}): Promise<T> {
    await this.init();
    const exports = this.instance!.exports as {
      memory: WebAssembly.Memory;
      uv_alloc: (len: number) => number;
      uv_free: (ptr: number, len: number) => void;
      uv_out_ptr: () => number;
      uv_rpc: (ptr: number, len: number) => number;
    };

    const t0 = method === 'build.run' ? performance.now() : 0;
    const reqJson = JSON.stringify({ method, params });
    const reqBytes = this.encoder.encode(reqJson);
    const reqPtr = exports.uv_alloc(reqBytes.length);
    new Uint8Array(exports.memory.buffer, reqPtr, reqBytes.length).set(reqBytes);

    let outLen = 0;
    try {
      outLen = exports.uv_rpc(reqPtr, reqBytes.length) >>> 0;
    } finally {
      exports.uv_free(reqPtr, reqBytes.length);
    }

    const outPtr = exports.uv_out_ptr() >>> 0;
    const outSlice = new Uint8Array(exports.memory.buffer, outPtr, outLen);
    const parsed = JSON.parse(this.decoder.decode(outSlice));
    if (parsed.error) {
      throw new Error(String(parsed.error));
    }

    if (method === 'build.run' && parsed.result && typeof parsed.result === 'object') {
      const elapsed = Math.max(1, Math.round(performance.now() - t0));
      parsed.result.elapsed_ms = elapsed;
      if (typeof parsed.result.log === 'string' && !parsed.result.log.includes(' ms]')) {
        parsed.result.log += ` [${elapsed} ms]`;
      }
    }
    return parsed.result as T;
  }
}
