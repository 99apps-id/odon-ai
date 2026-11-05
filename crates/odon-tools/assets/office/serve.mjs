// Static server + same-origin daemon proxy for the Agents Office starter.
//
// The ODON preview iframe has an opaque origin, so it cannot call the daemon
// directly (the daemon rejects `Origin: null` and CORS would not help). This
// process is local and trusted: it reads the loopback token from
// ~/.odon/api-token (or ODON_API_TOKEN) and adds it to every /api request.
// The page only ever talks to http://127.0.0.1:<port>/api, with no token in
// the browser and no CORS widening on the daemon.
import { createServer, request as httpRequest } from 'node:http';
import { readFile } from 'node:fs/promises';
import { readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { extname, join, normalize, sep } from 'node:path';

const root = process.cwd();
const port = Number(process.env.PORT || {{PORT}});
if (!Number.isInteger(port) || port < 1 || port > 65535) {
  console.error(`agents office: invalid PORT '${process.env.PORT}'`);
  process.exit(1);
}
let daemon;
try {
  daemon = new URL(process.env.ODON_DAEMON_URL || 'http://127.0.0.1:4096');
} catch {
  console.error(`agents office: ODON_DAEMON_URL '${process.env.ODON_DAEMON_URL}' is not a valid URL`);
  process.exit(1);
}
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
  '.webp': 'image/webp',
  '.svg': 'image/svg+xml',
};

function loopbackToken() {
  if (process.env.ODON_API_TOKEN) return process.env.ODON_API_TOKEN.trim();
  const home = process.env.ODON_HOME || homedir();
  try {
    return readFileSync(join(home, '.odon', 'api-token'), 'utf8').trim();
  } catch {
    return '';
  }
}

// The generated page calls this server on its own origin, so no CORS headers
// are needed. `Access-Control-Allow-Origin: *` would let any website the
// operator visits read daemon responses (and mutate state) through the
// token-attaching proxy, so it is deliberately omitted.
function corsHeaders() {
  return {
    'Cache-Control': 'no-store',
  };
}

function proxy(request, response) {
  const token = loopbackToken();
  if (!token) {
    response.writeHead(401, { ...corsHeaders(), 'Content-Type': 'application/json' });
    response.end(
      JSON.stringify({
        error:
          'No loopback token found. Start odond so it writes ~/.odon/api-token, or set ODON_API_TOKEN.',
      }),
    );
    return;
  }

  const headers = { ...request.headers };
  headers.host = daemon.host;
  headers.authorization = `Bearer ${token}`;
  delete headers.origin;
  delete headers.referer;

  const upstream = httpRequest(
    {
      hostname: daemon.hostname,
      port: daemon.port || 80,
      path: request.url,
      method: request.method,
      headers,
    },
    (reply) => {
      const replyHeaders = { ...reply.headers, ...corsHeaders() };
      response.writeHead(reply.statusCode || 502, replyHeaders);
      reply.pipe(response);
    },
  );
  upstream.on('error', () => {
    if (!response.headersSent) {
      response.writeHead(502, { ...corsHeaders(), 'Content-Type': 'application/json' });
    }
    response.end(JSON.stringify({ error: `ODON daemon is not reachable at ${daemon.origin}` }));
  });
  request.pipe(upstream);
}

createServer(async (request, response) => {
  if (request.method === 'OPTIONS') {
    response.writeHead(204, corsHeaders()).end();
    return;
  }
  if (request.url.startsWith('/api/')) {
    proxy(request, response);
    return;
  }

  try {
    const url = new URL(request.url, `http://${request.headers.host}`);
    const pathname = decodeURIComponent(url.pathname === '/' ? '/index.html' : url.pathname);
    const file = normalize(join(root, pathname));
    if (file !== root && !file.startsWith(root + sep)) {
      response.writeHead(403, corsHeaders()).end('Forbidden');
      return;
    }
    const data = await readFile(file);
    response.writeHead(200, {
      ...corsHeaders(),
      'Content-Type': types[extname(file).toLowerCase()] || 'application/octet-stream',
    });
    response.end(data);
  } catch {
    response.writeHead(404, { ...corsHeaders(), 'Content-Type': 'text/plain; charset=utf-8' });
    response.end('Not found');
  }
}).listen(port, '127.0.0.1', () => {
  console.log(`agents office: http://127.0.0.1:${port}  (daemon ${daemon.origin})`);
});
