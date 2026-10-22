import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { stripTypeScriptTypes } from 'node:module';

let counter = 0;
async function store(invoke = async () => { throw new Error('Unexpected invoke'); }) {
  const key = `__chatTest${counter++}`;
  globalThis[key] = { invoke };
  const source = (await readFile(new URL('../apps/ui/src/lib/chat.svelte.ts', import.meta.url), 'utf8'))
    .replace(/^import[\s\S]*?from ['"][^'"]+['"];\r?\n/gm, '');
  const code = stripTypeScriptTypes(`const { invoke } = globalThis.${key};
    const isTauri = () => true;
    const listen = async () => () => {};
    const providerName = value => value;
    const $state = value => value;
    ${source}
    export { handleEvent, applySnapshot, refreshSnapshot };
  `);
  const module = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
  delete globalThis[key];
  return module;
}
function message(id, role = 'assistant') {
  return { id: String(id), role, text: String(id), status: 'completed', generationId: 'gen' };
}
function snapshot(overrides = {}) {
  return { conversation: { id: 'chat', title: 'Chat', updatedAt: new Date().toISOString(), selection: null }, messages: [], state: 'generating', generationId: 'gen', sequence: 0, before: null, after: null, ...overrides };
}
function event(sequence, rest = {}) {
  return { type: 'generation', conversationId: 'chat', generationId: 'gen', state: 'generating', sequence, ...rest };
}
function deferred() {
  let resolve;
  const promise = new Promise(done => { resolve = done; });
  return { promise, resolve };
}

test('stream deltas append once and reset both reply and reasoning', async () => {
  const api = await store();
  api.applySnapshot(snapshot({ messages: [{ ...message('a'), text: 'hello', reasoning: 'old' }] }));
  api.handleEvent(event(1, { delta: { text: ' world', reasoning: ' thought' } }));
  api.handleEvent(event(1, { delta: { text: 'duplicate' } }));
  assert.equal(api.chat.active.messages[0].text, 'hello world');
  assert.equal(api.chat.active.messages[0].reasoning, 'old thought');
  api.handleEvent(event(2, { delta: { reset: true, text: 'retry' } }));
  assert.equal(api.chat.active.messages[0].text, 'retry');
  assert.equal(api.chat.active.messages[0].reasoning, undefined);
});

test('a missing delta waits for snapshot then replays newer events arriving during reconciliation', async () => {
  const pending = deferred();
  const calls = [];
  const api = await store((method) => { calls.push(method); return pending.promise; });
  api.applySnapshot(snapshot({ messages: [{ ...message('a'), text: 'a' }] }));
  api.handleEvent(event(2, { delta: { text: 'c' } }));
  assert.equal(api.chat.active.messages[0].text, 'a');
  api.handleEvent(event(3, { delta: { text: 'd' } }));
  assert.deepEqual(calls, ['conversation_open']);
  pending.resolve(snapshot({ sequence: 2, messages: [{ ...message('a'), text: 'abc' }] }));
  await pending.promise;
  await Promise.resolve();
  assert.equal(api.chat.active.messages[0].text, 'abcd');
});

test('history evicts opposite pages with bidirectional cursors and ignores responses after switching conversation', async () => {
  let next = { messages: Array.from({ length: 40 }, (_, i) => message(i)), before: null, after: '39' };
  const api = await store(async () => next);
  api.applySnapshot(snapshot({ messages: Array.from({ length: 120 }, (_, i) => message(i + 40)), before: '40' }));
  assert.equal(await api.loadHistory('before'), true);
  assert.equal(api.chat.active.messages.length, 120);
  assert.equal(api.chat.active.messages[0].id, '0');
  assert.equal(api.chat.active.after, '119');
  assert.equal(api.chat.active.before, null);
  next = { messages: Array.from({ length: 40 }, (_, i) => message(i + 120)), before: '120', after: null };
  await api.loadHistory('after');
  assert.equal(api.chat.active.messages[0].id, '40');
  assert.equal(api.chat.active.messages.at(-1).id, '159');
  assert.equal(api.chat.active.before, '40');
  assert.equal(api.chat.active.after, null);
  const pending = deferred();
  const other = await store(() => pending.promise);
  other.applySnapshot(snapshot({ before: '40', messages: [message('40')] }));
  const loading = other.loadHistory('before');
  other.applySnapshot(snapshot({ conversation: { id: 'other', title: 'Other' }, messages: [message('other')] }));
  pending.resolve({ messages: [message('old')], before: null, after: null });
  assert.equal(await loading, false);
  assert.equal(other.chat.active.messages[0].id, 'other');
});

