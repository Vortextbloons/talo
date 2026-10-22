import { createHash, randomBytes, randomUUID } from 'node:crypto';
import { createServer } from 'node:http';
import { createRemoteJWKSet, jwtVerify } from 'jose';
import type { ModelRuntime, ProviderModelConfig } from '@earendil-works/pi-coding-agent';
import { rememberSecret } from './providers.js';

export const CHATGPT_PROVIDER = 'chatgpt';
const issuer = 'https://auth.openai.com';
const resource = 'https://api.openai.com/v1';
const tokenEndpoint = `${issuer}/api/accounts/oauth/token`;
const planScope = 'chatgpt.tokens.use.direct';
const jwks = createRemoteJWKSet(new URL(`${issuer}/.well-known/jwks.json`));

export interface ChatGPTCredential {
  access: string;
  refresh: string;
  expires: number;
  clientId: string;
  subject: string;
  email: string;
  idToken: string;
  scopes: string[];
}

async function tokenRequest(body: URLSearchParams, signal?: AbortSignal): Promise<Record<string, unknown>> {
  const response = await fetch(tokenEndpoint, { method: 'POST', body,
    signal: AbortSignal.any([AbortSignal.timeout(25_000), ...(signal ? [signal] : [])]) });
  if (!response.ok) throw new Error('ChatGPT authorization expired or was declined. Connect your account again.');
  return await response.json() as Record<string, unknown>;
}

function tokenSet(data: Record<string, unknown>, previous?: ChatGPTCredential) {
  if (typeof data.access_token !== 'string' || !data.access_token ||
      !(typeof data.refresh_token === 'string' ? data.refresh_token : previous?.refresh) ||
      typeof data.expires_in !== 'number' || !Number.isFinite(data.expires_in) || data.expires_in <= 0 ||
      String(data.token_type).toLowerCase() !== 'bearer') throw new Error('ChatGPT returned an invalid token response.');
  const scopes = typeof data.scope === 'string' ? data.scope.split(/\s+/u) : previous?.scopes ?? [];
  if (!scopes.includes(planScope)) throw new Error('Allow Talo to use your ChatGPT plan, then try connecting again.');
  const access = data.access_token;
  const refresh = typeof data.refresh_token === 'string' ? data.refresh_token : previous!.refresh;
  rememberSecret(access);
  rememberSecret(refresh);
  return { access, refresh, expires: Date.now() + data.expires_in * 1000, scopes };
}

export async function refreshChatGPT(credential: ChatGPTCredential): Promise<ChatGPTCredential> {
  const data = await tokenRequest(new URLSearchParams({ grant_type: 'refresh_token', client_id: credential.clientId,
    refresh_token: credential.refresh, resource }));
  return { ...credential, ...tokenSet(data, credential) };
}

export async function revokeChatGPT(credential: ChatGPTCredential): Promise<boolean> {
  try {
    const discovery = await fetch(`${issuer}/.well-known/openid-configuration`, { signal: AbortSignal.timeout(10_000) });
    const config = await discovery.json() as { revocation_endpoint?: string };
    if (!config.revocation_endpoint || new URL(config.revocation_endpoint).origin !== issuer) return false;
    const response = await fetch(config.revocation_endpoint, { method: 'POST', signal: AbortSignal.timeout(10_000),
      body: new URLSearchParams({ token: credential.refresh, token_type_hint: 'refresh_token', client_id: credential.clientId }) });
    return response.status === 200;
  } catch { return false; }
}

type Attempt = { id: string; cancel: () => void; result?: ChatGPTCredential; error?: string };
let attempt: Attempt | undefined;

