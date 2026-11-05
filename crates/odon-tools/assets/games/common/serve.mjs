// Zero-dependency static server for the ODON preview panel.
//
// The panel renders the page inside a sandboxed iframe with an opaque origin,
// so ES modules and fetch() from that page are cross-origin requests. Every
// response therefore carries `Access-Control-Allow-Origin: *`. This server is
// for local development only; do not expose it on a public interface.
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { extname, join, normalize, sep } from 'node:path';

const root = process.cwd();
const port = Number(process.env.PORT || {{PORT}});
const types = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.gltf': 'model/gltf+json',
  '.glb': 'model/gltf-binary',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.webp': 'image/webp',
  '.svg': 'image/svg+xml',
  '.wav': 'audio/wav',
  '.mp3': 'audio/mpeg',
};

createServer(async (request, response) => {
  const cors = {
    'Access-Control-Allow-Origin': '*',
    'Access-Control-Allow-Methods': 'GET, HEAD, OPTIONS',
    'Access-Control-Allow-Headers': '*',
    'Cache-Control': 'no-store',
  };
  if (request.method === 'OPTIONS') {
    response.writeHead(204, cors).end();
    return;
  }

  try {
    const url = new URL(request.url, `http://${request.headers.host}`);
    const pathname = decodeURIComponent(url.pathname === '/' ? '/index.html' : url.pathname);
    const file = normalize(join(root, pathname));
    if (file !== root && !file.startsWith(root + sep)) {
      response.writeHead(403, cors).end('Forbidden');
      return;
    }
    const data = await readFile(file);
    response.writeHead(200, {
      ...cors,
      'Content-Type': types[extname(file).toLowerCase()] || 'application/octet-stream',
    });
    response.end(data);
  } catch {
    response.writeHead(404, { ...cors, 'Content-Type': 'text/plain; charset=utf-8' });
    response.end('Not found');
  }
}).listen(port, '127.0.0.1', () => {
  console.log(`preview ready: http://127.0.0.1:${port}`);
});
