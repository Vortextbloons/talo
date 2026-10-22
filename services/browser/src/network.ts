import http from 'node:http';
import https from 'node:https';
import net from 'node:net';
import { lookup } from 'node:dns/promises';
import { publicAddress } from '../../../workers/pi/src/web.js';

/** Resolve once, validate every answer, then connect to the validated IP. */
export async function publicTarget(host: string, port: number) {
  if (![80, 443].includes(port) || /(^|\.)(localhost|local|internal|home|lan)$/iu.test(host)) throw new Error('Private destination blocked');
  const rows = await lookup(host, { all: true });
  if (!rows.length || rows.some(row => !publicAddress(row.address))) throw new Error('Private destination blocked');
  return rows[0].address;
}
export async function downloadPublic(value: string, signal: AbortSignal, cookie?: string) {
  const url = new URL(value);
  if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) throw new Error('Invalid download URL');
  const address = await publicTarget(url.hostname, Number(url.port || (url.protocol === 'https:' ? 443 : 80)));
  signal.throwIfAborted();
  return new Promise<Buffer>((resolve, reject) => {
    const request = (url.protocol === 'https:' ? https : http).get(url, {
      signal, timeout: 30_000, headers: cookie ? { cookie } : {},
      lookup: (_host, _options, callback) => callback(null, address, net.isIP(address)),
    }, response => {
      if (!response.statusCode || response.statusCode < 200 || response.statusCode >= 300) { response.destroy(); reject(new Error('Download failed. Redirects need a separately approved URL.')); return; }
      const chunks: Buffer[] = []; let size = 0;
      response.on('data', chunk => { size += chunk.length; if (size > 25 * 1024 * 1024) { response.destroy(new Error('Download exceeds 25 MiB.')); return; } chunks.push(chunk); });
      response.on('end', () => resolve(Buffer.concat(chunks))); response.on('error', reject);
    });
    request.on('timeout', () => request.destroy(new Error('Download timed out.'))); request.on('error', reject);
  });
}
export async function startEgressProxy() {
  const sockets = new Set<net.Socket>();
  const server = http.createServer((req, res) => {
    void (async () => {
      const url = new URL(req.url!);
      if (url.protocol !== 'http:' || url.username || url.password) throw new Error('Invalid destination');
      const address = await publicTarget(url.hostname, Number(url.port || 80));
      const headers = { ...req.headers }; delete headers['proxy-authorization']; delete headers['proxy-connection'];
      const upstream = http.request({ host: address, port: Number(url.port || 80), path: url.pathname + url.search, method: req.method, headers, timeout: 30_000 }, reply => {
        res.writeHead(reply.statusCode ?? 502, reply.headers); reply.pipe(res);
      });
      upstream.on('timeout', () => upstream.destroy());
      upstream.on('error', () => { if (!res.headersSent) res.writeHead(502); res.end(); });
      req.pipe(upstream);
    })().catch(() => { res.writeHead(403); res.end('Talo blocked this destination.'); });
  });
  server.on('connect', (req, client, head) => {
    void (async () => {
      const url = new URL(`https://${req.url}`);
      const port = Number(url.port || 443);
      const address = await publicTarget(url.hostname.replace(/^\[|\]$/gu, ''), port);
      if (client.destroyed) return;
      const upstream = net.connect(port, address);
      sockets.add(upstream); upstream.on('close', () => sockets.delete(upstream));
      upstream.on('connect', () => { client.write('HTTP/1.1 200 Connection Established\r\n\r\n'); upstream.write(head); client.pipe(upstream); upstream.pipe(client); });
      upstream.on('error', () => client.destroy()); client.on('error', () => upstream.destroy()); client.on('close', () => upstream.destroy());
    })().catch(() => client.end('HTTP/1.1 403 Forbidden\r\n\r\n'));
  });
  server.on('connection', socket => { sockets.add(socket); socket.on('close', () => sockets.delete(socket)); socket.on('error', () => {}); });
  await new Promise<void>((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
  const port = (server.address() as net.AddressInfo).port;
  return { url: `http://127.0.0.1:${port}`, close: () => { for (const socket of sockets) socket.destroy(); server.close(); } };
}
