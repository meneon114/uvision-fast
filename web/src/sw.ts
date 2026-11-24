/// <reference lib="webworker" />
import { cleanupOutdatedCaches, createHandlerBoundToURL, precacheAndRoute } from 'workbox-precaching';
import { NavigationRoute, registerRoute } from 'workbox-routing';

declare const self: ServiceWorkerGlobalScope & { __WB_MANIFEST: Array<{ url: string; revision: string | null }> };

// precacheAndRoute fails the install event when any entry fails to download,
// so a half-cached app never takes control.
// codicon.css asks for its font as codicon-<hash>.ttf?<md5>: drop that bare md5 parameter
// (as well as Workbox's default utm_/fbclid ones) so the font matches its precache entry offline.
precacheAndRoute(self.__WB_MANIFEST, { ignoreURLParametersMatching: [/^utm_/, /^fbclid$/, /^[0-9a-f]{32}$/] });

// Removes only this worker's outdated Workbox precaches (same scope, -precache- names).
cleanupOutdatedCaches();

registerRoute(new NavigationRoute(createHandlerBoundToURL('index.html')));

// The 0.2 worker kept everything in one cache named uvision-fast-pwa-<hash>.
const LEGACY_CACHE_PREFIX = 'uvision-fast-pwa-';

self.addEventListener('activate', (event) => {
  event.waitUntil(caches.keys().then((keys) => Promise.all(keys.filter((k) => k.startsWith(LEGACY_CACHE_PREFIX)).map((k) => caches.delete(k)))));
});

self.addEventListener('message', (event) => {
  if (event.data && event.data.type === 'SKIP_WAITING') {
    void self.skipWaiting();
  }
});
