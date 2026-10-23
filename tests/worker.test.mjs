import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { SessionManager } from '@earendil-works/pi-coding-agent';
import { startWorker } from './helpers/worker.mjs';
import { startMockProvider } from './helpers/mock-provider.mjs';

async function filesUnder(directory) {
  const result = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const full = path.join(directory, entry.name);
    if (entry.isDirectory()) result.push(...await filesUnder(full));
    else if (entry.isFile()) result.push(full);
  }
  return result;
}

test('explicit session restore rejects missing and empty files without replacing them', { timeout: 40000 }, async () => {
  const data = await mkdtemp(path.join(os.tmpdir(), 'talo-missing-session-'));
  const worker = startWorker(data);
  try {
    await worker.waitReady();
    const missing = path.join(data, 'sessions', 'missing.jsonl');
    await assert.rejects(worker.request('missing', 'session.open', {
      conversationId: 'missing', sessionPath: missing,
    }), /session.*(?:missing|unavailable)/i);
    const empty = path.join(data, 'sessions', 'empty.jsonl');
    await writeFile(empty, '');
    await assert.rejects(worker.request('empty', 'session.open', {
      conversationId: 'empty', sessionPath: empty,
    }), /session.*(?:empty|damaged)/i);
    assert.equal(await readFile(empty, 'utf8'), '', 'a damaged session must not be overwritten');
    const corrupt = path.join(data, 'sessions', 'corrupt.jsonl');
    await writeFile(corrupt, '{private damaged history');
    await assert.rejects(worker.request('corrupt', 'session.open', {conversationId:'corrupt',sessionPath:corrupt}), /damaged/i);
    assert.equal(await readFile(corrupt,'utf8'),'{private damaged history');
    assert.ok(await worker.request('health', 'ping'));
  } finally {
    await worker.kill();
    await rm(data, { recursive: true, force: true });
  }
});

test('worker protocol, provider discovery without a session, legacy restore, and idle sleep', { timeout: 120_000 }, async () => {
  const data = await mkdtemp(path.join(os.tmpdir(), 'talo-worker-'));
  const worker = startWorker(data);
  try {
    await worker.waitReady();
    assert.equal((await worker.request('ping', 'ping')).sdk, '1.4.0');

    worker.child.stdin.write('{broken\n');
    const invalid = await worker.wait(event => event.type === 'error');
    assert.equal(invalid.message, 'Invalid protocol request');

    // Provider discovery is registry-only and must not require an opened session.
    const openai = await worker.request('models', 'models.list', { provider: 'openai' });
    assert.equal(openai.provider, 'openai');
    assert.equal(openai.native, true);
    assert.ok(openai.models.length > 0);
    assert.ok(openai.models.every(model => typeof model.reasoning === 'boolean'));

    const anthropic = await worker.request('models-a', 'models.list', { provider: 'anthropic' });
    assert.equal(anthropic.native, true);
    assert.ok(anthropic.models.length > 0);

    await assert.rejects(
      worker.request('bad-path', 'session.open', {
        conversationId: 'security',
        sessionPath: path.join(os.tmpdir(), 'outside-talo.jsonl'),
        provider: 'openai',
        model: 'gpt-4o',
      }),
      /inside Talo data/,
    );

    assert.equal(await worker.request('stop', 'shutdown'), true);
    assert.equal(await worker.exited, 0);

    // Seed a real Pi transcript through the unmodified SDK to exercise durable restore.
    const manager = SessionManager.create(path.join(data, 'agent-workspace'), path.join(data, 'sessions'));
    manager.appendMessage({ role: 'user', content: 'Persistence fixture', timestamp: Date.now() });
    const durableId = manager.getSessionId();

    const restored = startWorker(data);
    try {
      await restored.waitReady();
      const result = await restored.request('restore', 'session.open');
      assert.equal(result.error, undefined, result.error);
      assert.equal(result.sessionId, durableId);
      assert.match(result.path, /\.jsonl$/i);
      const snapshot = await restored.request('snap', 'session.snapshot', { conversationId: 'default' });
      assert.equal(snapshot.messages.at(-1).text, 'Persistence fixture');
      assert.equal(snapshot.state, 'idle');
      assert.equal(await restored.request('end', 'shutdown'), true);
      assert.equal(await restored.exited, 0);
    } finally {
      await restored.kill();
    }

    const sleepy = startWorker(data, { idleMs: 300 });
    try {
      await sleepy.waitReady();
      assert.ok(await sleepy.wait(event => event.type === 'sleeping', 10_000));
      assert.equal(await sleepy.exited, 0);
    } finally {
      await sleepy.kill();
    }
  } finally {
    await worker.kill();
    await rm(data, { recursive: true, force: true });
  }
});

