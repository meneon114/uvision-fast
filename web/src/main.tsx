import '@ui-sdk/core/web.css';
import '@vscode/codicons/dist/codicon.css';
import '@fontsource/jetbrains-mono/400.css';
import '@fontsource/jetbrains-mono/600.css';
import './styles/app.css';
import { RouterProvider } from '@tanstack/react-router';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { setRouter } from './lib/nav';

async function start(): Promise<void> {
  // The PWA hosts the engine in the page; VS Code hosts it in the extension.
  if (__HOST__ === 'web') {
    const { installWebHost } = await import('./host/web/boot');
    installWebHost();
  }
  const { router } = await import('./router');
  setRouter(router);
  if (__HOST__ === 'web') void registerPwa();
  createRoot(document.getElementById('app')!).render(
    <StrictMode>
      <RouterProvider router={router} />
    </StrictMode>,
  );
}

async function registerPwa(): Promise<void> {
  if (!('serviceWorker' in navigator) || import.meta.env.DEV) return;
  const [{ registerSW }, { toast }, { ide }] = await Promise.all([import('virtual:pwa-register'), import('@ui-sdk/react/toast'), import('./state/store')]);
  const updateSW = registerSW({
    immediate: true,
    onNeedRefresh() {
      toast('A new version of uVision Fast is ready', {
        description: 'Reload to update. Your files are saved.',
        action: { label: 'Reload', onAction: () => void updateSW(true) },
        duration: Infinity,
      });
    },
    onOfflineReady() {
      ide.set((s) => ({ pwa: { ...s.pwa, offlineReady: true } }));
      toast.success('Ready to work offline');
    },
  });
}

void start();
