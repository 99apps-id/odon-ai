// ODON WhatsApp Web bridge.
//
// Uses Baileys (WhatsApp multi-device protocol) to expose a tiny local HTTP
// API: start/QR/pairing, status, logout, and send. Inbound messages are
// forwarded to the ODON daemon so the normal agent runtime/tools handle them.
//
// This process is managed by `odond`; it can also be run manually:
//   npm install && node index.mjs
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import QRCode from 'qrcode';
import pino from 'pino';
import makeWASocket, {
  useMultiFileAuthState,
  DisconnectReason,
  fetchLatestBaileysVersion,
  Browsers,
  downloadMediaMessage,
} from '@whiskeysockets/baileys';

const PORT = Number(process.env.WA_BRIDGE_PORT || 4097);
const HOME = process.env.USERPROFILE || process.env.HOME || '.';
const AUTH_DIR = process.env.WA_AUTH_DIR || path.join(HOME, '.odon', 'whatsapp-auth');
const DAEMON_URL = process.env.ODON_DAEMON_URL || 'http://127.0.0.1:4096';
const API_TOKEN = process.env.ODON_API_TOKEN || '';
const BRIDGE_TOKEN = process.env.WA_BRIDGE_TOKEN || '';
const ALLOWED = (process.env.WA_ALLOWED_NUMBERS || '')
  .split(',')
  .map((v) => v.replace(/\D/g, ''))
  .filter(Boolean);
const ALLOW_GROUPS = process.env.WA_ALLOW_GROUPS === '1';
const ALLOW_ANYONE = process.env.WA_ALLOW_ANYONE === '1';
const logger = pino({ level: process.env.WA_BRIDGE_LOG_LEVEL || 'silent' });

fs.mkdirSync(AUTH_DIR, { recursive: true });

let sock = null;
let connected = false;
let qrDataUrl = null;
let pairingCode = null;
let requestedPhone = (process.env.WA_PAIRING_PHONE || '').replace(/\D/g, '') || null;
let me = null;
let lastError = null;
let starting = false;
let reconnectTimer = null;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function splitText(text, max = 3500) {
  if (text.length <= max) return [text];
  const chunks = [];
  let rest = text;
  while (rest.length > max) {
    let cut = rest.lastIndexOf('\n', max);
    if (cut < max * 0.5) cut = max;
    chunks.push(rest.slice(0, cut));
    rest = rest.slice(cut);
  }
  if (rest) chunks.push(rest);
  return chunks;
}

function extractText(message) {
  if (!message) return '';
  return (
    message.conversation ||
    message.extendedTextMessage?.text ||
    message.imageMessage?.caption ||
    message.videoMessage?.caption ||
    message.documentMessage?.caption ||
    message.audioMessage?.caption ||
    message.buttonsResponseMessage?.selectedDisplayText ||
    message.listResponseMessage?.title ||
    ''
  );
}

function findMedia(message) {
  if (!message) return null;
  if (message.imageMessage) {
    return { kind: 'image', content: message.imageMessage };
  }
  if (message.documentMessage) {
    return { kind: 'document', content: message.documentMessage };
  }
  if (message.videoMessage) {
    return { kind: 'video', content: message.videoMessage };
  }
  if (message.audioMessage) {
    return { kind: 'audio', content: message.audioMessage };
  }
  return null;
}

function fallbackFileName(kind, mime) {
  const extension = String(mime || '').split('/').pop()?.split(';')[0] || 'bin';
  return `whatsapp-${kind}-${Date.now()}.${extension}`;
}

async function extractAttachments(message) {
  const media = findMedia(message);
  if (!media) {
    return { attachments: [], notes: [] };
  }
  const notes = [];
  try {
    const buffer = await downloadMediaMessage(message, {
      reuploadRequest: sock.updateMediaMessage,
    });
    if (!buffer?.length) {
      notes.push(`[Attached ${media.kind} could not be downloaded from WhatsApp Web.]`);
      return { attachments: [], notes };
    }
    if (buffer.length > 20 * 1024 * 1024) {
      notes.push(`[Attached ${media.kind} is larger than 20 MB and was not forwarded.]`);
      return { attachments: [], notes };
    }
    const mime = String(media.content.mimetype || 'application/octet-stream');
    const attachments = [{
      name: String(media.content.fileName || fallbackFileName(media.kind, mime)),
      mime_type: mime,
      data_base64: buffer.toString('base64'),
    }];
    return { attachments, notes };
  } catch (error) {
    notes.push(`[Attached ${media.kind} download failed: ${error.message}]`);
    return { attachments: [], notes };
  }
}

function normalizeJid(to) {
  if (to.includes('@')) return to;
  return `${String(to).replace(/\D/g, '')}@s.whatsapp.net`;
}

