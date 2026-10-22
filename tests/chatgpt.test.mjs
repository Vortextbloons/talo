import { test } from 'node:test';
import assert from 'node:assert/strict';
import { generateKeyPair, exportJWK, SignJWT } from 'jose';
import { mkdtemp, readdir, readFile, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { beginChatGPT, pollChatGPT, refreshChatGPT, configureChatGPT } from '../dist/workers/pi/src/chatgpt.js';
import { createModelRuntime, setProviderKey, resolveModel, registerProviderEndpoint } from '../dist/workers/pi/src/providers.js';

test('ChatGPT sign-in validates callbacks and identity, renews the issued registration, and isolates cancellation', async () => {
  const originalFetch = globalThis.fetch;
  const { privateKey, publicKey } = await generateKeyPair('RS256');
  const jwk = { ...await exportJWK(publicKey), kid: 'talo-test', alg: 'RS256', use: 'sig' };
  let nonce = '';
  let invalidNonce = false;
  let grantScope = 'openid chatgpt.tokens.use.direct offline_access';
  const requests = [];
  globalThis.fetch = async (input, options) => {
    const url = String(input);
    if (url.startsWith('http://127.0.0.1:')) return originalFetch(input, options);
    if (url.endsWith('/.well-known/jwks.json')) return Response.json({ keys: [jwk] });
    assert.equal(url, 'https://auth.openai.com/api/accounts/oauth/token');
    const body = new URLSearchParams(options.body);
    requests.push(body);
    assert.equal(body.get('client_id'), 'oaiapp_test');
    assert.equal(body.get('resource'), 'https://api.openai.com/v1');
    const idToken = await new SignJWT({ nonce: invalidNonce ? 'wrong' : nonce, email: 'person@example.com' })
      .setProtectedHeader({ alg: 'RS256', kid: 'talo-test' }).setIssuer('https://auth.openai.com')
      .setAudience('oaiapp_test').setSubject('person').setIssuedAt().setExpirationTime('1h').sign(privateKey);
    return Response.json({ access_token: 'test-access', refresh_token: 'test-refresh', expires_in: 3600,
      token_type: 'Bearer', scope: grantScope, id_token: idToken });
  };
  async function finish(attemptId) {
    for (let i = 0; i < 100; i++) {
      const result = pollChatGPT(attemptId);
      if (!result.pending) return result.credential;
      await new Promise(resolve => setTimeout(resolve, 10));
    }
    assert.fail('Sign-in did not complete');
  }
  try {
    const first = await beginChatGPT('urn:uuid:11111111-1111-4111-8111-111111111111');
    const auth = new URL(first.url);
    await assert.rejects(() => beginChatGPT('urn:uuid:11111111-1111-4111-8111-111111111111'), /already in progress/);
    assert.equal(auth.searchParams.get('agent_name_hint'), 'Talo');
    assert.equal(auth.searchParams.get('client_id'), 'dynamic_agent_client');
    nonce = auth.searchParams.get('nonce');
    const callback = new URL(auth.searchParams.get('redirect_uri'));
    callback.search = new URLSearchParams({ code: 'test-code', state: 'wrong', client_id: 'oaiapp_test' }).toString();
    assert.equal((await fetch(callback)).status, 400);
    assert.equal(requests.length, 0);
    assert.equal(pollChatGPT(first.attemptId).pending, true);
    callback.searchParams.set('state', auth.searchParams.get('state'));
    assert.equal((await fetch(callback)).status, 200);
    const credential = await finish(first.attemptId);
    assert.equal(credential.email, 'person@example.com');
    assert.equal(credential.subject, 'person');
    assert.equal(credential.clientId, 'oaiapp_test');
    assert.equal(requests[0].get('redirect_uri'), auth.searchParams.get('redirect_uri'));
    const renewed = await refreshChatGPT(credential);
    assert.equal(renewed.subject, credential.subject);
    assert.equal(requests[1].get('grant_type'), 'refresh_token');
    const returning = await beginChatGPT('urn:uuid:11111111-1111-4111-8111-111111111111', credential);
    assert.equal(new URL(returning.url).searchParams.get('client_id'), 'oaiapp_test');
    assert.equal(new URL(returning.url).searchParams.get('agent_name_hint'), null);
    assert.throws(() => pollChatGPT('another-attempt', true), /no longer active/);
    assert.equal(pollChatGPT(returning.attemptId).pending, true);
    pollChatGPT(returning.attemptId, true);
    const rejected = await beginChatGPT('urn:uuid:11111111-1111-4111-8111-111111111111');
    const rejectedAuth = new URL(rejected.url);
    nonce = rejectedAuth.searchParams.get('nonce');
    invalidNonce = true;
    const rejectedCallback = new URL(rejectedAuth.searchParams.get('redirect_uri'));
    rejectedCallback.search = new URLSearchParams({ code: 'test-code', state: rejectedAuth.searchParams.get('state'), client_id: 'oaiapp_test' }).toString();
    await fetch(rejectedCallback);
    await assert.rejects(() => finish(rejected.attemptId), /identity did not match/);
    invalidNonce = false;
    grantScope = 'openid offline_access';
    const unapproved = await beginChatGPT('urn:uuid:11111111-1111-4111-8111-111111111111');
    const unapprovedAuth = new URL(unapproved.url);
    nonce = unapprovedAuth.searchParams.get('nonce');
    const unapprovedCallback = new URL(unapprovedAuth.searchParams.get('redirect_uri'));
    unapprovedCallback.search = new URLSearchParams({ code: 'test-code', state: unapprovedAuth.searchParams.get('state'), client_id: 'oaiapp_test' }).toString();
    await fetch(unapprovedCallback);
    await assert.rejects(() => finish(unapproved.attemptId), /Allow Talo to use your ChatGPT plan/);
  } finally { globalThis.fetch = originalFetch; }
});

test('Pi streams ChatGPT plan requests with tools, no response storage, and no API key fallback', async () => {
  const originalFetch = globalThis.fetch;
  const directory = await mkdtemp(path.join(os.tmpdir(), 'talo-chatgpt-'));
  let fail = false;
  const access = 'plan-access-token-for-test';
  const item = { id: 'message-1', type: 'message', role: 'assistant', status: 'completed',
    content: [{ type: 'output_text', text: 'OK', annotations: [] }] };
  globalThis.fetch = async (input, options) => {
    const url = String(input);
    assert.equal(new Headers(options.headers).get('authorization'), `Bearer ${access}`);
    if (url.endsWith('/models')) return Response.json({ models: [{ slug: 'gpt-4.1', display_name: 'GPT-4.1', visibility: 'list' }] });
    assert.equal(url, 'https://api.openai.com/v1/responses');
    const body = JSON.parse(options.body);
    assert.equal(body.store, false);
    assert.equal(body.stream, true);
    assert.equal(body.tools[0].name, 'lookup');
    const events = fail ? [{ type: 'response.failed', response: { status: 'failed',
      error: { code: 'subscription_sharing_usage_limit_exceeded', message: 'Plan limit reached' } } }] : [
      { type: 'response.created', response: { id: 'response-1' } },
      { type: 'response.output_item.added', output_index: 0, item: { ...item, content: [] } },
      { type: 'response.output_text.delta', output_index: 0, content_index: 0, delta: 'OK' },
      { type: 'response.output_item.done', output_index: 0, item },
      { type: 'response.completed', response: { id: 'response-1', status: 'completed', output: [item],
        usage: { input_tokens: 5, output_tokens: 1, total_tokens: 6 } } },
    ];
    return new Response(events.map(event => `event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`).join(''),
      { headers: { 'content-type': 'text/event-stream' } });
  };
  try {
    const runtime = await createModelRuntime(directory);
    await configureChatGPT(runtime, access);
    registerProviderEndpoint(runtime, { provider: 'chatgpt', baseUrl: 'https://another-endpoint.example/v1' });
    await setProviderKey(runtime, 'chatgpt', access);
    const model = resolveModel(runtime, 'chatgpt', 'gpt-4.1');
    assert.equal(model.provider, 'openai');
    const context = { messages: [{ role: 'user', content: 'Hello', timestamp: Date.now() }],
      tools: [{ name: 'lookup', description: 'Look up an item', parameters: { type: 'object', properties: {} } }] };
    const completed = await runtime.completeSimple(model, context);
    assert.equal(completed.stopReason, 'stop');
    assert.equal(completed.content[0].text, 'OK');
    fail = true;
    const failed = await runtime.completeSimple(model, context);
    assert.equal(failed.stopReason, 'error');
    assert.match(failed.errorMessage, /usage.limit|Plan limit/i);
    for (const file of (await readdir(directory)).filter(file => file.endsWith('.json'))) {
      assert.ok(!(await readFile(path.join(directory, file), 'utf8')).includes(access));
    }
  } finally { globalThis.fetch = originalFetch; await rm(directory, { recursive: true, force: true }); }
});

test('ChatGPT uses only account-visible models with Pi metadata and the public Responses endpoint', async () => {
  const originalFetch = globalThis.fetch;
  let registration;
  globalThis.fetch = async (url, options) => {
    assert.equal(url, 'https://api.openai.com/v1/models');
    assert.equal(options.headers.Authorization, 'Bearer account-token');
    return Response.json({ models: [
      { slug: 'visible', display_name: 'Visible model', visibility: 'list' },
      { slug: 'hidden', display_name: 'Hidden model', visibility: 'hidden' },
    ] });
  };
  try {
    await configureChatGPT({
      getModel: (_, id) => ({ id, contextWindow: 100_000, maxTokens: 10_000, input: ['text'], reasoning: true }),
      registerProvider: (id, config) => { registration = { id, config }; },
    }, 'account-token');
    assert.equal(registration.id, 'openai');
    assert.deepEqual(registration.config.models.map(model => model.id), ['visible']);
    assert.equal(registration.config.models[0].api, 'openai-responses');
    assert.equal(registration.config.models[0].baseUrl, 'https://api.openai.com/v1');
  } finally { globalThis.fetch = originalFetch; }
});