test('long replies stream with linear payload size and retain their complete final text', { timeout: 40000 }, async () => {
  const data = await mkdtemp(path.join(os.tmpdir(), 'talo-delta-size-'));
  const chunks = Array.from({ length: 128 }, (_, index) => `${index}:` + '界'.repeat(1024));
  const mock = await startMockProvider({ chunks });
  const worker = startWorker(data);
  try {
    await worker.waitReady();
    await worker.request('open', 'session.open', {
      conversationId: 'linear', provider: 'custom', model: 'mock-model',
      baseUrl: mock.url, apiKey: mock.state.key, reasoning: false,
    });
    const mark = worker.mark();
    await worker.request('send', 'session.prompt', { conversationId: 'linear', generationId: 'g', text: 'Long reply' });
    const done = await worker.waitGeneration('linear', 'g', ['completed', 'failed'], mark, 30000);
    assert.equal(done.event.state, 'completed', done.event.error);
    const expected = chunks.join('');
    assert.equal(done.event.text, expected);
    const events = worker.generationEvents('linear', 'g');
    assert.equal(events.map(event => event.delta?.text ?? '').join(''), expected);
    assert.ok(events.reduce((bytes, event) => bytes + Buffer.byteLength(JSON.stringify(event)), 0) < Buffer.byteLength(expected) * 5);
    const snapshot = await worker.request('snapshot', 'session.snapshot', { conversationId: 'linear' });
    assert.equal(snapshot.messages.at(-1).text, expected);
  } finally {
    await worker.kill();
    await mock.close();
    await rm(data, { recursive: true, force: true });
  }
});

