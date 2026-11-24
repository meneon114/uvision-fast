import * as esbuild from 'esbuild';
import { spawnSync } from 'node:child_process';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';

// Builds the extension host (esbuild) and the webview UI (the shared React app in
// ../web, built with `vite build --mode vscode` into dist/webview).
//
//   node esbuild.mjs               dev build: a missing uvcli/OpenOCD only warns
//   node esbuild.mjs --production  packaging build (vscode:prepublish): a missing
//                                  uvcli.exe fails the build, so a .vsix never ships
//                                  with an empty bin/
//   node esbuild.mjs --watch       rebuilds the extension host on change

const here = path.dirname(fileURLToPath(import.meta.url));
const watch = process.argv.includes('--watch');
const production = process.argv.includes('--production');

function fail(msg) {
  console.error(`\n\x1b[31merror:\x1b[0m ${msg}\n`);
  process.exit(1);
}

function warn(msg) {
  console.warn(`\x1b[33mwarning:\x1b[0m ${msg}`);
}

function buildWebview() {
  const webDir = path.resolve(here, '..', 'web');
  if (!fs.existsSync(path.join(webDir, 'node_modules'))) {
    fail(`web/node_modules is missing. Run "npm ci" in ${webDir} first.`);
  }
  const npx = process.platform === 'win32' ? 'npx.cmd' : 'npx';
  const r = spawnSync(npx, ['vite', 'build', '--mode', 'vscode'], { cwd: webDir, stdio: 'inherit', shell: process.platform === 'win32' });
  if (r.status !== 0) fail('The webview build (vite build --mode vscode) failed.');
  const manifest = path.join(here, 'dist', 'webview', '.vite', 'manifest.json');
  if (!fs.existsSync(manifest)) fail(`The webview build did not write ${manifest}.`);
}

function copyBinaries() {
  const binDir = path.join(here, 'bin');
  fs.mkdirSync(binDir, { recursive: true });

  const cliSrc = path.resolve(here, '..', 'cli', 'target', 'release', 'uvcli.exe');
  const cliDst = path.join(binDir, 'uvcli.exe');
  if (fs.existsSync(cliSrc)) {
    fs.copyFileSync(cliSrc, cliDst);
  } else if (!fs.existsSync(cliDst)) {
    const msg = `uvcli.exe not found at ${cliSrc}. Build it with "cargo build --release" in cli/ (Windows target).`;
    if (production) fail(`${msg}\nRefusing to package an extension with an empty bin/.`);
    warn(`${msg} The extension will fall back to uvisionFast.cliPath or PATH.`);
  }

  const ocdSrcRoot = path.resolve(here, '..', '..', 'openocd');
  const ocdBinSrc = path.join(ocdSrcRoot, 'bin');
  const ocdScriptsSrc = path.join(ocdSrcRoot, 'openocd', 'scripts');
  const ocdDstRoot = path.join(binDir, 'openocd');
  if (fs.existsSync(ocdBinSrc) && fs.existsSync(ocdScriptsSrc)) {
    fs.cpSync(ocdBinSrc, path.join(ocdDstRoot, 'bin'), { recursive: true });
    fs.cpSync(ocdScriptsSrc, path.join(ocdDstRoot, 'openocd', 'scripts'), { recursive: true });
  } else if (!fs.existsSync(path.join(ocdDstRoot, 'bin'))) {
    // OpenOCD is only needed for hardware flashing and debugging; the simulator works without it.
    warn(`OpenOCD not found at ${ocdSrcRoot}. Hardware flash/debug will need uvisionFast.openocdPath.`);
  }
}

const extCtx = await esbuild.context({
  entryPoints: [path.join(here, 'src', 'extension.ts')],
  bundle: true,
  outfile: path.join(here, 'dist', 'extension.js'),
  external: ['vscode'],
  format: 'cjs',
  platform: 'node',
  target: 'node18',
  sourcemap: !production,
  minify: production,
});

if (watch) {
  await extCtx.watch();
  console.log('Watching the extension host. Rebuild the webview with "npm run build:webview".');
} else {
  await extCtx.rebuild();
  await extCtx.dispose();
  buildWebview();
  copyBinaries();
  console.log(`Built extension and webview${production ? ' (production)' : ''}.`);
}
