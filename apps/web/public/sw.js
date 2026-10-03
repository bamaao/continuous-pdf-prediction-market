/* Minimal offline shell for PWA (FR-UI-02). Caches the app shell only — not RPC or Market API. */
const CACHE = "cpm-shell-v1";
const SHELL = ["/", "/manifest.json", "/icon-512.png", "/icon-180.png"];

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches.open(CACHE).then((cache) => cache.addAll(SHELL)).then(() => self.skipWaiting()),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches.keys().then((keys) =>
      Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))),
    ).then(() => self.clients.claim()),
  );
});

self.addEventListener("fetch", (event) => {
  const req = event.request;
  if (req.method !== "GET") return;
  const url = new URL(req.url);
  if (url.origin !== self.location.origin) return;
  // Never cache API / compose / wallet traffic.
  if (url.pathname.startsWith("/api/") || url.pathname.startsWith("/v1/")) return;
  event.respondWith(
    caches.match(req).then((hit) =>
      hit ||
      fetch(req)
        .then((res) => {
          if (res.ok && (url.pathname === "/" || url.pathname.endsWith(".js") || url.pathname.endsWith(".css"))) {
            const copy = res.clone();
            caches.open(CACHE).then((c) => c.put(req, copy));
          }
          return res;
        })
        .catch(() => caches.match("/") || Response.error()),
    ),
  );
});
