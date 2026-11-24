import { collectSeedFiles } from './examples';

const DB_NAME = 'uvision-fast-vfs';
const DB_VERSION = 1;
const STORE_FILES = 'files';
const STORE_META = 'meta';

export class VirtualFsStore {
  private db: IDBDatabase | null = null;
  private cache = new Map<string, string>(); // normPath -> content
  private keyMap = new Map<string, string>(); // lowercase norm -> display path

  private static normDisp(p: string): string {
    const clean = p.replace(/\\/g, '/');
    return clean.startsWith('/') ? clean : '/' + clean;
  }

  private static normKey(p: string): string {
    return VirtualFsStore.normDisp(p).toLowerCase();
  }

  async init(): Promise<void> {
    this.db = await new Promise<IDBDatabase>((resolve, reject) => {
      const req = indexedDB.open(DB_NAME, DB_VERSION);
      req.onupgradeneeded = () => {
        const db = req.result;
        if (!db.objectStoreNames.contains(STORE_FILES)) {
          db.createObjectStore(STORE_FILES);
        }
        if (!db.objectStoreNames.contains(STORE_META)) {
          db.createObjectStore(STORE_META);
        }
      };
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => reject(req.error || new Error('IndexedDB open failed'));
    });

    await this.loadAllFromDb();

    const seeded = await this.getMeta('seeded_v1');
    if (!seeded || this.cache.size === 0) {
      const seeds = collectSeedFiles();
      for (const [p, content] of Object.entries(seeds)) {
        if (!this.has(p)) {
          await this.writeFile(p, content);
        }
      }
      await this.setMeta('seeded_v1', true);
    }
  }

  private async loadAllFromDb(): Promise<void> {
    if (!this.db) return;
    this.cache.clear();
    this.keyMap.clear();
    await new Promise<void>((resolve, reject) => {
      const tx = this.db!.transaction(STORE_FILES, 'readonly');
      const store = tx.objectStore(STORE_FILES);
      const req = store.openCursor();
      req.onsuccess = () => {
        const cursor = req.result;
        if (cursor) {
          const disp = VirtualFsStore.normDisp(String(cursor.key));
          const key = disp.toLowerCase();
          this.keyMap.set(key, disp);
          this.cache.set(key, String(cursor.value ?? ''));
          cursor.continue();
        } else {
          resolve();
        }
      };
      req.onerror = () => reject(req.error);
    });
  }

  private async getMeta(key: string): Promise<any> {
    if (!this.db) return undefined;
    return new Promise((resolve) => {
      const tx = this.db!.transaction(STORE_META, 'readonly');
      const req = tx.objectStore(STORE_META).get(key);
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => resolve(undefined);
    });
  }

  private async setMeta(key: string, val: any): Promise<void> {
    if (!this.db) return;
    await new Promise<void>((resolve, reject) => {
      const tx = this.db!.transaction(STORE_META, 'readwrite');
      tx.objectStore(STORE_META).put(val, key);
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error);
    });
  }

  has(path: string): boolean {
    return this.cache.has(VirtualFsStore.normKey(path));
  }

  readFile(path: string): { path: string; content: string } {
    const key = VirtualFsStore.normKey(path);
    const content = this.cache.get(key);
    if (content === undefined) {
      throw new Error(`File not found in workspace: ${path}`);
    }
    return { path: this.keyMap.get(key) || VirtualFsStore.normDisp(path), content };
  }

  async writeFile(path: string, content: string): Promise<string> {
    const disp = VirtualFsStore.normDisp(path);
    const key = disp.toLowerCase();
    const prevDisp = this.keyMap.get(key);
    if (prevDisp && prevDisp !== disp && this.db) {
      await this.deleteFromDb(prevDisp);
    }
    this.keyMap.set(key, disp);
    this.cache.set(key, content);
    if (this.db) {
      await new Promise<void>((resolve, reject) => {
        const tx = this.db!.transaction(STORE_FILES, 'readwrite');
        tx.objectStore(STORE_FILES).put(content, disp);
        tx.oncomplete = () => resolve();
        tx.onerror = () => reject(tx.error);
      });
    }
    return disp;
  }

  private async deleteFromDb(dispKey: string): Promise<void> {
    if (!this.db) return;
    await new Promise<void>((resolve) => {
      const tx = this.db!.transaction(STORE_FILES, 'readwrite');
      tx.objectStore(STORE_FILES).delete(dispKey);
      tx.oncomplete = () => resolve();
      tx.onerror = () => resolve();
    });
  }

  async deleteFile(path: string): Promise<void> {
    const key = VirtualFsStore.normKey(path);
    const disp = this.keyMap.get(key);
    this.cache.delete(key);
    this.keyMap.delete(key);
    if (disp) await this.deleteFromDb(disp);
  }

  listFiles(): string[] {
    return Array.from(this.keyMap.values()).sort((a, b) => a.localeCompare(b));
  }

  toRecord(): Record<string, string> {
    const out: Record<string, string> = {};
    for (const [k, disp] of this.keyMap.entries()) {
      out[disp] = this.cache.get(k) ?? '';
    }
    return out;
  }

  async syncFromDump(dump: Record<string, string>): Promise<void> {
    for (const [p, content] of Object.entries(dump)) {
      const key = VirtualFsStore.normKey(p);
      if (this.cache.get(key) !== content) {
        await this.writeFile(p, content);
      }
    }
  }

  async resetExamples(): Promise<void> {
    const seeds = collectSeedFiles();
    for (const [p, content] of Object.entries(seeds)) {
      await this.writeFile(p, content);
    }
  }
}

export function triggerDownload(filename: string, data: string | Uint8Array, mime = 'application/octet-stream'): void {
  const copy = typeof data === 'string' ? undefined : new Uint8Array(data);
  const blob = typeof data === 'string'
    ? new Blob([data], { type: mime })
    : new Blob([copy!.buffer], { type: mime });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  setTimeout(() => {
    a.remove();
    URL.revokeObjectURL(url);
  }, 500);
}

export function decodeBase64(b64: string): Uint8Array {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) {
    bytes[i] = bin.charCodeAt(i);
  }
  return bytes;
}
