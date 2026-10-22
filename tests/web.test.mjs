import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm, readdir } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { WebTransport, createWebTools, boundWebResult, validateWebUrl, publicAddress, WEB_TOOLS } from '../dist/workers/pi/src/web.js';
import { loadMcpAdapter } from '../dist/workers/pi/src/mcp.js';
import { startWorker } from './helpers/worker.mjs';
import { startMockProvider } from './helpers/mock-provider.mjs';

test('web URL policy blocks private, encoded, mapped and credential-bearing destinations', async () => {
  for (const url of ['file:///etc/passwd', 'http://localhost', 'http://127.1', 'http://2130706433', 'http://0x7f000001', 'http://10.0.0.1', 'http://169.254.169.254', 'http://[::1]', 'http://[::ffff:127.0.0.1]', 'http://[fc00::1]', 'https://user:pass@example.com', 'http://example.com:8080', 'http://printer.local']) {
    await assert.rejects(validateWebUrl(url), undefined, url);
  }
  assert.equal(publicAddress('8.8.8.8'), true);
  assert.equal(publicAddress('2606:4700:4700::1111'), true);
  assert.equal(publicAddress('100.64.1.1'), false);
});

test('web transport correlates concurrent replies and propagates cancellation and deadlines', async () => {
  const messages = [];
  const transport = new WebTransport(value => messages.push(value));
  const first = transport.request('tools/call', { name: 'web_search' });
  const second = transport.request('tools/call', { name: 'web_fetch' });
  transport.receive({ id: messages[1].message.id, result: 'second' });
  transport.receive({ id: messages[0].message.id, result: 'first' });
  assert.deepEqual(await Promise.all([first, second]), ['first','second']);
  const controller = new AbortController();
  const pending = transport.request('tools/call', {}, controller.signal);
  controller.abort();
  await assert.rejects(pending, /cancelled/);
  assert.equal(messages.at(-1).message.method, 'notifications/cancelled');
  await assert.rejects(transport.request('tools/call', {}, undefined, 10), /timed out/);
});

test('large web evidence stays bounded with real source URLs and a complete local artifact', async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'talo-evidence-'));
  try {
    const raw = { content: [{ type: 'text', text: 'Evidence '.repeat(6000) + ' https://example.com/source' }] };
    const result = await boundWebResult(raw, directory);
    assert.equal(result.text.length, 10000);
    assert.equal(result.truncated, true);
    assert.ok(result.sources.includes('https://example.com/source'));
    assert.deepEqual(JSON.parse(await readFile(path.join(directory, result.artifact), 'utf8')), raw);
    const screenshot = await boundWebResult({ content: [{ type: 'image', mimeType: 'image/png', data: Buffer.from('PNG fixture').toString('base64') }] }, directory, 'https://example.com/');
    assert.equal(await readFile(path.join(directory, screenshot.artifact), 'utf8'), 'PNG fixture');
    assert.ok(!JSON.stringify(screenshot).includes('base64'));
    const blocked = await boundWebResult({isError:false,content:[{type:'text',text:'[meta] {"ok":false,"read_status":"error","code":"guard.ssrf"}'},{type:'text',text:'Private redirect blocked'}]}, directory);
    assert.equal(blocked.isError, true, 'DonSeTch text-mode guard errors must not appear as successful evidence');
  } finally { await rm(directory, { recursive: true, force: true }); }
});

test('real Pi invokes native web tool and consumes a bounded host MCP reply', {timeout:40000}, async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'talo-pi-web-'));
  const provider = await startMockProvider({mode:'web-tool',chunks:['Read the source.']});
  const worker = startWorker(directory);
  try {
    await worker.waitReady();
    await worker.request('open-web','session.open',{conversationId:'web-chat',provider:'custom',model:'mock-model',baseUrl:provider.url,apiKey:provider.state.key});
    await worker.request('prompt-web','session.prompt',{conversationId:'web-chat',generationId:'web-generation',text:'Find Rust documentation'});
    const call = await worker.wait(value => value.type === 'web');
    assert.equal(call.message.method, 'tools/call');
    assert.equal(call.message.params.name, 'web_search');
    worker.send({type:'web_response',message:{jsonrpc:'2.0',id:call.message.id,result:{content:[{type:'text',text:'Rust docs: https://doc.rust-lang.org/'}]}}});
    const done = await worker.waitGeneration('web-chat','web-generation');
    assert.equal(done.event.state, 'completed', done.event.error);
    const toolResult = provider.state.requests.flatMap(value => value.body.messages ?? []).find(message => message.role === 'tool');
    assert.ok(toolResult, 'tool evidence must reach Pi');
    assert.match(JSON.stringify(toolResult), /https:\/\/doc.rust-lang.org/);
    assert.match(JSON.stringify(toolResult), /Untrusted web evidence/);
    const completed = await worker.wait(value => value.type === 'chat' && value.event?.type === 'research' && value.event.activity.state === 'completed');
    assert.equal(completed.event.activity.sources[0], 'https://doc.rust-lang.org/');
    const tools = provider.state.requests.find(value => value.body.tools)?.body.tools;
    assert.equal(new Set(tools.map(tool => tool.function.name)).size, 4);
    await worker.request('open-cancel-web','session.open',{conversationId:'web-cancel',provider:'custom',model:'mock-model',baseUrl:provider.url,apiKey:provider.state.key});
    const mark = worker.mark();
    await worker.request('prompt-cancel-web','session.prompt',{conversationId:'web-cancel',generationId:'cancel-web-generation',text:'Search now'});
    const active = await worker.waitFrom(mark, value => value.type === 'web' && value.message.method === 'tools/call');
    await worker.request('abort-web','session.abort',{generationId:'cancel-web-generation'});
    const cancelled = await worker.waitFrom(mark, value => value.type === 'web' && value.message.method === 'notifications/cancelled');
    assert.equal(cancelled.message.params.requestId, active.message.id);
    assert.equal((await worker.waitGeneration('web-cancel','cancel-web-generation')).event.state, 'cancelled');
  } finally {
    await worker.kill(); await provider.close(); await rm(directory, {recursive:true,force:true});
  }
});

test('four native tools are lazy, keep budgets, expose structured evidence and reject duplicate MCP', async () => {
  const messages = [];
  const activity = [];
  const directory = await mkdtemp(path.join(os.tmpdir(), 'talo-tools-'));
  const transport = new WebTransport(value => {
    messages.push(value);
    queueMicrotask(() => transport.receive({ id: value.message.id, result: { content: [{type:'text',text:'Source https://example.com/'}] } }));
  });
  try {
    const tools = createWebTools(transport, directory, event => activity.push(event));
    assert.deepEqual(tools.map(tool => tool.name), WEB_TOOLS);
    assert.equal(messages.length, 0, 'registration must not start research');
    const result = await tools[0].execute('search-1', { query: 'Talo research' });
    assert.equal(messages[0].message.params.arguments.max_results, 6);
    assert.equal(messages[0].message.params.arguments.deadline_ms, 45000);
    assert.equal(activity.at(-1).state, 'completed');
    assert.match(result.content[0].text, /Untrusted web evidence/);
    await assert.rejects(tools[1].execute('fetch-private', {url:'http://127.0.0.1'}), /blocked/);
    assert.equal(activity.at(-1).state, 'failed');
    assert.equal((await readdir(directory)).length, 0);
    await assert.rejects(loadMcpAdapter({request:async()=>true}, 'donsetch'), /already registered/);
  } finally { await rm(directory, {recursive:true,force:true}); }
});
