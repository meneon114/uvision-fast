import { createHashHistory, createRouter } from '@tanstack/react-router';
import { RouteError } from './components/shell/RouteStates';
import { routeTree } from './routeTree.gen';

// Hash history: the PWA is served from a sub-path on GitHub Pages and the
// VS Code webview has no server to rewrite deep links.
export const router = createRouter({
  routeTree,
  history: createHashHistory(),
  defaultPreload: 'intent',
  defaultPreloadStaleTime: 30_000,
  defaultPendingMs: 150,
  defaultErrorComponent: RouteError,
  scrollRestoration: false,
});

declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router;
  }
}
