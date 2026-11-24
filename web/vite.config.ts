import path from 'node:path';
import { tanstackRouter } from '@tanstack/router-plugin/vite';
import react from '@vitejs/plugin-react';
import { defineConfig, type PluginOption } from 'vite';
import { VitePWA } from 'vite-plugin-pwa';

// One React app, two hosts. `vite build` makes the offline PWA in web/dist.
// `vite build --mode vscode` makes the VS Code webview bundle in extension/dist/webview.
export default defineConfig(({ mode }) => {
  const vscode = mode === 'vscode';

  const plugins: PluginOption[] = [
    tanstackRouter({ target: 'react', autoCodeSplitting: true }),
    react(),
  ];

  if (!vscode) {
    plugins.push(
      VitePWA({
        strategies: 'injectManifest',
        srcDir: 'src',
        filename: 'sw.ts',
        registerType: 'prompt',
        injectRegister: false,
        manifest: false,
        injectManifest: {
          globPatterns: ['**/*.{html,js,css,wasm,woff,woff2,ttf,svg,png,webmanifest}'],
          maximumFileSizeToCacheInBytes: 16 * 1024 * 1024,
        },
        devOptions: { enabled: false },
      }),
    );
  }

  return {
    base: './',
    define: {
      __HOST__: JSON.stringify(vscode ? 'vscode' : 'web'),
    },
    plugins,
    server: {
      fs: { allow: ['..'] },
    },
    build: vscode
      ? {
          outDir: path.resolve(__dirname, '../extension/dist/webview'),
          emptyOutDir: true,
          manifest: true,
          copyPublicDir: false,
          target: 'es2022',
          rollupOptions: {
            input: path.resolve(__dirname, 'src/main.tsx'),
          },
        }
      : {
          outDir: 'dist',
          target: 'es2022',
        },
  };
});
