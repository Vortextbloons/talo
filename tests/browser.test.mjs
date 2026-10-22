import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm, access, readFile } from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import http from 'node:http';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { startWorker } from './helpers/worker.mjs';
import { BrowserManager } from '../dist/services/browser/src/manager.js';
import { publicTarget, startEgressProxy } from '../dist/services/browser/src/network.js';
import { BROWSER_TOOLS, createBrowserTools } from '../dist/workers/pi/src/browser.js';
import { WebTransport } from '../dist/workers/pi/src/web.js';

const root = path.resolve('vendor');
const fixture = '<title>Browser fixture</title><h1>Browser fixture</h1><label>Note<input aria-label="Note"></label><button onclick="document.querySelector(\'h1\').textContent=\'Clicked\'">Change heading</button>';
const target = async manager => { const s = await manager.state(); const p = s.tabs.find(p => p.id === s.activeTab); return { tabId: p.id, revision: p.revision }; };

test('eight browser tools are separate from web research and cancellation reaches the host', async () => {
  const frames = []; const events = [];
  const transport = new WebTransport(value => frames.push(value));
  const tools = createBrowserTools(transport, () => ({ provider: 'custom', model: 'fixture' }), event => events.push(event));
  assert.deepEqual(tools.map(t => t.name), [...BROWSER_TOOLS]);
  const abort = new AbortController();
  const promise = tools.find(t => t.name === 'browser_snapshot').execute('test', {}, abort.signal);
  abort.abort(); await assert.rejects(promise, /cancelled/iu);
  assert.equal(frames.at(-1).message.method, 'notifications/cancelled');
  assert.equal(events.at(-1).state, 'cancelled');
});

test('network proxy blocks loopback and custom ports before opening a socket', async () => {
  await assert.rejects(publicTarget('127.0.0.1', 80), /blocked/iu);
  await assert.rejects(publicTarget('::1', 443), /blocked/iu);
  await assert.rejects(publicTarget('example.com', 9222), /blocked/iu);
  const proxy = await startEgressProxy();
  try {
    const status = await new Promise((resolve, reject) => {
      const req = http.get(proxy.url, { path: 'http://127.0.0.1/' }, res => { res.resume(); resolve(res.statusCode); }); req.on('error', reject);
    });
    assert.equal(status, 403);
  } finally { proxy.close(); }
});

test('bundled sandboxed Chromium: profile, ownership, deterministic actions, tabs, cancellation and restart', { timeout: 60000 }, async () => {
  const data = await mkdtemp(path.join(os.tmpdir(), 'talo-browser-'));
  const states = [];
  const manager = new BrowserManager(root, data, state => states.push(state));
  await manager.acquire();
  try {
    const duplicate = new BrowserManager(root, data, () => {}, async () => {});
    await assert.rejects(duplicate.acquire(), /already owns/iu);
    await manager.start();
    await access(path.join(data, 'browser/profiles/primary/Default'));
    assert.equal((await manager.state()).state, 'connected');
    assert.equal((await manager.state()).owner, 'agent');
    await assert.rejects(manager.execute({ name: 'browser_interact', arguments: { action: 'click', selector: 'button' } }), /approval/iu);
    const page = manager.context.pages()[0]; await page.setContent(fixture);
    const snap = await manager.execute({ name: 'browser_snapshot' }); assert.match(snap.snapshot, /Browser fixture/u);
    await manager.execute({ name: 'browser_interact', approved: true, target: await target(manager), arguments: { action: 'type', selector: 'input', text: 'Remember me' } });
    assert.equal(await page.locator('input').inputValue(), 'Remember me');
    await manager.execute({ name: 'browser_interact', approved: true, target: await target(manager), arguments: { action: 'click', selector: 'button' } });
    assert.equal(await page.locator('h1').textContent(), 'Clicked');
    await assert.rejects(manager.execute({ name: 'browser_interact', approved: true, target: { tabId: 'stale', revision: 'stale' }, arguments: { action: 'click', selector: 'button' } }), /page changed/iu);
    const shot = await manager.execute({ name: 'browser_screenshot' }); assert.equal((await readFile(path.join(data, 'artifacts/browser', shot.artifact))).subarray(1,4).toString(), 'PNG');
    await manager.context.addCookies([{ name: 'talo-session-test', value: 'persistent', domain: 'example.com', path: '/', expires: Math.floor(Date.now()/1000)+86400, secure: true }]);
    await manager.handoff('human'); assert.equal((await manager.state()).owner, 'human');
    await assert.rejects(manager.execute({ name: 'browser_snapshot' }), /paused/iu);
    await assert.rejects(manager.execute({ name: 'browser_handoff', arguments: { action: 'return' } }), /Return Control/u);
    await manager.handoff('agent'); assert.equal((await manager.state()).owner, 'agent');
    const tabs = await manager.execute({ name: 'browser_tabs', arguments: { action: 'create' } }); assert.equal(tabs.tabs.length, 2);
    const controller = new AbortController();
    const running = manager.execute({ name: 'browser_interact', approved: true, target: await target(manager), arguments: { action: 'click', selector: '#does-not-exist' } }, controller.signal);
    await new Promise(resolve => setTimeout(resolve, 100)); controller.abort();
    await assert.rejects(running, /cancelled|timed out/iu);
    assert.equal((await manager.state()).tabs.length, 1);
    await manager.stop(); await manager.handoff('agent');
    assert.equal((await manager.context.cookies('https://example.com')).find(c => c.name === 'talo-session-test')?.value, 'persistent');
    const cdp = await manager.context.browser().newBrowserCDPSession();
    await cdp.send('Browser.close').catch(() => {});
    await new Promise(resolve => setTimeout(resolve, 150));
    assert.equal((await manager.state()).state, 'crashed');
    await manager.start(); assert.equal((await manager.state()).state, 'connected');
    await assert.rejects(manager.execute({ name: 'browser_open', arguments: { url: 'file:///C:/Windows/win.ini' } }), /public HTTP/iu);
    await assert.rejects(manager.execute({ name: 'browser_download', approved: true, arguments: { url: 'http://127.0.0.1/' } }), /blocked/iu);
  } finally { await manager.dispose(); await rm(data, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 }); }
});

