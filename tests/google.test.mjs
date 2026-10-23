import test from 'node:test';
import assert from 'node:assert/strict';
import { createGoogleTools, GOOGLE_TOOLS, GOOGLE_WRITES } from '../dist/workers/pi/src/google.js';
import { ToolRegistry } from '../dist/workers/pi/src/registry.js';
import { ManagedToolsExtension } from '../dist/workers/pi/src/tools-extension.js';
import { McpManager } from '../dist/workers/pi/src/mcp.js';
import { WebTransport } from '../dist/workers/pi/src/web.js';

function stubTransport(handler) {
  const calls = [];
  return {
    calls,
    request(method, params, signal, timeout) {
      calls.push({ method, params, timeout });
      return Promise.resolve(handler({ method, params, signal, timeout, call: calls.length }))
        .then(value => {
          signal?.throwIfAborted();
          return value;
        });
    },
  };
}

function textResult(value) {
  return { content: [{ type: 'text', text: typeof value === 'string' ? value : JSON.stringify(value) }] };
}

function hostChannel() {
  const sent = [];
  const transport = new WebTransport(value => sent.push(value), 'Google');
  const calls = () => sent.map(value => value.message ?? value);
  const lastRequest = () => calls().filter(message => message?.method === 'google/call').at(-1);
  const reply = (id, payload) => transport.receive({ id, ...payload });
  const cancellations = () => calls().filter(message => message?.method === 'notifications/cancelled');
  return { transport, sent, calls, lastRequest, reply, cancellations };
}

test('every Google tool forwards one exact operation owned by its conversation', async () => {
  const transport = stubTransport(({ params }) => textResult({ tool: params.tool }));
  const tools = createGoogleTools(transport, 'conversation-1', () => 'generation-1');
  assert.deepEqual(tools.map(tool => tool.name), [...GOOGLE_TOOLS]);

  for (const tool of tools) {
    const result = await tool.execute('call', {}, undefined);
    assert.match(result.content[0].text, /Untrusted Google Workspace data/);
  }
  assert.equal(transport.calls.length, GOOGLE_TOOLS.length);
  for (const call of transport.calls) {
    assert.equal(call.method, 'google/call');
    assert.equal(call.params.conversationId, 'conversation-1');
    assert.equal(call.params.generationId, 'generation-1');
    assert.ok(GOOGLE_TOOLS.includes(call.params.tool));
    assert.equal(
      call.timeout,
      GOOGLE_WRITES.has(call.params.tool) ? 145_000 : 70_000,
      'writes wait for an approval, reads do not',
    );
  }
});

test('a failed mutation is never retried automatically', async () => {
  const channel = hostChannel();
  const [send] = createGoogleTools(channel.transport, 'conversation-1', () => 'generation-1')
    .filter(tool => tool.name === 'google_gmail_send');
  const pending = send.execute('call', { to: 'alex@example.com', subject: 'Report', body: 'Body' }, undefined);
  const request = channel.lastRequest();
  channel.reply(request.id, { error: { message: 'Talo could not confirm whether Google applied this change.' } });
  await assert.rejects(pending, /could not confirm/);
  assert.equal(channel.calls().filter(message => message.method === 'google/call').length, 1);
});

test('cancelling a Google call tells the host which request to stop', async () => {
  const channel = hostChannel();
  const read = createGoogleTools(channel.transport, 'conversation-1', () => 'generation-1')
    .find(tool => tool.name === 'google_gmail_read');

  const controller = new AbortController();
  const pending = read.execute('cancel', { messageId: 'abc_123' }, controller.signal);
  const request = channel.lastRequest();
  assert.ok(request.id, 'the call carries the id the host must cancel');
  controller.abort();
  await assert.rejects(pending, /Google cancelled/);
  const cancellation = channel.cancellations().at(-1);
  assert.equal(cancellation.params.requestId, request.id);
  assert.equal(cancellation.params.reason, 'Google cancelled');

  channel.reply(request.id, { result: textResult({ id: 'abc_123' }) });

  const aborted = new AbortController();
  aborted.abort();
  await assert.rejects(read.execute('pre', { messageId: 'abc_123' }, aborted.signal));
  assert.equal(
    channel.calls().filter(message => message.method === 'google/call').length,
    1,
    'an already-cancelled turn never reaches the host',
  );
});