test('real Pi streaming, authenticated provider validation, cancellation, and retries', { timeout: 150_000 }, async () => {
  const data = await mkdtemp(path.join(os.tmpdir(), 'talo-chat-'));
  const mock = await startMockProvider();
  const secret = mock.state.key;
  const worker = startWorker(data, { idleMs: 60_000 });
  try {
    await worker.waitReady();

    const validated = await worker.request('validate', 'provider.validate', {
      provider: 'custom',
      model: 'mock-model',
      baseUrl: mock.url,
      apiKey: secret,
      reasoning: false,
    }, 40_000);
    assert.equal(validated.authenticated, true);
    assert.equal(validated.model, 'mock-model');

    await assert.rejects(
      worker.request('validate-bad', 'provider.validate', {
        provider: 'custom',
        model: 'mock-model',
        baseUrl: mock.url,
        apiKey: 'wrong-secret-key',
        reasoning: false,
      }, 40_000),
      error => !error.message.includes('wrong-secret-key'),
    );

    const opened = await worker.request('open', 'session.open', {
      conversationId: 'c1',
      sessionPath: null,
      provider: 'custom',
      model: 'mock-model',
      baseUrl: mock.url,
      apiKey: secret,
      reasoning: false,
    }, 40_000);
    assert.ok(opened.sessionId);
    assert.match(opened.path, /\.jsonl$/i);
    assert.equal(opened.messageCount, 0);
    assert.equal(opened.messages, undefined, 'session.open must not return the transcript');
    const history = await worker.request('history-empty', 'session.history', { conversationId: 'c1' });
    assert.deepEqual(history.messages, []);
    assert.equal(history.nextOffset, null);

    const firstMark = worker.mark();
    await worker.request('send', 'session.prompt', {
      conversationId: 'c1',
      generationId: 'g1',
      text: 'Hello',
    });
    const firstDone = await worker.waitGeneration('c1', 'g1', ['completed', 'failed', 'cancelled'], firstMark, 40_000);
    assert.equal(firstDone.event.state, 'completed', firstDone.event.error);
    assert.equal(firstDone.event.text, 'Hello from mock');
    assert.equal(firstDone.event.message.status, 'completed');
    assert.equal(firstDone.event.message.role, 'assistant');
    const chatRequest = mock.state.requests.find(request =>
      request.url?.endsWith('/chat/completions') &&
      request.body?.messages?.some(message => typeof message.content === 'string'
        ? message.content === 'Hello'
        : Array.isArray(message.content) && message.content.some(part => part.type === 'text' && part.text === 'Hello')));
    assert.ok(chatRequest, 'the prompt should reach the provider');
    assert.ok(chatRequest.body.messages.some(message =>
      message.role === 'system' && /You are Talo|default name is Talo/u.test(JSON.stringify(message.content))),
    'chat should use the Talo persona');
    assert.deepEqual(chatRequest.body.tools.map(tool => tool.function.name).sort(), ['edit','read','tools','write'], 'one managed Pi extension discovers MCP groups; file tools remain native and shell stays off');
    assert.ok(!chatRequest.body.messages.some(message => message.role === 'tool'),
      'worker must not send tool results');
    assert.equal(chatRequest.body.stream, true);
    const streamed = worker.generationEvents('c1', 'g1').filter(event => event.state === 'generating');
    assert.ok(streamed.length >= 2, 'assistant text should stream in multiple events');
    const deltas = streamed.filter(event => event.delta?.text);
    assert.ok(deltas.length >= 2);
    assert.equal(deltas.map(event => event.delta.text).join(''), 'Hello from mock');
    assert.ok(deltas.every(event => event.text === undefined && event.reasoning === undefined));
    assert.ok(streamed.some(event => event.delta?.reset));

    let snapshot = await worker.request('snap1', 'session.snapshot', { conversationId: 'c1' });
    assert.deepEqual(snapshot.messages.map(message => message.role), ['user', 'assistant']);
    assert.equal(snapshot.messages[0].text, 'Hello');
    assert.equal(snapshot.messages[1].text, 'Hello from mock');
    assert.equal(snapshot.state, 'completed');

    // U+2028 is valid JSON string content; the worker must not treat it as a line boundary.
    mock.configure({ chunks: ['unicode', ' ok'], delayMs: 0 });
    const unicodeMark = worker.mark();
    await worker.request('send-unicode', 'session.prompt', {
      conversationId: 'c1',
      generationId: 'g1-unicode',
      text: 'Hello world',
    });
    const unicodeDone = await worker.waitGeneration('c1', 'g1-unicode', ['completed', 'failed'], unicodeMark, 40_000);
    assert.equal(unicodeDone.event.state, 'completed', unicodeDone.event.error);
    assert.equal(unicodeDone.event.text, 'unicode ok');
    snapshot = await worker.request('snap1b', 'session.snapshot', { conversationId: 'c1' });
    assert.equal(snapshot.messages[2].text, 'Hello world');

    const page = await worker.request('history-page', 'session.history', { conversationId: 'c1', offset: 1, limit: 1 });
    assert.deepEqual(page.messages.map(message => message.text), ['Hello from mock']);
    assert.equal(page.nextOffset, 2);

    // Credentials must never be written to Talo data or echoed through protocol events.
    for (const file of await filesUnder(data)) {
      const content = await readFile(file, 'utf8').catch(() => '');
      assert.ok(!content.includes(secret), `credential leaked into ${path.relative(data, file)}`);
    }
    assert.ok(!JSON.stringify(worker.events).includes(secret));
    assert.ok(!worker.stderr().includes(secret));

    mock.configure({ chunks: ['slow', ' chunk', ' one', ' two', ' three'], delayMs: 140, mode: 'success' });
    const cancelMark = worker.mark();
    await worker.request('open2', 'session.open', {
      conversationId: 'c2',
      sessionPath: null,
      provider: 'custom',
      model: 'mock-model',
      baseUrl: mock.url,
      apiKey: secret,
    }, 40_000);
    await worker.request('send2', 'session.prompt', {
      conversationId: 'c2',
      generationId: 'g2',
      text: 'Cancel me',
    });
    await worker.waitFrom(cancelMark, event =>
      event.type === 'chat' && event.event?.type === 'generation' &&
      event.event.generationId === 'g2' && event.event.delta?.text === 'slow', 20_000);
    const aborted = await worker.request('abort', 'session.abort', { generationId: 'g2' }, 20_000);
    assert.equal(aborted.aborted, true);
    const cancelled = await worker.waitGeneration('c2', 'g2', ['cancelled'], cancelMark, 20_000);
    assert.equal(cancelled.event.state, 'cancelled');
    snapshot = await worker.request('snap2', 'session.snapshot', { conversationId: 'c2' });
    assert.equal(snapshot.messages.at(-1).status, 'cancelled');

    mock.configure({ chunks: ['retried', ' ok'], delayMs: 0, mode: 'success' });
    const retryMark = worker.mark();
    // The host re-arms the same generation id for a retry.
    await worker.request('retry2', 'session.retry', { conversationId: 'c2', generationId: 'g2' });
    const retried = await worker.waitGeneration('c2', 'g2', ['completed', 'failed'], retryMark, 40_000);
    assert.equal(retried.event.state, 'completed', retried.event.error);
    assert.equal(retried.event.text, 'retried ok');
    snapshot = await worker.request('snap2b', 'session.snapshot', { conversationId: 'c2' });
    assert.equal(snapshot.messages.filter(message => message.role === 'user').length, 1);
    assert.equal(snapshot.messages.filter(message => message.role === 'assistant').length, 1);
    assert.equal(snapshot.messages.at(-1).text, 'retried ok');

    mock.configure({ mode: 'unauthorized', chunks: ['ignored'], delayMs: 0 });
    const failMark = worker.mark();
    await worker.request('open3', 'session.open', {
      conversationId: 'c3',
      sessionPath: null,
      provider: 'custom',
      model: 'mock-model',
      baseUrl: mock.url,
      apiKey: secret,
    }, 40_000);
    await worker.request('send3', 'session.prompt', {
      conversationId: 'c3',
      generationId: 'g3',
      text: 'This will fail',
    });
    const failed = await worker.waitGeneration('c3', 'g3', ['failed'], failMark, 40_000);
    assert.equal(failed.event.state, 'failed');
    assert.ok(failed.event.error);
    snapshot = await worker.request('snap3', 'session.snapshot', { conversationId: 'c3' });
    assert.equal(snapshot.messages.at(-1).status, 'failed');

    mock.configure({ mode: 'success', chunks: ['recovered'], delayMs: 0 });
    const failRetryMark = worker.mark();
    await worker.request('retry3', 'session.retry', { conversationId: 'c3', generationId: 'g3' });
    const recovered = await worker.waitGeneration('c3', 'g3', ['completed', 'failed'], failRetryMark, 40_000);
    assert.equal(recovered.event.state, 'completed', recovered.event.error);
    assert.equal(recovered.event.text, 'recovered');
    snapshot = await worker.request('snap3b', 'session.snapshot', { conversationId: 'c3' });
    assert.equal(snapshot.messages.filter(message => message.role === 'user').length, 1);
    assert.equal(snapshot.messages.filter(message => message.role === 'assistant').length, 1);

    // The worker still answers immediately after cancellation and retry paths.
    assert.equal((await worker.request('ping2', 'ping')).sdk, '1.4.0');
    assert.equal(await worker.request('stop', 'shutdown'), true);
    assert.equal(await worker.exited, 0);
  } finally {
    await worker.kill();
    await mock.close();
    await rm(data, { recursive: true, force: true });
  }
});

