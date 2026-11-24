// Auto-generated offline-first Service Worker for uVision Fast PWA
const CACHE_NAME = 'uvision-fast-pwa-902efb58ca4d';
const PRECACHE_PATHS = [
  "./",
  "./assets/index-4xWfU5FI.css",
  "./assets/index-A6FWBHeZ.js",
  "./assets/main-B0SRjztB.js",
  "./icons/icon-192.png",
  "./icons/icon-512.png",
  "./icons/icon.svg",
  "./index.html",
  "./manifest.webmanifest",
  "./uvision_core.wasm"
];

self.addEventListener('install', (event) => {
  event.waitUntil(
    caches.open(CACHE_NAME).then(async (cache) => {
      const scopeUrl = self.registration.scope;
      await Promise.all(
        PRECACHE_PATHS.map(async (rel) => {
          const url = new URL(rel, scopeUrl).toString();
          try {
            const resp = await fetch(url, { cache: 'no-cache' });
            if (resp.ok) {
              await cache.put(url, resp);
            }
          } catch {
            // Fallback to add if individual fetch needs default mode
            await cache.add(url).catch(() => {});
          }
        })
      );
    }).then(() => self.skipWaiting())
  );
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches.keys().then((keys) =>
      Promise.all(keys.filter((k) => k !== CACHE_NAME).map((k) => caches.delete(k)))
    ).then(() => self.clients.claim())
  );
});

self.addEventListener('fetch', (event) => {
  const req = event.request;
  if (req.method !== 'GET') return;
  const reqUrl = new URL(req.url);
  if (reqUrl.origin !== self.location.origin) return;

  if (req.mode === 'navigate' || reqUrl.pathname.endsWith('/manifest.webmanifest')) {
    event.respondWith(
      fetch(req, { cache: 'no-cache' })
        .then((networkResp) => {
          if (networkResp && networkResp.ok) {
            const copy = networkResp.clone();
            caches.open(CACHE_NAME).then((c) => c.put(req, copy));
          }
          return networkResp;
        })
        .catch(() =>
          caches.match(req, { ignoreSearch: true }).then((cached) => {
            if (cached) return cached;
            const indexUrl = new URL('./index.html', self.registration.scope).toString();
            return caches.match(indexUrl);
          })
        )
    );
    return;
  }

  event.respondWith(
    caches.match(req, { ignoreSearch: true }).then((cached) => {
      if (cached) return cached;
      return fetch(req).then((networkResp) => {
        if (networkResp && networkResp.ok) {
          const copy = networkResp.clone();
          caches.open(CACHE_NAME).then((c) => c.put(req, copy));
        }
        return networkResp;
      });
    })
  );
});
