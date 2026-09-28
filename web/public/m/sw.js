// The phone app's service worker (PRD mobile-companion D-16, D-21, B23,
// B31, B32, B34). It caches only the static shell: the page, its hashed
// assets, the manifest and the icons, so the app opens without a network and
// shows the unreachable line rather than a blank page. Agent state, rows,
// replies and the push subscription travel on /ws and are never cached here.
//
// There is no start_url in the manifest on purpose: the Home Screen app keeps
// the address it was added from, whose fragment carries the phone's
// credential into an installed app whose storage iOS may keep apart.

const CACHE = "hide-phone-shell-v1";
const SHELL = ["/m/", "/m/manifest.webmanifest", "/m/icon-192.png", "/m/icon-512.png", "/m/apple-touch-icon.png"];

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(CACHE)
      .then((cache) => cache.addAll(SHELL))
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((key) => key !== CACHE).map((key) => caches.delete(key))))
      .then(() => self.clients.claim()),
  );
});

function isShell(url) {
  return url.origin === self.location.origin && (url.pathname.startsWith("/m/") || url.pathname.startsWith("/assets/"));
}

self.addEventListener("fetch", (event) => {
  const request = event.request;
  const url = new URL(request.url);
  if (request.method !== "GET" || !isShell(url) || url.pathname === "/m/sw.js") return;
  if (url.pathname.startsWith("/assets/")) {
    // Hashed file names never change content: the cache answers first.
    event.respondWith(
      caches.match(request).then(
        (hit) =>
          hit ??
          fetch(request).then((response) => {
            if (response.ok) {
              const copy = response.clone();
              void caches.open(CACHE).then((cache) => cache.put(request, copy));
            }
            return response;
          }),
      ),
    );
    return;
  }
  // The page and the manifest: the network first, so a new build lands at once.
  const key = request.mode === "navigate" ? "/m/" : request;
  event.respondWith(
    fetch(request)
      .then((response) => {
        if (response.ok) {
          const copy = response.clone();
          void caches.open(CACHE).then((cache) => cache.put(key, copy));
        }
        return response;
      })
      .catch(() => caches.match(key).then((hit) => hit ?? Response.error())),
  );
});

async function closeTags(tags) {
  if (!Array.isArray(tags) || tags.length === 0) return;
  const shown = await self.registration.getNotifications();
  for (const notification of shown) if (tags.includes(notification.tag)) notification.close();
}

self.addEventListener("push", (event) => {
  let payload = null;
  try {
    payload = event.data ? event.data.json() : null;
  } catch {
    payload = null;
  }
  if (!payload || typeof payload.title !== "string") return;
  event.waitUntil(
    closeTags(payload.clear).then(() =>
      self.registration.showNotification(payload.title, {
        body: payload.body,
        tag: payload.tag,
        icon: "/m/icon-192.png",
        badge: "/m/icon-192.png",
        data: { device_id: payload.device_id, pane_id: payload.pane_id },
      }),
    ),
  );
});

self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  const data = event.notification.data ?? {};
  const tag = data.device_id && data.pane_id ? `${data.device_id}|${data.pane_id}` : null;
  event.waitUntil(
    self.clients.matchAll({ type: "window", includeUncontrolled: true }).then((windows) => {
      const open = windows.find((client) => new URL(client.url).pathname.startsWith("/m/"));
      if (open) {
        if (tag) open.postMessage({ type: "open", tag });
        return open.focus();
      }
      return self.clients.openWindow(tag ? `/m/#open=${encodeURIComponent(tag)}` : "/m/");
    }),
  );
});
