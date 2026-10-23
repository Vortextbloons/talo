import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { stripTypeScriptTypes } from 'node:module';

let counter = 0;
async function store(check, invoke = async () => {}) {
  const key = `__updaterTest${counter++}`;
  globalThis[key] = { check, invoke };
  const source = (await readFile(new URL('../apps/ui/src/lib/updates.svelte.ts', import.meta.url), 'utf8'))
    .replace(/^import[^\n]+\r?\n/gm, '');
  const code = stripTypeScriptTypes(`const { check, invoke } = globalThis.${key};
    const isTauri = () => true;
    const $state = value => value;
    ${source}
  `);
  const module = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
  delete globalThis[key];
  return module;
}

test('failed update downloads leave running work untouched', async () => {
  const calls = [];
  const fail = async () => { calls.push('download'); throw new Error('network connection failed'); };
  const api = await store(async () => ({ body: 'Update', download: fail, downloadAndInstall: fail }), async command => calls.push(command));
  await api.checkForUpdates();
  await api.installUpdate();
  assert.deepEqual(calls, ['download']);
  assert.equal(api.updates.phase, 'error');
});

test('the verified update downloads before work stops and installation begins', async () => {
  const calls = [];
  const update = {
    async download(onEvent) {
      calls.push('download');
      onEvent({ event: 'Started', data: { contentLength: 10 } });
      onEvent({ event: 'Progress', data: { chunkLength: 10 } });
      onEvent({ event: 'Finished' });
    },
    async install() { calls.push('install'); },
  };
  const api = await store(async () => update, async command => calls.push(command));
  await api.checkForUpdates();
  await api.installUpdate();
  assert.deepEqual(calls, ['download', 'prepare_update', 'install']);
  assert.equal(api.updates.phase, 'installing');
  assert.equal(api.updates.received, 10);
});

test('missing release feeds are distinguished from internet connection failures', async () => {
  const api = await store(async () => { throw new Error('Could not fetch a valid release JSON from the remote'); });
  await api.checkForUpdates();
  assert.equal(api.updates.phase, 'error');
  assert.match(api.updates.error, /update files.*unavailable/i);
  assert.doesNotMatch(api.updates.error, /check your connection/i);
});

test('signature errors take precedence over generic request errors', async () => {
  const api = await store(async () => { throw new Error('update request signature verification failed'); });
  await api.checkForUpdates();
  assert.match(api.updates.error, /signature check/i);
});
