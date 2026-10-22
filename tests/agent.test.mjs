import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { mkdtemp, rm } from 'node:fs/promises';
import os from 'node:os';
import { Type } from 'typebox';
import { startWorker } from './helpers/worker.mjs';
import { startMockProvider } from './helpers/mock-provider.mjs';
import { createAgentTools } from '../dist/workers/pi/src/agent.js';
import { WebTransport } from '../dist/workers/pi/src/web.js';
import { ToolRegistry } from '../dist/workers/pi/src/registry.js';

test('the agent tool forwards its conversation and renders the host answer', async () => {
  const frames = [];
  const transport = new WebTransport(frame => frames.push(frame), 'Talo');
  const [tool] = createAgentTools(transport, 'chat-1');
  assert.equal(tool.name, 'agent');
  assert.ok(tool.parameters.properties.operation, 'operation stays discoverable');
  assert.equal(tool.parameters.properties.ask, undefined, 'unknown fields are refused by the schema');

  const pending = tool.execute('t1', {
    operation: 'ask',
    header: 'Which folder?',
    questions: [{ id: 'q1', question: 'Where?', options: [{ id: 'a', label: 'Docs' }, { id: 'b', label: 'Desktop' }] }],
  }, undefined);
  assert.equal(frames.length, 1);
  const message = frames[0].message;
  assert.equal(message.method, 'agent.call');
  assert.equal(message.params.operation, 'ask');
  assert.equal(message.params.conversationId, 'chat-1');

  transport.receive({ jsonrpc: '2.0', id: message.id, result: { askId: 'ask-1', expired: false, answers: [{ questionId: 'q1', optionId: 'b', decided: false }] } });
  const result = await pending;
  assert.match(result.content[0].text, /"optionId":"b"/);
});

test('a workspace bundle can withhold a capability from its own tools', async () => {
  let allowed = true;
  const registry = new ToolRegistry(async () => allowed);
  const tool = {
    name: 'browser_open', label: 'Open', description: 'Open a page',
    parameters: Type.Object({ url: Type.String() }),
    execute: async () => ({ content: [{ type: 'text', text: 'opened' }], details: {} }),
  };
  const [wrapped] = registry.wrap([tool], () => false);
  await assert.rejects(wrapped.execute('x', { url: 'https://example.com' }, undefined), /capability/);
  const [open] = registry.wrap([tool]);
  assert.match((await open.execute('y', { url: 'https://example.com' }, undefined)).content[0].text, /opened/);
});

test('a tool call reaches the host and the turn continues after the host replies', { timeout: 45000 }, async () => {
  const data = await mkdtemp(path.join(os.tmpdir(), 'talo-agent-'));
  const provider = await startMockProvider({ mode: 'agent-tool' });
  const worker = startWorker(data, { idleMs: 2000 });
  try {
    await worker.waitReady();
    const selection = { conversationId: 'agent-chat', provider: 'custom', model: 'mock-model', apiKey: provider.state.key, baseUrl: provider.url };
    await worker.request('open', 'session.open', { ...selection, sessionPath: null });
    await worker.request('prompt', 'session.prompt', {
      conversationId: 'agent-chat', generationId: 'gen-1',
      text: 'Plan my afternoon', capabilities: ['web'], context: '',
    });
    const call = await worker.wait(event => event.type === 'agent' && event.message?.method === 'agent.call');
    const first = provider.state.requests.find(request => request.body.tools?.length);
    assert.deepEqual(first.body.tools.map(tool => tool.function.name), ['tools'], 'a workspace without files hides read/write/edit');
    assert.equal(call.message.params.operation, 'task_assign');
    assert.equal(call.message.params.conversationId, 'agent-chat');
    worker.send({ type: 'agent_response', message: { jsonrpc: '2.0', id: call.message.id, result: { id: 'assignment-1', status: 'open' } } });
    const done = await worker.waitGeneration('agent-chat', 'gen-1');
    assert.equal(done.event.state, 'completed');
    assert.ok(provider.state.requests.at(-1).body.messages.some(message => message.role === 'tool' && message.content.includes('assignment-1')));
  } finally {
    await worker.kill();
    await provider.close();
    await rm(data, { recursive: true, force: true });
  }
});