test('a timed-out write reports an uncertain outcome while a read stays retryable', async () => {
  const channel = hostChannel();
  const [send] = createGoogleTools(channel.transport, 'conversation-1', () => 'generation-1', { write: 20 })
    .filter(tool => tool.name === 'google_gmail_send');
  await assert.rejects(
    send.execute('call', { to: 'alex@example.com', subject: 'Report', body: 'Body' }, undefined),
    error => {
      assert.match(error.message, /could not confirm whether Google applied this change/);
      assert.doesNotMatch(error.message, /Retry or narrow/);
      return true;
    },
  );
  assert.equal(
    channel.cancellations().at(-1).params.requestId,
    channel.lastRequest().id,
    'a timed-out write also stops the host from waiting on it',
  );

  const [list] = createGoogleTools(channel.transport, 'conversation-1', () => 'generation-1', { read: 20 })
    .filter(tool => tool.name === 'google_gmail_list');
  await assert.rejects(list.execute('call', {}, undefined), /Retry or narrow the request/);
});

test('oversized Google results are refused instead of flooding the chat', async () => {
  const transport = stubTransport(() => textResult('x'.repeat(20 * 1024)));
  const [list] = createGoogleTools(transport, 'conversation-1');
  await assert.rejects(list.execute('call', {}, undefined), /more than Talo can show/);

  const empty = stubTransport(() => ({ content: [] }));
  const [again] = createGoogleTools(empty, 'conversation-1');
  await assert.rejects(again.execute('call', {}, undefined), /returned no data/);
});

test('the registry keeps Google tools on the Integrations budget and respects workspace capabilities', async () => {
  const checks = [];
  let googleEnabled = true;
  const authorize = async (action, scope) => { checks.push({ action, scope }); return true; };
  const registry = new ToolRegistry(authorize);
  const transport = stubTransport(() => textResult({ ok: true }));
  const tools = registry.wrap(
    createGoogleTools(transport, 'conversation-1'),
    tool => googleEnabled && tool.startsWith('google_'),
    authorize,
  );
  const catalog = registry.list();
  assert.equal(catalog.length, GOOGLE_TOOLS.length);
  for (const tool of catalog) {
    assert.equal(tool.category, 'Integrations');
    assert.equal(tool.timeoutMs, 150_000);
    assert.equal(tool.maxResultBytes, 16_384);
    assert.deepEqual(tool.permissions, ['google.read', 'google.write']);
  }

  const list = tools.find(tool => tool.name === 'google_gmail_list');
  await list.execute('call', {}, undefined);
  assert.deepEqual(checks.at(-1), { action: 'tool.use', scope: 'google_gmail_list' });

  googleEnabled = false;
  await assert.rejects(list.execute('call', {}, undefined), /does not include that capability/);
  assert.equal(transport.calls.length, 1);
});

test('managed discovery exposes Google schemas without runnable calls', async () => {
  let executions = 0;
  const external = new McpManager(async () => true);
  const transport = stubTransport(() => { executions += 1; return textResult({ ok: true }); });
  const googleTools = createGoogleTools(transport, 'conversation-1');
  const extension = new ManagedToolsExtension([
    { id: 'talo-google', category: 'Integrations', description: 'Google', tools: googleTools, timeoutMs: 145_000, maxResultBytes: 16_384, resources: ['Google Workspace'] },
  ], external, async () => true);
  try {
    const servers = JSON.parse((await extension.tool.execute('list', { operation: 'list' }, undefined)).content[0].text);
    assert.ok(servers.some(server => server.id === 'talo-google'));
    const discovered = JSON.parse((await extension.tool.execute('discover', { operation: 'discover', server: 'talo-google' }, undefined)).content[0].text);
    assert.equal(discovered.total, GOOGLE_TOOLS.length);
    assert.equal(discovered.tools[0].inputSchema, undefined, 'discovery lists names before schemas');
    const schema = JSON.parse((await extension.tool.execute('schema', { operation: 'discover', server: 'talo-google', tool: 'google_gmail_send' }, undefined)).content[0].text);
    assert.equal(schema.tools[0].name, 'google_gmail_send');
    assert.equal(schema.tools[0].inputSchema.type, 'object');
    assert.equal(executions, 0);
  } finally {
    await extension.close();
    await external.close();
  }
});
