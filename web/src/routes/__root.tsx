import { createRootRoute, Outlet } from '@tanstack/react-router';
import { OverlayHost } from '@ui-sdk/react/overlays';
import { useEffect } from 'react';
import { importFiles } from '../components/dialogs/WorkspaceDialogs';
import { boot } from '../state/ide';
import { useIde } from '../state/store';

let booted = false;

function useTheme(): void {
  const theme = useIde((s) => s.theme);
  useEffect(() => {
    const root = document.documentElement;
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    // In a VS Code webview, "system" means the editor's theme, which VS Code
    // reports as a class on <body> (vscode-light, vscode-dark, vscode-high-contrast).
    const body = document.body.classList;
    const inVsCode = __HOST__ === 'vscode';
    const systemDark = () =>
      inVsCode ? body.contains('vscode-dark') || (body.contains('vscode-high-contrast') && !body.contains('vscode-high-contrast-light')) : mq.matches;
    const apply = () => {
      const dark = theme === 'dark' || (theme === 'system' && systemDark());
      root.dataset.theme = dark ? 'dark' : 'light';
      root.style.colorScheme = dark ? 'dark' : 'light';
    };
    apply();
    mq.addEventListener('change', apply);
    const obs = inVsCode ? new MutationObserver(apply) : null;
    obs?.observe(document.body, { attributes: true, attributeFilter: ['class'] });
    return () => {
      mq.removeEventListener('change', apply);
      obs?.disconnect();
    };
  }, [theme]);
}

// Drop files anywhere on the window to import them into the browser workspace.
function useDropImport(): void {
  const hostKind = useIde((s) => s.hostKind);
  useEffect(() => {
    if (hostKind !== 'web') return;
    const over = (e: DragEvent) => {
      if (e.dataTransfer?.types.includes('Files')) {
        e.preventDefault();
        document.documentElement.dataset.drop = 'true';
      }
    };
    const leave = (e: DragEvent) => {
      if (!e.relatedTarget) delete document.documentElement.dataset.drop;
    };
    const drop = (e: DragEvent) => {
      delete document.documentElement.dataset.drop;
      if (!e.dataTransfer?.files.length) return;
      e.preventDefault();
      void importFiles([...e.dataTransfer.files]);
    };
    window.addEventListener('dragover', over);
    window.addEventListener('dragleave', leave);
    window.addEventListener('drop', drop);
    return () => {
      window.removeEventListener('dragover', over);
      window.removeEventListener('dragleave', leave);
      window.removeEventListener('drop', drop);
    };
  }, [hostKind]);
}

function Root() {
  if (!booted) {
    booted = true;
    boot();
  }
  useTheme();
  useDropImport();
  return (
    <>
      <Outlet />
      <OverlayHost
        toast={{
          className:
            'mb-8 [&_[data-slot=toast]]:border [&_[data-slot=toast]]:border-border2 [&_[data-slot=toast]]:rounded-md [&_[data-slot=toast]]:bg-bg [&_[data-slot=toast]]:text-fg2 [&_[data-slot=toast]]:shadow-pop',
        }}
      />
    </>
  );
}

export const Route = createRootRoute({ component: Root });