test('idle sleep waits for an in-flight generation', { timeout: 120_000 }, async () => {
  const data = await mkdtemp(path.join(os.tmpdir(), 'talo-idle-'));
  const mock = await startMockProvider({
    chunks: ['one', ' two', ' three', ' four', ' five', ' six', ' seven', ' eight'],
    delayMs: 120,
  });
  const worker = startWorker(data, { idleMs: 400 });
  try {
    await worker.waitReady();
    await worker.request('open', 'session.open', {
      conversationId: 'idle',
      sessionPath: null,
      provider: 'custom',
      model: 'mock-model',
      baseUrl: mock.url,
      apiKey: mock.state.key,
    }, 40_000);
    const mark = worker.mark();
    await worker.request('send', 'session.prompt', {
      conversationId: 'idle',
      generationId: 'idle-generation',
      text: 'Take your time',
    });
    await worker.waitFrom(mark, event =>
      event.type === 'chat' && event.event?.type === 'generation' &&
      event.event.generationId === 'idle-generation' &&
      typeof event.event.delta?.text === 'string' && event.event.delta.text.length > 0, 20_000);
    await new Promise(resolve => setTimeout(resolve, 900));
    assert.equal(worker.child.exitCode, null, 'worker must not sleep while generating');
    assert.ok(!worker.events.some(event => event.type === 'sleeping'));
    const completed = await worker.waitGeneration('idle', 'idle-generation', ['completed', 'failed'], mark, 30_000);
    assert.equal(completed.event.state, 'completed', completed.event.error);
    assert.ok(await worker.wait(event => event.type === 'sleeping', 10_000));
    assert.equal(await worker.exited, 0);
  } finally {
    await worker.kill();
    await mock.close();
    await rm(data, { recursive: true, force: true });
  }
});

