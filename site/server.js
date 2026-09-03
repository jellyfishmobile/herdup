// Static file server for the herdup landing page.
//
// No dependencies on purpose: the whole site is three files, and a build step
// or a framework here would be more moving parts than the thing it serves.
// Railway sets PORT; everything else is defaulted.

import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL(".", import.meta.url));
const PORT = Number(process.env.PORT) || 3000;

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ico": "image/x-icon",
  ".webmanifest": "application/manifest+json",
};

const server = createServer(async (req, res) => {
  try {
    const url = new URL(req.url ?? "/", "http://localhost");
    // Resolve inside ROOT only: normalize first, then reject anything that
    // climbed out with `..`.
    let pathname = decodeURIComponent(url.pathname);
    if (pathname.endsWith("/")) pathname += "index.html";
    const rel = normalize(pathname).replace(/^([/\\])+/, "");
    if (rel.split(/[/\\]/).includes("..")) {
      res.writeHead(403).end("forbidden");
      return;
    }

    const file = join(ROOT, rel);
    const ext = extname(file).toLowerCase();
    const body = await readFile(file);

    // HTML, CSS and JS revalidate every time. They are a few KB, and the
    // alternative is a deploy leaving returning visitors on new markup with an
    // hour-old stylesheet. Media is immutable in practice and gets a long TTL.
    const revalidate = ext === ".html" || ext === ".css" || ext === ".js";

    res.writeHead(200, {
      "content-type": TYPES[ext] ?? "application/octet-stream",
      "cache-control": revalidate ? "no-cache" : "public, max-age=86400",
      "x-content-type-options": "nosniff",
      "referrer-policy": "strict-origin-when-cross-origin",
    });
    res.end(body);
  } catch {
    // One page, so anything missing goes to it rather than a bare 404 body.
    try {
      const body = await readFile(join(ROOT, "index.html"));
      res.writeHead(404, { "content-type": TYPES[".html"] }).end(body);
    } catch {
      res.writeHead(404).end("not found");
    }
  }
});

server.listen(PORT, () => {
  console.log(`herdup site listening on ${PORT}`);
});