test('local Stagehand v4 extracts using a configured compatible provider, without Browserbase', { timeout: 60000 }, async () => {
  const data = await mkdtemp(path.join(os.tmpdir(), 'talo-stagehand-'));
  let requests = 0;
  const server = http.createServer((req, res) => {
    let raw = ''; req.on('data', chunk => raw += chunk); req.on('end', () => {
      const input = JSON.parse(raw); requests++;
      assert.equal(req.headers.authorization, 'Bearer fixture-private-key');
      assert.equal(input.model, 'fixture-model');
      const sample = schema => {
        if (schema.const !== undefined) return schema.const;
        if (schema.enum) return schema.enum[0];
        if (schema.anyOf || schema.oneOf) return sample((schema.anyOf ?? schema.oneOf)[0]);
        if (schema.type === 'object' || schema.properties) return Object.fromEntries(Object.entries(schema.properties ?? {}).map(([key, value]) => [key, key === 'heading' ? 'Browser fixture' : sample(value)]));
        if (schema.type === 'boolean') return true;
        if (schema.type === 'array') return [];
        if (schema.type === 'number' || schema.type === 'integer') return 1;
        return 'done';
      };
      const content = JSON.stringify(sample(input.response_format.json_schema.schema));
      res.setHeader('content-type', 'application/json'); res.end(JSON.stringify({ choices: [{ message: { role: 'assistant', content } }], usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 } }));
    });
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const manager = new BrowserManager(root, data, () => {}, async () => {});
  await manager.acquire();
  try {
    await manager.start(); const page = manager.context.pages()[0]; await page.setContent(fixture);
    const result = await manager.execute({ name: 'browser_ai', approved: true, target: await target(manager), arguments: { action: 'extract', instruction: 'Extract the heading', schema: { type: 'object', properties: { heading: { type: 'string' } }, required: ['heading'], additionalProperties: false } }, model: { provider: 'custom', model: 'fixture-model', apiKey: 'fixture-private-key', baseUrl: `http://127.0.0.1:${server.address().port}/v1` } });
    assert.ok(requests > 0); assert.equal(result.data.heading, 'Browser fixture');
  } finally { await manager.dispose(); server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); await rm(data, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 }); }
});


test('shipped headless service survives a Pi hibernation cycle', { timeout: 60000 }, async () => {
  const data = await mkdtemp(path.join(os.tmpdir(), 'talo-service-'));
  const child = spawn(path.join(root, 'runtime/node/node.exe'), [path.join(root, 'bundle/services/browser/src/main.js')], {
    env: { ...process.env, NODE_OPTIONS: '', NODE_PATH: '', TALO_DATA_DIR: data, TALO_VENDOR_DIR: root }, windowsHide: true,
  });
  child.stderr.resume();
  const exited = new Promise(resolve => child.once('exit', resolve));
  const pending = new Map();
  createInterface({ input: child.stdout }).on('line', line => {
    const value = JSON.parse(line); const call = pending.get(value.id);
    if (call) { pending.delete(value.id); value.error ? call.reject(new Error(value.error.message)) : call.resolve(value.result); }
  });
  let sequence = 0;
  const request = (method, params) => new Promise((resolve, reject) => {
    const id = String(++sequence); const timer = setTimeout(() => { pending.delete(id); reject(new Error('Browser service reply deadline')); }, 30000);
    pending.set(id, { resolve: value => { clearTimeout(timer); resolve(value); }, reject: error => { clearTimeout(timer); reject(error); } });
    child.stdin.write(JSON.stringify({ id, method, params }) + '\n');
  });
  let pi;
  try {
    let state = await request('control', { action: 'open' }); assert.equal(state.owner, 'agent'); assert.equal(state.state, 'connected');
    state = await request('control', { action: 'return' }); assert.equal(state.owner, 'agent');
    assert.ok((await request('tools/call', { name: 'browser_snapshot', arguments: {} })).snapshot !== undefined);
    pi = startWorker(data, { idleMs: 200 }); await pi.waitReady(); await pi.exited;
    state = await request('state'); assert.equal(state.state, 'connected'); assert.equal(child.exitCode, null);
    pi = startWorker(data, { idleMs: 120000 }); await pi.waitReady(); await pi.request('browser-worker-wake', 'ping'); await pi.close();
    state = await request('control', { action: 'take_over' }); assert.equal(state.owner, 'human');
    await request('control', { action: 'stop' }); assert.equal((await request('state')).state, 'stopped');
  } finally {
    await pi?.close(); child.stdin.end(JSON.stringify({ method: 'shutdown' }) + '\n');
    const timer = setTimeout(() => child.kill(), 5000); await exited; clearTimeout(timer);
    await rm(data, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  }
});
