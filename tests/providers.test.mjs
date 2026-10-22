import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { createInterface } from 'node:readline';
import {
  CUSTOM_PROVIDER,
  PROVIDERS,
  isLocalEndpoint,
  modelsJson,
  parseProviderPreference,
  providerDescriptor,
} from '../vendor/bundle/packages/contracts/src/providers.js';

test('provider catalog points at Pi provider IDs and documented endpoints', () => {
  assert.deepEqual(PROVIDERS.map(provider => provider.id), ['opencode', 'opencode-go', 'openrouter', 'nvidia', 'custom']);
  const byId = Object.fromEntries(PROVIDERS.map(provider => [provider.id, provider]));
  assert.equal(byId.opencode.apiBaseUrl, 'https://opencode.ai/inference');
  assert.equal(byId['opencode-go'].apiBaseUrl, 'https://opencode.ai/inference/go');
  assert.equal(byId.openrouter.apiBaseUrl, 'https://openrouter.ai/api/v1');
  assert.equal(byId.nvidia.apiBaseUrl, 'https://integrate.api.nvidia.com/v1');
  assert.equal(byId.opencode.keysUrl, 'https://opencode.ai/auth');
  assert.equal(byId.openrouter.keysUrl, 'https://openrouter.ai/keys');
  assert.equal(byId.nvidia.keysUrl, 'https://build.nvidia.com/settings/api-keys');
  for (const provider of PROVIDERS.filter(item => item.native)) {
    assert.equal(provider.reasoning, true, provider.id);
    assert.ok(provider.models.length > 0, provider.id);
    assert.ok(provider.models.every(model => model.reasoning), `${provider.id} presets support reasoning`);
    assert.match(provider.apiBaseUrl, /^https:\/\//);
    assert.match(provider.keysUrl, /^https:\/\//);
  }
  assert.equal(new Set(PROVIDERS.flatMap(provider => provider.models.map(model => `${provider.id}/${model.id}`))).size,
    PROVIDERS.reduce((count, provider) => count + provider.models.length, 0));
});

test('provider preference normalizes legacy settings and clamps values', () => {
  assert.deepEqual(parseProviderPreference(null), { provider: 'opencode', model: '', baseUrl: '', reasoning: false });
  assert.deepEqual(parseProviderPreference({ provider: 'Anthropic', model: '  claude  ', baseUrl: ' https://example.com/v1 ', reasoning: 'yes' }),
    { provider: 'opencode', model: 'claude', baseUrl: 'https://example.com/v1', reasoning: false });
  assert.equal(parseProviderPreference({ provider: 'openrouter', model: 'x'.repeat(500) }).model.length, 200);
  assert.equal(providerDescriptor('removed-provider').id, 'opencode');
});

test('models.json keeps native Pi providers intact and declares custom reasoning', () => {
  for (const provider of PROVIDERS.filter(item => item.native)) {
    const document = modelsJson({ provider: provider.id, model: 'any-model', baseUrl: '', reasoning: true });
    assert.deepEqual(document.providers, {}, `${provider.id} must not replace Pi's catalog`);
  }
  const local = modelsJson({ provider: CUSTOM_PROVIDER, model: 'llama3.3', baseUrl: 'http://127.0.0.1:11434/v1', reasoning: true });
  assert.equal(local.$schema, 'https://pi.dev/schemas/models.schema.json');
  assert.equal(local.providers.custom.api, 'openai-completions');
  assert.equal(local.providers.custom.apiKey, 'talo-local');
  assert.equal(local.providers.custom.models[0].id, 'llama3.3');
  assert.equal(local.providers.custom.models[0].reasoning, true);
  const hosted = modelsJson({ provider: CUSTOM_PROVIDER, model: 'reasoner', baseUrl: 'https://models.example.com/v1', reasoning: false });
  assert.equal(hosted.providers.custom.apiKey, '$TALO_CUSTOM_API_KEY');
  assert.equal(hosted.providers.custom.models[0].reasoning, false);
  assert.deepEqual(modelsJson({ provider: CUSTOM_PROVIDER, model: 'x', baseUrl: 'not a url', reasoning: true }).providers, {});
  assert.deepEqual(modelsJson({ provider: CUSTOM_PROVIDER, model: '', baseUrl: 'http://localhost:1234/v1', reasoning: true }).providers, {});
  assert.equal(isLocalEndpoint('http://[::1]:8080/v1'), true);
  assert.equal(isLocalEndpoint('https://openrouter.ai/api/v1'), false);
});

function worker(data) {
  const child = spawn(path.resolve('vendor/runtime/node/node.exe'), [path.resolve('vendor/bundle/workers/pi/src/main.js')], {
    env: { ...process.env, TALO_DATA_DIR: data },
    stdio: ['pipe', 'pipe', 'pipe'],
    windowsHide: true,
  });
  const events = [];
  let stderr = '';
  child.stderr.on('data', data => stderr += data);
  createInterface({ input: child.stdout }).on('line', line => {
    try { events.push(JSON.parse(line)); } catch { /* Ignore malformed output. */ }
  });
  async function wait(predicate, timeout = 30000) {
    const end = Date.now() + timeout;
    while (Date.now() < end) {
      const event = events.find(predicate);
      if (event) return event;
      if (child.exitCode !== null) throw new Error(`Worker exited ${child.exitCode}: ${stderr}`);
      await new Promise(resolve => setTimeout(resolve, 25));
    }
    throw new Error(`Worker timeout: ${stderr}`);
  }
  return { child, wait, send: request => child.stdin.write(`${JSON.stringify(request)}\n`) };
}

test('Pi registry reports reasoning levels for hosted and custom providers', { timeout: 90000 }, async () => {
  const data = await mkdtemp(path.join(os.tmpdir(), 'talo-providers-'));
  const w = worker(data);
  try {
    await writeFile(path.join(data, 'provider.json'), JSON.stringify({
      provider: 'custom',
      model: 'local-reasoner',
      baseUrl: 'http://127.0.0.1:8080/v1',
      reasoning: true,
    }));
    await w.wait(event => event.type === 'ready');
    w.send({ id: 'open', method: 'session.open' });
    const opened = await w.wait(event => event.id === 'open');
    assert.equal(opened.error, undefined, opened.error);
    assert.equal(opened.result.provider, 'custom');

    // Talo writes models.json before the session exists, because Pi composes its registry at creation.
    const modelsFile = JSON.parse(await readFile(path.join(data, 'pi', 'models.json'), 'utf8'));
    assert.equal(modelsFile.providers.custom.baseUrl, 'http://127.0.0.1:8080/v1');
    assert.equal(modelsFile.providers.custom.models[0].reasoning, true);

    w.send({ id: 'custom', method: 'models.list', params: { provider: 'custom' } });
    const custom = await w.wait(event => event.id === 'custom');
    assert.equal(custom.result.models.length, 1);
    assert.equal(custom.result.models[0].id, 'local-reasoner');
    assert.equal(custom.result.models[0].reasoning, true);
    assert.ok(custom.result.models[0].thinkingLevels.includes('medium'));

    // Native providers come from Pi's catalog with provider-specific reasoning metadata.
    w.send({ id: 'zen', method: 'models.list', params: { provider: 'opencode' } });
    const zen = await w.wait(event => event.id === 'zen');
    assert.equal(zen.result.native, true);
    assert.ok(zen.result.models.length > 0);
    assert.ok(zen.result.models.some(model => model.reasoning && model.thinkingLevels.includes('high')));

    w.send({ id: 'nim', method: 'models.list', params: { provider: 'nvidia' } });
    const nim = await w.wait(event => event.id === 'nim');
    assert.ok(nim.result.models.some(model => model.reasoning));

    w.send({ id: 'unknown', method: 'models.list', params: { provider: 'anthropic' } });
    assert.match((await w.wait(event => event.id === 'unknown')).error, /Unknown provider/);

    w.send({ id: 'stop', method: 'shutdown' });
  } finally {
    w.child.kill();
    await rm(data, { recursive: true, force: true });
  }
});
