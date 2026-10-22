import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
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

test('ChatGPT browser sign-in crosses the worker bridge without exposing credentials or using API-key auth', { timeout: 30_000 }, async () => {
  const data = await mkdtemp(path.join(os.tmpdir(), 'talo-chatgpt-worker-'));
  const worker = startWorker(data);
  try {
    await worker.waitReady();
    const started = await worker.request('login', 'chatgpt.begin', { hostId: 'urn:uuid:11111111-1111-4111-8111-111111111111' });
    const url = new URL(started.url);
    assert.equal(url.origin, 'https://auth.openai.com');
    assert.equal(url.searchParams.get('agent_name_hint'), 'Talo');
    assert.equal(url.searchParams.get('ext_agent_host_id'), 'urn:uuid:11111111-1111-4111-8111-111111111111');
    const callback = new URL(url.searchParams.get('redirect_uri'));
    assert.equal(callback.hostname, '127.0.0.1');
    assert.equal(callback.pathname, '/auth/callback');
    assert.deepEqual(await worker.request('poll', 'chatgpt.poll', { attemptId: started.attemptId }), { pending: true });
    await assert.rejects(worker.request('models', 'models.list', { provider: 'chatgpt' }), /Connect your ChatGPT account/);
    assert.deepEqual(await worker.request('cancel', 'chatgpt.poll', { attemptId: started.attemptId, cancel: true }), { pending: false });
    for (const file of await filesUnder(data)) {
      assert.ok(!(await readFile(file, 'utf8')).includes(url.searchParams.get('state')));
    }
  } finally {
    await worker.kill();
    await rm(data, { recursive: true, force: true });
  }
});

test('provider discovery uses the bundled Pi catalog without a session', { timeout: 120_000 }, async () => {
  const data = await mkdtemp(path.join(os.tmpdir(), 'talo-providers-'));
  const worker = startWorker(data);
  try {
    await worker.waitReady();
    for (const provider of ['openai', 'anthropic', 'google', 'openrouter']) {
      const result = await worker.request(`models-${provider}`, 'models.list', { provider });
      assert.equal(result.provider, provider);
      assert.equal(result.native, true);
      assert.ok(result.name && result.name !== provider, `${provider} should have a bundled display name`);
      assert.ok(result.models.length > 0, `${provider} should expose catalog models`);
      assert.ok(result.models.some(model => model.reasoning), `${provider} should expose reasoning metadata`);
      assert.ok(result.models.every(model =>
        typeof model.contextWindow === 'number' &&
        typeof model.maxTokens === 'number' &&
        Array.isArray(model.thinkingLevels)));
    }

    const gemini = await worker.request('models-gemini', 'models.list', { provider: 'gemini' });
    assert.equal(gemini.provider, 'gemini');
    assert.equal(gemini.native, true);
    assert.ok(gemini.models.length > 0);

    const emptyCustom = await worker.request('custom-empty', 'models.list', { provider: 'custom' });
    assert.equal(emptyCustom.native, false);
    assert.ok(Array.isArray(emptyCustom.models));

    const custom = await worker.request('custom-params', 'models.list', {
      provider: 'custom',
      model: 'mock-model',
      baseUrl: 'http://127.0.0.1:11434/v1',
      reasoning: true,
    });
    assert.equal(custom.native, false);
    assert.equal(custom.models.length, 1);
    assert.equal(custom.models[0].id, 'mock-model');
    assert.equal(custom.models[0].reasoning, true);
    assert.ok(custom.models[0].thinkingLevels.includes('medium'));

    await assert.rejects(
      worker.request('unknown', 'models.list', { provider: 'not-a-real-provider' }),
      /Unknown provider/,
    );

    assert.equal(await worker.request('stop', 'shutdown'), true);
    assert.equal(await worker.exited, 0);
  } finally {
    await worker.kill();
    await rm(data, { recursive: true, force: true });
  }
});

test('provider preference projects custom models without persisting secrets', { timeout: 120_000 }, async () => {
  const data = await mkdtemp(path.join(os.tmpdir(), 'talo-preference-'));
  const worker = startWorker(data);
  try {
    await writeFile(path.join(data, 'provider.json'), JSON.stringify({
      provider: 'custom',
      model: 'local-reasoner',
      baseUrl: 'http://127.0.0.1:8080/v1',
      reasoning: true,
    }));
    await worker.waitReady();

    // Discovery is registry-only: no session is opened before this call.
    const models = await worker.request('custom', 'models.list', { provider: 'custom' });
    assert.equal(models.native, false);
    assert.equal(models.models.length, 1);
    assert.equal(models.models[0].id, 'local-reasoner');
    assert.equal(models.models[0].reasoning, true);
    assert.ok(models.models[0].thinkingLevels.includes('medium'));

    const opened = await worker.request('legacy-open', 'session.open');
    assert.equal(opened.error, undefined, opened.error);
    assert.equal(opened.provider, 'custom');
    assert.equal(opened.model, 'local-reasoner');

    const modelsFile = JSON.parse(await readFile(path.join(data, 'pi', 'models.json'), 'utf8'));
    assert.equal(modelsFile.$schema, 'https://pi.dev/schemas/models.schema.json');
    assert.equal(modelsFile.providers.custom.baseUrl, 'http://127.0.0.1:8080/v1');
    assert.equal(modelsFile.providers.custom.api, 'openai-completions');
    assert.equal(modelsFile.providers.custom.apiKey, 'talo-local');
    assert.equal(modelsFile.providers.custom.models[0].id, 'local-reasoner');
    assert.equal(modelsFile.providers.custom.models[0].reasoning, true);

    assert.equal(await worker.request('stop', 'shutdown'), true);
    assert.equal(await worker.exited, 0);
  } finally {
    await worker.kill();
    await rm(data, { recursive: true, force: true });
  }
});

test('provider validation authenticates and errors never contain credentials', { timeout: 120_000 }, async () => {
  const data = await mkdtemp(path.join(os.tmpdir(), 'talo-validate-'));
  const mock = await startMockProvider();
  const secret = mock.state.key;
  const worker = startWorker(data);
  try {
    await worker.waitReady();
    const result = await worker.request('ok', 'provider.validate', {
      provider: 'custom',
      model: 'mock-model',
      baseUrl: mock.url,
      apiKey: secret,
      reasoning: false,
    }, 40_000);
    assert.equal(result.authenticated, true);
    assert.equal(result.model, 'mock-model');
    assert.equal(result.provider, 'custom');
    assert.equal(result.native, false);

    mock.configure({ mode: 'unauthorized', errorMessage: `bad key ${secret}` });
    let failure;
    try {
      await worker.request('bad', 'provider.validate', {
        provider: 'custom',
        model: 'mock-model',
        baseUrl: mock.url,
        apiKey: secret,
        reasoning: false,
      }, 40_000);
    } catch (error) {
      failure = error;
    }
    assert.ok(failure, 'a rejected credential must fail validation');
    assert.ok(!failure.message.includes(secret), 'credential leaked into worker error');
    assert.match(failure.message, /\[redacted\]/);

    for (const file of await filesUnder(data)) {
      const content = await readFile(file, 'utf8').catch(() => '');
      assert.ok(!content.includes(secret), `credential leaked into ${path.relative(data, file)}`);
    }
    assert.ok(!JSON.stringify(worker.events).includes(secret));
    assert.ok(!worker.stderr().includes(secret));

    assert.equal(await worker.request('stop', 'shutdown'), true);
    assert.equal(await worker.exited, 0);
  } finally {
    await worker.kill();
    await mock.close();
    await rm(data, { recursive: true, force: true });
  }
});