export async function beginChatGPT(hostId: string, previous?: ChatGPTCredential) {
  if (attempt) throw new Error('A ChatGPT sign-in is already in progress. Cancel it before starting another.');
  if (!/^urn:uuid:[0-9a-f-]{36}$/iu.test(hostId)) throw new Error('Invalid Talo installation identifier.');
  const state = randomBytes(32).toString('base64url');
  const nonce = randomBytes(32).toString('base64url');
  const verifier = randomBytes(32).toString('base64url');
  const controller = new AbortController();
  let consumed = false;
  let timer: ReturnType<typeof setTimeout>;
  const current: Attempt = { id: randomUUID(), cancel: () => { controller.abort(); clearTimeout(timer); server.close(); server.closeAllConnections(); } };
  const server = createServer((request, response) => {
    const callback = new URL(request.url ?? '/', 'http://127.0.0.1');
    response.setHeader('Content-Type', 'text/plain; charset=utf-8');
    response.setHeader('Cache-Control', 'no-store');
    if (request.method !== 'GET' || callback.pathname !== '/auth/callback' || callback.searchParams.get('state') !== state || consumed) {
      response.writeHead(400).end('Invalid sign-in callback. Return to Talo and try again.');
      return;
    }
    consumed = true;
    response.end('Return to Talo to finish connecting your ChatGPT account.');
    server.close();
    void (async () => {
      if (callback.searchParams.has('error')) throw new Error('ChatGPT sign-in was declined.');
      const clientId = callback.searchParams.get('client_id') ?? previous?.clientId;
      const code = callback.searchParams.get('code');
      if (!code || !clientId || clientId === 'dynamic_agent_client' || (previous && clientId !== previous.clientId)) {
        throw new Error('ChatGPT registration was incomplete. Try connecting again.');
      }
      const data = await tokenRequest(new URLSearchParams({ grant_type: 'authorization_code', client_id: clientId,
        code, code_verifier: verifier, redirect_uri: redirectUri, resource }), controller.signal);
      if (typeof data.id_token !== 'string') throw new Error('ChatGPT did not return an identity token.');
      rememberSecret(data.id_token);
      const { payload } = await jwtVerify(data.id_token, jwks, {
        issuer, audience: clientId, requiredClaims: ['sub', 'exp', 'iat'], clockTolerance: 5,
      });
      if (payload.nonce !== nonce || !payload.sub || (previous && payload.sub !== previous.subject)) {
        throw new Error('ChatGPT account identity did not match this sign-in.');
      }
      const tokens = tokenSet(data);
      if (!controller.signal.aborted) current.result = { ...tokens, clientId, subject: payload.sub,
        email: typeof payload.email === 'string' ? payload.email : '', idToken: data.id_token };
    })().catch(error => { current.error = error instanceof Error ? error.message : 'ChatGPT sign-in failed.'; });
  });
  attempt = current;
  try {
    await new Promise<void>((resolve, reject) => {
      server.once('error', reject);
      server.listen(0, '127.0.0.1', resolve);
    });
  } catch {
    attempt = undefined;
    current.cancel();
    throw new Error('Could not start the ChatGPT sign-in listener.');
  }
  const address = server.address();
  if (!address || typeof address === 'string') { server.close(); throw new Error('Could not start ChatGPT sign-in.'); }
  const redirectUri = `http://127.0.0.1:${address.port}/auth/callback`;
  timer = setTimeout(() => { current.cancel(); current.error = 'ChatGPT sign-in timed out. Try again.'; }, 5 * 60_000);
  timer.unref();
  const url = new URL(`${issuer}/api/accounts/authorize`);
  url.search = new URLSearchParams({ client_id: previous?.clientId ?? 'dynamic_agent_client',
    ...(previous ? (previous.idToken ? { id_token_hint: previous.idToken } : {}) : { agent_name_hint: 'Talo' }),
    ext_agent_host_id: hostId, response_type: 'code', redirect_uri: redirectUri,
    scope: `openid profile email offline_access resource.invoke ${planScope}`, resource, state, nonce,
    code_challenge_method: 'S256', code_challenge: createHash('sha256').update(verifier).digest('base64url') }).toString();
  return { attemptId: current.id, url: url.toString() };
}

export function pollChatGPT(id: string, cancel = false) {
  if (!attempt || attempt.id !== id) throw new Error('ChatGPT sign-in is no longer active. Try again.');
  const current = attempt;
  if (cancel || current.error || current.result) {
    current.cancel();
    attempt = undefined;
    if (cancel) return { pending: false };
    if (current.error) throw new Error(current.error);
    return { pending: false, credential: current.result };
  }
  return { pending: true };
}

export async function configureChatGPT(runtime: ModelRuntime, access: string) {
  const response = await fetch(`${resource}/models`, {
    headers: { Authorization: `Bearer ${access}` }, signal: AbortSignal.timeout(25_000),
  });
  if (!response.ok) throw new Error('Could not load models for your ChatGPT plan. Check account access and try again.');
  const data = await response.json() as { models?: { slug: string; display_name: string; visibility: string }[] };
  const models: ProviderModelConfig[] = [];
  for (const entry of data.models ?? []) {
    if (entry.visibility !== 'list') continue;
    const known = runtime.getModel('openai', entry.slug);
    if (known) models.push({ ...known, type: 'chat', id: entry.slug, name: entry.display_name,
      api: 'openai-responses', baseUrl: resource, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 } });
  }
  if (!models.length) throw new Error('No supported models are available for this ChatGPT account.');
  runtime.registerProvider('openai', { name: 'ChatGPT plan', api: 'openai-responses', baseUrl: resource, models });
}
