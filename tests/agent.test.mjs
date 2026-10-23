import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { mkdtemp, rm } from 'node:fs/promises';
import os from 'node:os';
import { Type } from 'typebox';
import { startWorker } from './helpers/worker.mjs';
import { startMockProvider } from './helpers/mock-provider.mjs';
import { createAgentTools } from '../dist/workers/pi/src/agent.js';
import { createDelegationTools } from '../dist/workers/pi/src/delegation.js';
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

test('the delegation tool forwards a stable start request and returns its task id', async () => {
  const frames = [];
  const transport = new WebTransport(frame => frames.push(frame), 'Talo');
  const [tool] = createDelegationTools(transport, 'parent-1');
  assert.equal(tool.name, 'delegate');
  const pending = tool.execute('d1', {
    operation: 'start',
    title: 'Read the RFC',
    brief: 'Summarize section 4 only.',
  }, undefined);
  assert.equal(frames[0].message.method, 'delegation.call');
  assert.equal(frames[0].message.params.conversationId, 'parent-1');
  assert.equal(frames[0].message.params.operation, 'start');
  transport.receive({
    jsonrpc: '2.0',
    id: frames[0].message.id,
    result: { id: 'task-1', title: 'Read the RFC', state: 'queued' },
  });
  const result = await pending;
  assert.match(result.content[0].text, /"id":"task-1"/);
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
    assert.equal(call.message.params.operation, 'schedule_save');
    assert.equal(call.message.params.recurrence, 'daily');
    assert.equal(call.message.params.timezone, 'America/Denver');
    assert.equal(call.message.params.conversationId, 'agent-chat');
    worker.send({ type: 'agent_response', message: { jsonrpc: '2.0', id: call.message.id, result: { id: 'schedule-1', nextAt: 4070908800 } } });
    const done = await worker.waitGeneration('agent-chat', 'gen-1');
    assert.equal(done.event.state, 'completed');
    assert.ok(provider.state.requests.at(-1).body.messages.some(message => message.role === 'tool' && message.content.includes('schedule-1')));
    const activity = worker.events
      .filter(event => event.type === 'chat' && event.event?.type === 'activity')
      .map(event => event.event.activity);
    assert.ok(activity.some(item => item.tool === 'tools' && item.label && item.state === 'running'));
    assert.ok(activity.some(item => item.tool === 'tools' && item.state === 'completed'));
    assert.ok(activity.every(item => !('arguments' in item) && !('result' in item)));
  } finally {
    await worker.kill();
    await provider.close();
    await rm(data, { recursive: true, force: true });
  }
});

test('a structured ask reaches the host through the tools layer', { timeout: 45000 }, async () => {
  const data = await mkdtemp(path.join(os.tmpdir(), 'talo-ask-'));
  const provider = await startMockProvider({
    mode: 'agent-tool',
    agentArguments: {
      operation: 'ask', header: 'Which folder?',
      questions: [{ id: 'q1', question: 'Where?', options: [{ id: 'a', label: 'Docs' }, { id: 'b', label: 'Desktop' }], recommended: 'a' }],
    },
  });
  const worker = startWorker(data, { idleMs: 2000 });
  try {
    await worker.waitReady();
    const selection = { conversationId: 'ask-chat', provider: 'custom', model: 'mock-model', apiKey: provider.state.key, baseUrl: provider.url };
    await worker.request('open', 'session.open', { ...selection, sessionPath: null });
    await worker.request('prompt', 'session.prompt', {
      conversationId: 'ask-chat', generationId: 'gen-ask-1',
      text: 'Ask me where to save', capabilities: ['web'], context: '',
    });
    const call = await worker.wait(event => event.type === 'agent' && event.message?.method === 'agent.call');
    assert.equal(call.message.params.operation, 'ask');
    assert.equal(call.message.params.questions[0].question, 'Where?');
    worker.send({ type: 'agent_response', message: { jsonrpc: '2.0', id: call.message.id, result: { askId: 'ask-1', expired: false, answers: [{ questionId: 'q1', optionId: 'b', decided: false }] } } });
    const done = await worker.waitGeneration('ask-chat', 'gen-ask-1');
    assert.equal(done.event.state, 'completed');
    assert.ok(provider.state.requests.at(-1).body.messages.some(message => message.role === 'tool' && message.content.includes('ask-1')));
  } finally {
    await worker.kill();
    await provider.close();
    await rm(data, { recursive: true, force: true });
  }
});

test('an ask payload that breaks the schema fails with an error the model can act on', { timeout: 45000 }, async () => {
  const data = await mkdtemp(path.join(os.tmpdir(), 'talo-ask-bad-'));
  const provider = await startMockProvider({
    mode: 'agent-tool',
    agentArguments: {
      operation: 'ask', header: 'Which folder?',
      questions: [{ id: 'q1', question: 'Where?', options: [{ id: 'a', label: 'Docs', title: 'Docs folder' }, { id: 'b', label: 'Desktop' }] }],
    },
  });
  const worker = startWorker(data, { idleMs: 2000 });
  try {
    await worker.waitReady();
    const selection = { conversationId: 'ask-bad-chat', provider: 'custom', model: 'mock-model', apiKey: provider.state.key, baseUrl: provider.url };
    await worker.request('open', 'session.open', { ...selection, sessionPath: null });
    await worker.request('prompt', 'session.prompt', {
      conversationId: 'ask-bad-chat', generationId: 'gen-ask-2',
      text: 'Ask me where to save', capabilities: ['web'], context: '',
    });
    const done = await worker.waitGeneration('ask-bad-chat', 'gen-ask-2');
    assert.equal(done.event.state, 'completed');
    const toolMessage = provider.state.requests.at(-1).body.messages.find(message => message.role === 'tool');
    assert.ok(toolMessage, 'the model sees the tool outcome');
    assert.match(toolMessage.content, /schema/i, 'the failure names the schema');
    assert.match(toolMessage.content, /additional propert|options\/0/i, 'the failure names the offending field so the model can fix it');
  } finally {
    await worker.kill();
    await provider.close();
    await rm(data, { recursive: true, force: true });
  }
});