async function notifyDaemon(message) {
  try {
    const headers = { 'Content-Type': 'application/json' };
    if (API_TOKEN) headers.Authorization = `Bearer ${API_TOKEN}`;
    if (BRIDGE_TOKEN) headers['x-bridge-token'] = BRIDGE_TOKEN;
    const res = await fetch(`${DAEMON_URL}/api/relays/whatsapp/inbound`, {
      method: 'POST',
      headers,
      body: JSON.stringify(message),
    });
    if (!res.ok) {
      console.error(`[bridge] daemon inbound rejected: HTTP ${res.status}`);
    }
  } catch (error) {
    console.error(`[bridge] daemon inbound failed: ${error.message}`);
  }
}

function status() {
  return {
    running: true,
    connected,
    qr: qrDataUrl,
    pairing_code: pairingCode,
    phone: me?.id?.split(':')[0] || requestedPhone || null,
    jid: me?.id || null,
    push_name: me?.name || null,
    error: lastError,
    updated_at: new Date().toISOString(),
  };
}

function scheduleReconnect() {
  if (reconnectTimer) clearTimeout(reconnectTimer);
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    startSocket(requestedPhone).catch((error) => {
      lastError = error.message;
      console.error(`[bridge] reconnect failed: ${error.message}`);
    });
  }, 3000);
}

async function startSocket(phone = null) {
  if (starting) return;
  if (phone) requestedPhone = String(phone).replace(/\D/g, '');

  if (sock) {
    try {
      sock.end(undefined);
    } catch {
      /* ignore */
    }
    sock = null;
  }
  connected = false;
  qrDataUrl = null;
  pairingCode = null;
  lastError = null;
  starting = true;

  try {
    const { state, saveCreds } = await useMultiFileAuthState(AUTH_DIR);
    const { version } = await fetchLatestBaileysVersion();
    sock = makeWASocket({
      version,
      auth: state,
      logger,
      printQRInTerminal: false,
      browser: Browsers.ubuntu('ODON Desktop'),
      markOnlineOnConnect: false,
      syncFullHistory: false,
    });

    sock.ev.on('creds.update', saveCreds);

    sock.ev.on('connection.update', async (update) => {
      try {
        if (update.qr) {
          qrDataUrl = await QRCode.toDataURL(update.qr, { margin: 1, width: 320 });
        }
        if (update.connection === 'open') {
          connected = true;
          qrDataUrl = null;
          pairingCode = null;
          me = sock.user;
          lastError = null;
          console.error(`[bridge] connected as ${me?.id || 'unknown'}`);
        }
        if (update.connection === 'close') {
          connected = false;
          const code = update.lastDisconnect?.error?.output?.statusCode;
          lastError = update.lastDisconnect?.error?.message || `connection closed (${code})`;
          if (code === DisconnectReason.loggedOut || code === 401) {
            fs.rmSync(AUTH_DIR, { recursive: true, force: true });
            fs.mkdirSync(AUTH_DIR, { recursive: true });
            requestedPhone = null;
            me = null;
            lastError = 'WhatsApp session logged out. Scan the QR again.';
          } else {
            scheduleReconnect();
          }
        }
      } catch (error) {
        lastError = error.message;
      }
    });

    sock.ev.on('messages.upsert', async ({ messages, type }) => {
      if (type !== 'notify') return;
      for (const message of messages) {
        try {
          if (!message.message || message.key.fromMe) continue;
          const from = message.key.remoteJid;
          if (!from || from === 'status@broadcast') continue;
          if (from.endsWith('@g.us') && !ALLOW_GROUPS) continue;
          const digits = from.split('@')[0].replace(/\D/g, '');
          if (ALLOWED.length > 0) {
            if (!ALLOWED.includes(digits)) continue;
          } else if (!ALLOW_ANYONE) {
            // Secure default: no allowlist and no explicit opt-in = reject.
            continue;
          }
          const { attachments, notes } = await extractAttachments(message);
          let text = extractText(message.message).trim();
          if (notes.length) {
            text = `${text}\n\n${notes.join('\n')}`.trim();
          }
          if (!text && attachments.length > 0) {
            const label = attachments.length === 1 ? 'media attachment' : `${attachments.length} media attachments`;
            text = `[WhatsApp message with ${label}]`;
          }
          if (!text && attachments.length === 0) continue;
          await notifyDaemon({
            from,
            text,
            attachments,
            id: message.key.id,
            push_name: message.pushName || null,
          });
        } catch (error) {
          console.error(`[bridge] message handling failed: ${error.message}`);
        }
      }
    });

    const registered = sock.authState?.creds?.registered;
    if (requestedPhone && !registered) {
      try {
        // Pairing codes are an alternative to scanning the QR with a phone.
        await sleep(600);
        pairingCode = await sock.requestPairingCode(requestedPhone);
        qrDataUrl = null;
        console.error(`[bridge] pairing code requested for ${requestedPhone}`);
      } catch (error) {
        lastError = `pairing code failed: ${error.message}`;
        console.error(`[bridge] ${lastError}`);
      }
    }
  } catch (error) {
    lastError = error.message;
    console.error(`[bridge] start failed: ${error.message}`);
    scheduleReconnect();
  } finally {
    starting = false;
  }
}

