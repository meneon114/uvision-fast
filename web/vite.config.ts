import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { defineConfig, Plugin } from 'vite';

function walkFiles(dir: string, baseDir = dir): string[] {
  const out: string[] = [];
  for (const ent of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, ent.name);
    if (ent.isDirectory()) {
      out.push(...walkFiles(full, baseDir));
    } else {
      const rel = path.relative(baseDir, full).replace(/\\/g, '/');
      if (rel !== 'sw.js' && !rel.endsWith('.map')) {
        out.push('./' + rel);
      }
    }
  }
  return out;
}

function uvisionPwaSwPlugin(): Plugin {
  return {
    name: 'uvision-pwa-sw',
    closeBundle() {
      const distDir = path.resolve(__dirname, 'dist');
      if (!fs.existsSync(distDir)) return;
      const assets = Array.from(new Set(['./', './index.html', ...walkFiles(distDir)])).sort();
      const hash = crypto
        .createHash('sha256')
        .update(assets.map((a) => {
          const p = a === './' ? path.join(distDir, 'index.html') : path.join(distDir, a.slice(2));
          return `${a}:${fs.existsSync(p) ? fs.statSync(p).size : 0}`;
        }).join('|'))
        .digest('hex')
        .slice(0, 12);

      const swSource = `// Auto-generated offline-first Service Worker for uVision Fast PWA
const CACHE_NAME = 'uvision-fast-pwa-${hash}';
const PRECACHE_PATHS = ${JSON.stringify(assets, null, 2)};

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

  if (req.mode === 'navigate') {
    event.respondWith(
      caches.match(req, { ignoreSearch: true }).then((cached) => {
        if (cached) return cached;
        const indexUrl = new URL('./index.html', self.registration.scope).toString();
        return caches.match(indexUrl).then((idx) => idx || fetch(req));
      })
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
`;
      fs.writeFileSync(path.join(distDir, 'sw.js'), swSource, 'utf8');
      console.log(`Generated offline sw.js (${assets.length} precached assets, cache ${hash}).`);
    },
  };
}

export default defineConfig({
  base: './',
  server: {
    fs: {
      allow: ['..'],
    },
  },
  plugins: [uvisionPwaSwPlugin()],
});