test('explicit per-conversation sessions survive a worker crash and retry the interrupted turn', { timeout: 150_000 }, async () => {
  const data = await mkdtemp(path.join(os.tmpdir(), 'talo-crash-'));
  const mock = await startMockProvider({
    chunks: Array.from({ length: 40 }, (_, index) => `part${index} `),
    delayMs: 100,
  });
  const secret = mock.state.key;
  const first = startWorker(data, { idleMs: 60_000 });
  let sessionPath;
  try {
    await first.waitReady();
    const opened = await first.request('open', 'session.open', {
      conversationId: 'crash',
      sessionPath: null,
      provider: 'custom',
      model: 'mock-model',
      baseUrl: mock.url,
      apiKey: secret,
    }, 40_000);
    sessionPath = opened.path;
    assert.match(sessionPath, /\.jsonl$/i);
    const mark = first.mark();
    await first.request('send', 'session.prompt', {
      conversationId: 'crash',
      generationId: 'g-crash',
      text: 'Survive a crash',
    });
    await first.waitFrom(mark, event =>
      event.type === 'chat' && event.event?.type === 'generation' &&
      event.event.generationId === 'g-crash' &&
      typeof event.event.delta?.text === 'string' && event.event.delta.text.length > 0, 20_000);
    // Simulate a host/worker crash, not an orderly abort.
    first.child.kill('SIGKILL');
    await first.exited;
  } finally {
    await first.kill();
  }

  const second = startWorker(data, { idleMs: 60_000 });
  try {
    await second.waitReady();
    const restored = await second.request('restore', 'session.open', {
      conversationId: 'crash',
      sessionPath,
      provider: 'custom',
      model: 'mock-model',
      baseUrl: mock.url,
      apiKey: secret,
    }, 40_000);
    assert.ok(restored.sessionId);
    let snapshot = await second.request('snap', 'session.snapshot', { conversationId: 'crash' });
    assert.equal(snapshot.messages[0].role, 'user');
    assert.equal(snapshot.messages[0].text, 'Survive a crash');

    mock.configure({ mode: 'success', chunks: ['recovered', ' after', ' crash'], delayMs: 0 });
    const retryMark = second.mark();
    await second.request('retry', 'session.retry', { conversationId: 'crash', generationId: 'g-crash-retry' });
    const recovered = await second.waitGeneration('crash', 'g-crash-retry', ['completed', 'failed'], retryMark, 40_000);
    assert.equal(recovered.event.state, 'completed', recovered.event.error);
    assert.equal(recovered.event.text, 'recovered after crash');
    snapshot = await second.request('snap2', 'session.snapshot', { conversationId: 'crash' });
    assert.equal(snapshot.messages.filter(message => message.role === 'user').length, 1);
    assert.equal(snapshot.messages.filter(message => message.role === 'assistant').length, 1);
    assert.equal(await second.request('stop', 'shutdown'), true);
    assert.equal(await second.exited, 0);
  } finally {
    await second.kill();
    await mock.close();
    await rm(data, { recursive: true, force: true });
  }
});