async function logout() {
  try {
    if (sock) await sock.logout();
  } catch {
    /* ignore */
  }
  try {
    sock?.end(undefined);
  } catch {
    /* ignore */
  }
  sock = null;
  connected = false;
  qrDataUrl = null;
  pairingCode = null;
  me = null;
  requestedPhone = null;
  fs.rmSync(AUTH_DIR, { recursive: true, force: true });
  fs.mkdirSync(AUTH_DIR, { recursive: true });
  return status();
}

async function sendText(to, text) {
  if (!connected || !sock) {
    const error = new Error('WhatsApp Web is not connected. Scan the QR or enter a pairing code first.');
    error.statusCode = 409;
    throw error;
  }
  const jid = normalizeJid(to);
  for (const chunk of splitText(text)) {
    await sock.sendMessage(jid, { text: chunk });
  }
}

function mediaKind(mime) {
  const value = String(mime || '').toLowerCase();
  if (value.startsWith('image/')) return 'image';
  if (value.startsWith('video/')) return 'video';
  if (value.startsWith('audio/')) return 'audio';
  return 'document';
}

async function sendMedia(to, fileName, mime, base64Data, caption) {
  if (!connected || !sock) {
    const error = new Error('WhatsApp Web is not connected. Scan the QR or enter a pairing code first.');
    error.statusCode = 409;
    throw error;
  }
  const buffer = Buffer.from(String(base64Data || ''), 'base64');
  if (!buffer.length) {
    const error = new Error('media data is empty');
    error.statusCode = 400;
    throw error;
  }
  const kind = mediaKind(mime);
  const content = {
    [kind]: buffer,
    fileName: String(fileName || 'odon-file'),
    mimetype: String(mime || 'application/octet-stream'),
  };
  if (caption) content.caption = String(caption).slice(0, 900);
  await sock.sendMessage(normalizeJid(to), content);
}

function authorized(req) {
  if (!BRIDGE_TOKEN) return true;
  return req.headers['x-bridge-token'] === BRIDGE_TOKEN;
}

function readJson(req) {
  return new Promise((resolve, reject) => {
    let body = '';
    req.on('data', (chunk) => {
      body += chunk;
      if (body.length > 34_000_000) {
        reject(new Error('payload too large'));
        req.destroy();
      }
    });
    req.on('end', () => {
      if (!body) return resolve({});
      try {
        resolve(JSON.parse(body));
      } catch {
        reject(new Error('invalid JSON'));
      }
    });
    req.on('error', reject);
  });
}

function json(res, code, payload) {
  res.writeHead(code, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' });
  res.end(JSON.stringify(payload));
}

const server = http.createServer(async (req, res) => {
  try {
    const url = new URL(req.url, `http://127.0.0.1:${PORT}`);
    if (url.pathname === '/health') return json(res, 200, { ok: true });
    if (!authorized(req)) return json(res, 401, { error: 'unauthorized' });

    if (req.method === 'GET' && url.pathname === '/status') {
      return json(res, 200, status());
    }
    if (req.method === 'POST' && url.pathname === '/start') {
      const body = await readJson(req);
      if (body.phone && !connected) requestedPhone = String(body.phone).replace(/\D/g, '');
      // Never tear down an established session. If the socket is already
      // starting, the auto-start (with WA_PAIRING_PHONE if provided) owns it.
      if (connected) return json(res, 200, status());
      if (starting) return json(res, 200, status());
      await startSocket(requestedPhone);
      return json(res, 200, status());
    }
    if (req.method === 'POST' && url.pathname === '/logout') {
      return json(res, 200, await logout());
    }
    if (req.method === 'POST' && url.pathname === '/send') {
      const body = await readJson(req);
      if (!body.to || !body.text) return json(res, 400, { error: 'to and text are required' });
      await sendText(body.to, body.text);
      return json(res, 200, { sent: true });
    }
    if (req.method === 'POST' && url.pathname === '/send-media') {
      const body = await readJson(req);
      if (!body.to || !body.data) return json(res, 400, { error: 'to and data are required' });
      await sendMedia(body.to, body.file_name, body.mime, body.data, body.caption);
      return json(res, 200, { sent: true });
    }
    return json(res, 404, { error: 'not found' });
  } catch (error) {
    const code = error.statusCode || 500;
    return json(res, code, { error: error.message || String(error) });
  }
});

server.listen(PORT, '127.0.0.1', () => {
  console.error(`[bridge] listening on http://127.0.0.1:${PORT} (auth dir: ${AUTH_DIR})`);
  if (process.env.WA_AUTO_START !== '0') {
    startSocket(requestedPhone).catch((error) => {
      lastError = error.message;
      console.error(`[bridge] auto-start failed: ${error.message}`);
    });
  }
});

function shutdown() {
  try {
    sock?.end(undefined);
  } catch {
    /* ignore */
  }
  server.close(() => process.exit(0));
  setTimeout(() => process.exit(0), 1500);
}
process.on('SIGINT', shutdown);
process.on('SIGTERM', shutdown);
