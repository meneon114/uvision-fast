import * as esbuild from 'esbuild';
import * as fs from 'fs';
import * as path from 'path';

const watch = process.argv.includes('--watch');

const copyAssetsPlugin = {
  name: 'copy-assets',
  setup(build) {
    build.onEnd(() => {
      fs.mkdirSync('dist', { recursive: true });
      const src = path.join('webview', 'keil.css');
      const dst = path.join('dist', 'keil.css');
      if (fs.existsSync(src)) {
        fs.copyFileSync(src, dst);
      }

      const binDir = path.join('bin');
      fs.mkdirSync(binDir, { recursive: true });
      const cliSrc = path.resolve('..', 'cli', 'target', 'release', 'uvcli.exe');
      if (fs.existsSync(cliSrc)) {
        fs.copyFileSync(cliSrc, path.join(binDir, 'uvcli.exe'));
      }

      const ocdSrcRoot = path.resolve('..', '..', 'openocd');
      const ocdBinSrc = path.join(ocdSrcRoot, 'bin');
      const ocdScriptsSrc = path.join(ocdSrcRoot, 'openocd', 'scripts');
      const ocdDstRoot = path.join(binDir, 'openocd');
      if (fs.existsSync(ocdBinSrc)) {
        fs.cpSync(ocdBinSrc, path.join(ocdDstRoot, 'bin'), { recursive: true });
      }
      if (fs.existsSync(ocdScriptsSrc)) {
        fs.cpSync(ocdScriptsSrc, path.join(ocdDstRoot, 'openocd', 'scripts'), { recursive: true });
      }
    });
  },
};

const extCtx = await esbuild.context({
  entryPoints: ['src/extension.ts'],
  bundle: true,
  outfile: 'dist/extension.js',
  external: ['vscode'],
  format: 'cjs',
  platform: 'node',
  target: 'node18',
  sourcemap: true,
});

const webCtx = await esbuild.context({
  entryPoints: ['webview/main.ts'],
  bundle: true,
  outfile: 'dist/webview.js',
  format: 'iife',
  platform: 'browser',
  target: 'es2022',
  sourcemap: true,
  plugins: [copyAssetsPlugin],
});

if (watch) {
  await Promise.all([extCtx.watch(), webCtx.watch()]);
  console.log('Watching extension and webview...');
} else {
  await Promise.all([extCtx.rebuild(), webCtx.rebuild()]);
  await Promise.all([extCtx.dispose(), webCtx.dispose()]);
  console.log('Built extension and webview.');
}