test('concurrent conversations keep separate provider credentials and endpoints', { timeout: 150_000 }, async () => {
  const data = await mkdtemp(path.join(os.tmpdir(), 'talo-isolation-'));
  const firstMock = await startMockProvider({
    key: 'first-secret-key',
    chunks: ['first', ' response'],
    delayMs: 120,
  });
  const secondMock = await startMockProvider({
    key: 'second-secret-key',
    chunks: ['second', ' response'],
    delayMs: 120,
  });
  const worker = startWorker(data, { idleMs: 60_000 });
  try {
    await worker.waitReady();
    await worker.request('open-a', 'session.open', {
      conversationId: 'isolation-a',
      sessionPath: null,
      provider: 'custom',
      model: 'mock-model',
      baseUrl: firstMock.url,
      apiKey: firstMock.state.key,
    }, 40_000);
    await worker.request('open-b', 'session.open', {
      conversationId: 'isolation-b',
      sessionPath: null,
      provider: 'custom',
      model: 'mock-model',
      baseUrl: secondMock.url,
      apiKey: secondMock.state.key,
    }, 40_000);

    const mark = worker.mark();
    await worker.request('send-a', 'session.prompt', {
      conversationId: 'isolation-a',
      generationId: 'isolation-ga',
      text: 'First conversation',
    });
    await worker.request('send-b', 'session.prompt', {
      conversationId: 'isolation-b',
      generationId: 'isolation-gb',
      text: 'Second conversation',
    });
    const [a, b] = await Promise.all([
      worker.waitGeneration('isolation-a', 'isolation-ga', ['completed', 'failed'], mark, 60_000),
      worker.waitGeneration('isolation-b', 'isolation-gb', ['completed', 'failed'], mark, 60_000),
    ]);
    assert.equal(a.event.state, 'completed', a.event.error);
    assert.equal(b.event.state, 'completed', b.event.error);
    assert.equal(a.event.text, 'first response');
    assert.equal(b.event.text, 'second response');
    const snapshotA = await worker.request('snap-a', 'session.snapshot', { conversationId: 'isolation-a' });
    const snapshotB = await worker.request('snap-b', 'session.snapshot', { conversationId: 'isolation-b' });
    assert.equal(snapshotA.messages.at(-1).text, 'first response');
    assert.equal(snapshotB.messages.at(-1).text, 'second response');
    assert.ok(!JSON.stringify(worker.events).includes(firstMock.state.key));
    assert.ok(!JSON.stringify(worker.events).includes(secondMock.state.key));
    assert.equal(await worker.request('stop', 'shutdown'), true);
    assert.equal(await worker.exited, 0);
  } finally {
    await worker.kill();
    await firstMock.close();
    await secondMock.close();
    await rm(data, { recursive: true, force: true });
  }
});

test('bounded memory context reaches Pi without duplicate injections and is refreshed after wake', { timeout: 40000 }, async () => {
  const data = await mkdtemp(path.join(os.tmpdir(), 'talo-memory-context-'));
  const mock = await startMockProvider();
  let worker = startWorker(data);
  const selection = { conversationId:'memory-chat',provider:'custom',model:'mock-model',baseUrl:mock.url,apiKey:mock.state.key };
  const context = '\n# USER.md\n- Approved preference: use concise examples\n# MEMORY.md\n- Project decision: use SQLite\n';
  try {
    await worker.waitReady();
    const opened=await worker.request('open','session.open',selection);
    for(let i=0;i<2;i++) {
      const mark=worker.mark();
      await worker.request(`prompt-${i}`,'session.prompt',{conversationId:'memory-chat',generationId:`memory-${i}`,text:`Question ${i}`,context});
      const done=await worker.waitGeneration('memory-chat',`memory-${i}`,['completed','failed'],mark);
      assert.equal(done.event.state,'completed',done.event.error);
      const body=mock.state.requests.filter(r=>r.url.endsWith('/chat/completions')).at(-1).body;
      const system=body.messages.filter(m=>m.role==='system').map(m=>JSON.stringify(m.content)).join('\n');
      assert.equal(system.split('Approved preference: use concise examples').length-1,1,'memory should be injected exactly once');
      assert.match(system,/Project decision: use SQLite/);
    }
    await assert.rejects(worker.request('budget','session.prompt',{conversationId:'memory-chat',generationId:'too-big',text:'No',context:'x'.repeat(4097)}),/budget/);
    await worker.close();
    worker=startWorker(data);await worker.waitReady();
    const resumed=await worker.request('resume','session.open',{...selection,sessionPath:opened.path});
    assert.equal(resumed.sessionId,opened.sessionId);
    assert.ok(resumed.messages.some(m=>m.text==='Question 0'));
    const mark=worker.mark();
    await worker.request('fresh-context','session.prompt',{conversationId:'memory-chat',generationId:'after-wake',text:'After wake',context:'\n# MEMORY.md\n- Updated project decision: use Rust\n'});
    assert.equal((await worker.waitGeneration('memory-chat','after-wake',['completed','failed'],mark)).event.state,'completed');
    const body=mock.state.requests.filter(r=>r.url.endsWith('/chat/completions')).at(-1).body;
    const system=body.messages.filter(m=>m.role==='system').map(m=>JSON.stringify(m.content)).join('\n');
    assert.match(system,/Updated project decision: use Rust/);
    assert.ok(!system.includes('Approved preference: use concise examples'),'superseded context must not remain in current system prompt');
  } finally {await worker.close();await mock.close();await rm(data,{recursive:true,force:true});}
});

