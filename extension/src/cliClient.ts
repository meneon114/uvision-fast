import * as cp from 'child_process';
import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';

interface PendingReq {
  resolve: (value: any) => void;
  reject: (reason?: any) => void;
}

export class UvCliClient {
  private proc: cp.ChildProcessWithoutNullStreams | null = null;
  private nextId = 1;
  private pending = new Map<number, PendingReq>();
  private buffer = '';
  private disposed = false;

  constructor(private readonly extensionPath: string) {}

  public resolveCliPath(): string {
    const cfg = vscode.workspace.getConfiguration('uvisionFast');
    const configured = (cfg.get<string>('cliPath') || '').trim();
    if (configured && fs.existsSync(configured)) {
      return configured;
    }

    const candidates = [
      path.join(this.extensionPath, 'bin', 'uvcli.exe'),
      path.resolve(this.extensionPath, '..', 'cli', 'target', 'release', 'uvcli.exe'),
      path.resolve(this.extensionPath, '..', 'cli', 'target', 'debug', 'uvcli.exe'),
      'E:\\3-1\\3108 - Computer Architecture Sessional\\Software\\uVision-Fast\\cli\\target\\release\\uvcli.exe',
    ];
    for (const c of candidates) {
      if (fs.existsSync(c)) {
        return c;
      }
    }
    return 'uvcli.exe';
  }

  private ensureStarted(): void {
    if (this.proc && !this.proc.killed && this.proc.exitCode === null) {
      return;
    }
    const cliExe = this.resolveCliPath();
    const cfg = vscode.workspace.getConfiguration('uvisionFast');
    const keilRoot = (cfg.get<string>('keilRoot') || '').trim();
    const openocdPath = (cfg.get<string>('openocdPath') || '').trim();

    const args: string[] = [];
    if (keilRoot) {
      args.push('--keil-root', keilRoot);
    }
    if (openocdPath) {
      args.push('--openocd', openocdPath);
    }
    args.push('serve');

    this.buffer = '';
    this.proc = cp.spawn(cliExe, args, {
      stdio: ['pipe', 'pipe', 'pipe'],
      windowsHide: true,
    });

    this.proc.stdout.setEncoding('utf8');
    this.proc.stdout.on('data', (chunk: string) => {
      this.buffer += chunk;
      let idx: number;
      while ((idx = this.buffer.indexOf('\n')) !== -1) {
        const line = this.buffer.slice(0, idx).trim();
        this.buffer = this.buffer.slice(idx + 1);
        if (!line) {
          continue;
        }
        try {
          const msg = JSON.parse(line);
          const req = this.pending.get(msg.id);
          if (req) {
            this.pending.delete(msg.id);
            if (msg.error) {
              req.reject(new Error(msg.error.message || 'uvcli RPC error'));
            } else {
              req.resolve(msg.result);
            }
          }
        } catch {
          // Ignore malformed stdout line
        }
      }
    });

    this.proc.stderr.setEncoding('utf8');
    this.proc.stderr.on('data', (err: string) => {
      console.error('[uvcli stderr]', err);
    });

    this.proc.on('exit', (code) => {
      const err = new Error(`uvcli process exited (code ${code})`);
      for (const [, req] of this.pending) {
        req.reject(err);
      }
      this.pending.clear();
      this.proc = null;
    });

    this.proc.on('error', (err) => {
      for (const [, req] of this.pending) {
        req.reject(err);
      }
      this.pending.clear();
      this.proc = null;
    });
  }

  public request<T = any>(method: string, params: Record<string, any> = {}): Promise<T> {
    if (this.disposed) {
      return Promise.reject(new Error('UvCliClient is disposed'));
    }
    this.ensureStarted();
    const id = this.nextId++;
    const payload = JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n';
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      if (!this.proc || !this.proc.stdin.writable) {
        this.pending.delete(id);
        reject(new Error('uvcli stdin not writable'));
        return;
      }
      this.proc.stdin.write(payload, 'utf8');
    });
  }

  public dispose(): void {
    this.disposed = true;
    if (this.proc) {
      try {
        this.proc.kill();
      } catch {
        // ignore
      }
      this.proc = null;
    }
    this.pending.clear();
  }
}