test('live replies leave historical windows bounded and latest jump restores complete current state', async () => {
  const pending = deferred();
  const api = await store(() => pending.promise);
  api.applySnapshot(snapshot({ after: 'older', messages: [message('older')] }));
  api.handleEvent(event(1, { delta: { text: 'new' } }));
  api.handleEvent(event(2, { state: 'completed', text: 'new', message: message('new') }));
  assert.deepEqual(api.chat.active.messages.map(item => item.id), ['older']);
  assert.equal(api.chat.active.state, 'completed');
  const latest = api.jumpToLatest();
  pending.resolve(snapshot({ sequence: 2, state: 'completed', generationId: null, messages: [message('new')] }));
  await latest;
  assert.equal(api.chat.active.after, null);
  assert.equal(api.chat.active.messages[0].id, 'new');
});


test('stream growth evicts old messages while retaining history access', async () => {
  const api = await store();
  api.applySnapshot(snapshot({ messages: Array.from({ length: 120 }, (_, i) => ({ ...message(i), generationId: `old-${i}` })) }));
  api.handleEvent(event(1, { delta: { text: 'new reply' } }));
  assert.equal(api.chat.active.messages.length, 120);
  assert.equal(api.chat.active.messages[0].id, '1');
  assert.equal(api.chat.active.before, '1');
  assert.equal(api.chat.active.messages.at(-1).text, 'new reply');
});

test('failed latest load does not report a successful jump', async () => {
  const api = await store(async () => { throw new Error('Cannot open'); });
  api.applySnapshot(snapshot({ after: 'older', messages: [message('older')] }));
  assert.equal(await api.jumpToLatest(), false);
  assert.equal(api.chat.active.after, 'older');
  assert.equal(api.chat.historyRevision, 0);
});

test('a queued follow-up becomes a visible user turn when its generation starts', async () => {
  const api = await store();
  api.applySnapshot(snapshot({
    state: 'completed',
    generationId: null,
    queued: [{ id: 'queued-gen', text: 'Follow up later', createdAt: new Date().toISOString() }],
    messages: [message('current')],
  }));
  api.handleEvent({ type: 'queue', conversationId: 'chat', queued: [] });
  api.handleEvent({ type: 'generation', conversationId: 'chat', generationId: 'queued-gen', state: 'generating', sequence: 1 });
  assert.ok(api.chat.active.messages.some(item => item.role === 'user' && item.text === 'Follow up later'));
  assert.equal(api.chat.active.queued.length, 0);
});

test('delegation events update only their parent conversation cards', async () => {
  const api = await store();
  api.applySnapshot(snapshot({ messages: [] }));
  const task = {
    id: 'task-1', parentConversationId: 'chat', childConversationId: 'child-1',
    title: 'Read the RFC', brief: 'Summarize it', state: 'running', delivered: false,
    createdAt: '2025-01-01T00:00:00.000Z', updatedAt: '2025-01-01T00:00:00.000Z',
  };
  api.handleEvent({ type: 'delegation', task });
  assert.equal(api.chat.active.delegations[0].state, 'running');
  api.handleEvent({ type: 'delegation', task: { ...task, state: 'failed', error: 'Provider failed' } });
  assert.equal(api.chat.active.delegations[0].state, 'failed');
  api.handleEvent({ type: 'delegation', task: { ...task, id: 'other', parentConversationId: 'other-chat' } });
  assert.equal(api.chat.active.delegations.length, 1);
});