test('maximum host image payload reaches a completed generation', { timeout:40000 }, async()=>{
  const data=await mkdtemp(path.join(os.tmpdir(),'talo-image-payload-'));
  const mock=await startMockProvider();
  const worker=startWorker(data);
  try {
    await worker.waitReady();
    await worker.request('open','session.open',{conversationId:'images',provider:'custom',model:'mock-model',baseUrl:mock.url,apiKey:mock.state.key});
    const image={name:'screenshot.png',mimeType:'image/png',data:Buffer.alloc(8*1024*1024).toString('base64'),path:path.join(data,'screenshot.png')};
    const mark=worker.mark();
    await worker.request('prompt','session.prompt',{conversationId:'images',generationId:'images-turn',text:'Describe the attachments',images:[image,image]});
    const done=await worker.waitGeneration('images','images-turn',['completed','failed'],mark);
    assert.equal(done.event.state,'completed',done.event.error);
    assert.equal(done.event.text,'Hello from mock');
  } finally {await worker.close();await mock.close();await rm(data,{recursive:true,force:true});}
});

test('concurrent opens cannot share one authoritative session file', { timeout:40000 }, async()=>{
  const data=await mkdtemp(path.join(os.tmpdir(),'talo-session-owner-'));
  const mock=await startMockProvider();let worker=startWorker(data);
  const selection={provider:'custom',model:'mock-model',baseUrl:mock.url,apiKey:mock.state.key};
  try {
    await worker.waitReady();
    const opened=await worker.request('seed','session.open',{...selection,conversationId:'seed'});
    assert.equal(opened.autoCompactionEnabled,true);
    const mark=worker.mark();await worker.request('seed-prompt','session.prompt',{conversationId:'seed',generationId:'seed-generation',text:'Original history'});
    await worker.waitGeneration('seed','seed-generation',['completed'],mark);
    await worker.close();worker=startWorker(data);await worker.waitReady();
    const result=await Promise.allSettled([
      worker.request('owner-a','session.open',{...selection,conversationId:'owner-a',sessionPath:opened.path}),
      worker.request('owner-b','session.open',{...selection,conversationId:'owner-b',sessionPath:opened.path}),
    ]);
    assert.equal(result.filter(r=>r.status==='fulfilled').length,1);
    assert.match(result.find(r=>r.status==='rejected').reason.message,/already open/);
    assert.ok((await readFile(opened.path,'utf8')).includes('Original history'));
  }finally{await worker.close();await mock.close();await rm(data,{recursive:true,force:true});}
});

