import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const webDir = path.resolve(__dirname, '..');
const cliDir = path.resolve(webDir, '..', 'cli');
const wasmOut = path.join(cliDir, 'target', 'wasm32-unknown-unknown', 'release', 'uvision_core.wasm');
const pubDir = path.join(webDir, 'public');

console.log('Building Rust uvision_core.wasm (wasm32-unknown-unknown release)...');
execFileSync('cargo', ['build', '--lib', '--target', 'wasm32-unknown-unknown', '--release'], {
  cwd: cliDir,
  stdio: 'inherit',
  shell: process.platform === 'win32',
});

if (!fs.existsSync(wasmOut)) {
  throw new Error(`Expected WASM output at ${wasmOut}`);
}

fs.mkdirSync(pubDir, { recursive: true });
const dest = path.join(pubDir, 'uvision_core.wasm');
fs.copyFileSync(wasmOut, dest);
const bytes = fs.statSync(dest).size;
console.log(`Copied uvision_core.wasm (${(bytes / 1024).toFixed(1)} KB) -> ${dest}`);
