// The cache of real places in the browser: what geodata keeps on disk natively (the maps' tiles,
// OurAirports' tables, the regions built), kept in Cache Storage. geodata's workers read
// `cache/<key>` (GET, HEAD; GET with a range for the start of a file) and write it (PUT) with
// synchronous requests (crates/sim/geodata/src/disk.rs), which this answers. Every other request
// goes to the network as if it were not here.
const STORE = "flight-cache";

self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", (event) => event.waitUntil(self.clients.claim()));

self.addEventListener("fetch", (event) => {
  const url = new URL(event.request.url);
  if (url.origin === location.origin && url.pathname.startsWith(new URL("cache/", self.registration.scope).pathname)) {
    event.respondWith(answer(event.request, url.href));
  }
});

async function answer(request, key) {
  const store = await caches.open(STORE);
  switch (request.method) {
    case "GET":
    case "HEAD": {
      const kept = await store.match(key);
      if (!kept) return new Response(null, { status: 404 });
      if (request.method === "HEAD") return new Response(null, { status: 200 });
      const range = /^bytes=0-(\d+)$/.exec(request.headers.get("Range") ?? "");
      return range ? new Response(await start(kept, Number(range[1]) + 1), { status: 206 }) : kept;
    }
    case "PUT":
      await store.put(key, new Response(await request.arrayBuffer()));
      return new Response(null, { status: 204 });
    default:
      return new Response(null, { status: 405 });
  }
}

// The first `length` bytes of a kept answer, without reading the rest (a region is some 80 MB).
async function start(kept, length) {
  const reader = kept.body.getReader();
  const out = new Uint8Array(length);
  let filled = 0;
  while (filled < length) {
    const { done, value } = await reader.read();
    if (done) break;
    const part = value.subarray(0, length - filled);
    out.set(part, filled);
    filled += part.length;
  }
  reader.cancel();
  return out.subarray(0, filled);
}