test('cancelling one conversation leaves a concurrent generation untouched', { timeout: 60000 }, async () => {
  const data = await mkdtemp(path.join(os.tmpdir(), 'talo-cancel-isolation-'));
  const mock = await startMockProvider({
    chunks: ['one', ' two', ' three', ' four', ' five', ' six'],
    delayMs: 120,
  });
  const worker = startWorker(data, { idleMs: 60_000 });
  try {
    await worker.waitReady();
    for (const id of ['cancel-a', 'cancel-b']) {
      await worker.request(`open-${id}`, 'session.open', {
        conversationId: id,
        sessionPath: null,
        provider: 'custom',
        model: 'mock-model',
        baseUrl: mock.url,
        apiKey: mock.state.key,
      }, 40_000);
    }
    const mark = worker.mark();
    await worker.request('send-a', 'session.prompt', {
      conversationId: 'cancel-a',
      generationId: 'ga',
      text: 'Stop this one',
    });
    await worker.request('send-b', 'session.prompt', {
      conversationId: 'cancel-b',
      generationId: 'gb',
      text: 'Keep this one',
    });
    await worker.waitFrom(mark, event =>
      event.type === 'chat' && event.event?.type === 'generation' &&
      event.event.generationId === 'ga' && typeof event.event.delta?.text === 'string' && event.event.delta.text.length > 0,
      20_000);
    assert.equal((await worker.request('abort-a', 'session.abort', { generationId: 'ga' }, 20_000)).aborted, true);
    const [cancelled, completed] = await Promise.all([
      worker.waitGeneration('cancel-a', 'ga', ['cancelled', 'failed', 'completed'], mark, 30_000),
      worker.waitGeneration('cancel-b', 'gb', ['cancelled', 'failed', 'completed'], mark, 40_000),
    ]);
    assert.equal(cancelled.event.state, 'cancelled');
    assert.equal(completed.event.state, 'completed', completed.event.error);
    assert.equal(completed.event.text, mock.state.chunks.join(''));
    const first = await worker.request('snap-a', 'session.snapshot', { conversationId: 'cancel-a' });
    const second = await worker.request('snap-b', 'session.snapshot', { conversationId: 'cancel-b' });
    assert.equal(first.messages.at(-1).status, 'cancelled');
    assert.equal(second.messages.at(-1).status, 'completed');
    assert.ok(worker.generationEvents('cancel-a', 'ga').every(event => event.conversationId === 'cancel-a'));
    assert.ok(worker.generationEvents('cancel-b', 'gb').every(event => event.conversationId === 'cancel-b'));
  } finally {
    await worker.kill();
    await mock.close();
    await rm(data, { recursive: true, force: true });
  }
});

test('a delegated result resumes the parent as hidden context, not a fabricated user turn', { timeout: 40000 }, async () => {
  const data = await mkdtemp(path.join(os.tmpdir(), 'talo-delivery-'));
  const mock = await startMockProvider({ chunks: ['Combined', ' answer'] });
  const worker = startWorker(data, { idleMs: 60_000 });
  try {
    await worker.waitReady();
    await worker.request('open', 'session.open', {
      conversationId: 'parent',
      sessionPath: null,
      provider: 'custom',
      model: 'mock-model',
      baseUrl: mock.url,
      apiKey: mock.state.key,
    }, 40_000);
    const first = worker.mark();
    await worker.request('first', 'session.prompt', {
      conversationId: 'parent',
      generationId: 'first',
      text: 'Original question',
    });
    assert.equal((await worker.waitGeneration('parent', 'first', ['completed', 'failed'], first)).event.state, 'completed');

    const delivery = worker.mark();
    await worker.request('deliver', 'session.deliver', {
      conversationId: 'parent',
      generationId: 'delivery',
      text: 'Task: Read the RFC\nState: completed\nResult: Section 4 is stable.',
    });
    const done = await worker.waitGeneration('parent', 'delivery', ['completed', 'failed'], delivery);
    assert.equal(done.event.state, 'completed', done.event.error);
    assert.equal(done.event.text, 'Combined answer');

    const snapshot = await worker.request('snap', 'session.snapshot', { conversationId: 'parent' });
    assert.equal(snapshot.messages.filter(message => message.role === 'user').length, 1);
    assert.equal(snapshot.messages.filter(message => message.role === 'assistant').length, 2);
    assert.ok(!snapshot.messages.some(message => message.role === 'user' && message.text.includes('Section 4 is stable')));
    const request = mock.state.requests.filter(item => item.url.endsWith('/chat/completions')).at(-1);
    assert.ok(request.body.messages.some(message => message.role === 'custom' || JSON.stringify(message.content).includes('Section 4 is stable')),
      'the structured result should reach the model as context');
  } finally {
    await worker.kill();
    await mock.close();
    await rm(data, { recursive: true, force: true });
  }
});
