import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import path from 'node:path';
import type { AgentSession, ModelRuntime, ProviderModelConfig } from '@earendil-works/pi-coding-agent';
import type {
  AuthOperationOptions,
  Credential,
  CredentialInfo,
  CredentialStore,
  Model,
} from '@earendil-works/pi-ai';
import { extractText } from './messages.js';


const PREFERENCE_FILE = 'provider.json';

const MODELS_FILE = 'models.json';

const CUSTOM_PROVIDER = 'custom';
const CUSTOM_PROVIDER_NAME = 'Custom OpenAI-compatible';
const CUSTOM_API_KEY_ENV = 'TALO_CUSTOM_API_KEY';
const CUSTOM_LOCAL_KEY = 'talo-local';
const CUSTOM_CONTEXT_WINDOW = 128_000;
const CUSTOM_MAX_TOKENS = 16_384;
const PI_MODELS_SCHEMA = 'https://pi.dev/schemas/models.schema.json';
const PROVIDER_PROBE_TIMEOUT_MS = 25_000;
const PROVIDER_PROBE_MAX_TOKENS = 16;

const MAX_REMEMBERED_SECRETS = 32;

const rememberedSecrets = new Set<string>();


export function rememberSecret(secret: string | undefined): void {
  if (!secret || secret.length < 3) return;
  if (rememberedSecrets.size >= MAX_REMEMBERED_SECRETS) {
    const oldest = rememberedSecrets.values().next().value;
    if (oldest !== undefined) rememberedSecrets.delete(oldest);
  }
  rememberedSecrets.add(secret);
}


export function sanitizeError(error: unknown): string {
  let text = error instanceof Error
    ? error.message
    : typeof error === 'string'
      ? error
      : 'Unexpected worker error';
  for (const secret of rememberedSecrets) {
    if (secret.length >= 3) text = text.split(secret).join('[redacted]');
  }
  text = text
    .replace(/(?:authorization\s*[:=]\s*)(?:bearer\s+)?[^\s,;]+/gi, 'authorization: [redacted]')
    .replace(/([?&](?:api[_-]?key|key|token|access_token)=)[^&\s]+/gi, '$1[redacted]')
    .replace(/\b(?:sk|pk|rk|xai|AIza)[-_A-Za-z0-9]{8,}\b/g, '[redacted]')
    .replace(/\bbearer\s+[A-Za-z0-9._~+/=-]{8,}\b/gi, 'bearer [redacted]');
  text = text.replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/g, '').trim();
  return (text || 'Unexpected worker error').slice(0, 2000);
}


export interface ProviderPreference {
  provider: string;
  model: string;
  baseUrl: string;
  reasoning: boolean;
}

const DEFAULT_PREFERENCE: ProviderPreference = {
  provider: 'opencode',
  model: '',
  baseUrl: '',
  reasoning: false,
};

const PROVIDER_ID = /^[a-z0-9][a-z0-9._-]{0,63}$/i;
const SESSION_PATH_MAX = 4096;

const PROVIDER_ALIASES: Record<string, string> = { gemini: 'google', chatgpt: 'openai' };

export function canonicalProviderId(provider: string): string {
  return PROVIDER_ALIASES[provider] ?? provider;
}

export function normalizeProviderId(value: unknown, fallback?: string): string {
  if (value === undefined || value === null || value === '') {
    if (fallback !== undefined) return fallback;
    throw new Error('Provider is required');
  }
  if (typeof value !== 'string') throw new Error('Provider must be a string');
  const provider = value.trim();
  if (!PROVIDER_ID.test(provider)) throw new Error('Invalid provider');
  return provider;
}

export function normalizeModelId(value: unknown, fallback = ''): string {
  if (value === undefined || value === null || value === '') return fallback;
  if (typeof value !== 'string') throw new Error('Model must be a string');
  const model = value.trim();
  if (!model || model.length > 200) throw new Error('Invalid model');
  return model;
}

export function normalizeApiKey(value: unknown): string | undefined {
  if (value === undefined || value === null || value === '') return undefined;
  if (typeof value !== 'string') throw new Error('Credential must be a string');
  const key = value.trim();
  if (!key || key.length > 16_384) throw new Error('Invalid credential');
  return key;
}

export function normalizeBaseUrl(value: unknown): string | undefined {
  if (value === undefined || value === null || value === '') return undefined;
  if (typeof value !== 'string') throw new Error('Base URL must be a string');
  const raw = value.trim();
  if (!raw || raw.length > 2048) throw new Error('Invalid base URL');
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    throw new Error('Base URL must be an absolute http or https URL');
  }
  if (url.protocol !== 'http:' && url.protocol !== 'https:') {
    throw new Error('Base URL must be an absolute http or https URL');
  }
  if (url.username || url.password) throw new Error('Base URL must not contain credentials');
  return url.toString().replace(/\/+$/u, '');
}

export function normalizeReasoning(value: unknown, fallback = false): boolean {
  if (value === undefined || value === null) return fallback;
  if (typeof value !== 'boolean') throw new Error('Reasoning must be a boolean');
  return value;
}

function normalizePreference(value: unknown): ProviderPreference {
  const record = typeof value === 'object' && value !== null ? value as Record<string, unknown> : {};
  const rawProvider = typeof record.provider === 'string' ? record.provider.trim() : '';
  let baseUrl = '';
  try {
    baseUrl = normalizeBaseUrl(record.baseUrl) ?? '';
  } catch {
    // A malformed projection is ignored rather than redirected to an arbitrary endpoint.
  }
  return {
    provider: PROVIDER_ID.test(rawProvider) ? rawProvider : DEFAULT_PREFERENCE.provider,
    model: typeof record.model === 'string' ? record.model.trim().slice(0, 200) : '',
    baseUrl,
    reasoning: record.reasoning === true,
  };
}

export async function readPreference(dataDir: string): Promise<ProviderPreference> {
  try {
    return normalizePreference(JSON.parse(await readFile(path.join(dataDir, PREFERENCE_FILE), 'utf8')));
  } catch {
    return { ...DEFAULT_PREFERENCE };
  }
}

function httpUrl(value: string): boolean {
  try {
    const url = new URL(value);
    return url.protocol === 'http:' || url.protocol === 'https:';
  } catch {
    return false;
  }
}

function isLocalEndpoint(value: string): boolean {
  try {
    const host = new URL(value).hostname.toLowerCase();
    return host === 'localhost' || host === '127.0.0.1' || host === '0.0.0.0' ||
      host === '::1' || host === '[::1]' || host.endsWith('.localhost');
  } catch {
    return false;
  }
}

interface ModelsJsonDocument {
  $schema: string;
  providers: Record<string, {
    baseUrl?: string;
    api?: string;
    apiKey?: string;
    models?: Array<Record<string, unknown>>;
  }>;
}








function modelsJson(preference: ProviderPreference): ModelsJsonDocument {
  const empty: ModelsJsonDocument = { $schema: PI_MODELS_SCHEMA, providers: {} };
  if (preference.provider !== CUSTOM_PROVIDER) return empty;
  const model = preference.model.trim();
  const baseUrl = preference.baseUrl.trim();
  if (!model || !httpUrl(baseUrl)) return empty;
  return {
    $schema: PI_MODELS_SCHEMA,
    providers: {
      [CUSTOM_PROVIDER]: {
        baseUrl,
        api: 'openai-completions',
        apiKey: isLocalEndpoint(baseUrl) ? CUSTOM_LOCAL_KEY : `$${CUSTOM_API_KEY_ENV}`,
        models: [{
          id: model,
          name: model,
          reasoning: preference.reasoning,
          input: ['text'],
          contextWindow: CUSTOM_CONTEXT_WINDOW,
          maxTokens: CUSTOM_MAX_TOKENS,
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
        }],
      },
    },
  };
}


export async function syncModelsJson(agentDir: string, preference: ProviderPreference): Promise<boolean> {
  const target = path.join(agentDir, MODELS_FILE);
  const text = `${JSON.stringify(modelsJson(preference), null, 2)}\n`;
  try {
    if (await readFile(target, 'utf8') === text) return false;
  } catch {
    // Missing file: write it below.
  }
  await mkdir(agentDir, { recursive: true });
  const temporary = `${target}.tmp`;
  await writeFile(temporary, text);
  await rename(temporary, target);
  return true;
}


class MemoryCredentialStore implements CredentialStore {
  private readonly values = new Map<string, Credential>();

  async read(providerId: string, _options?: AuthOperationOptions): Promise<Credential | undefined> {
    return this.values.get(providerId);
  }

  async list(_options?: AuthOperationOptions): Promise<readonly CredentialInfo[]> {
    return [...this.values.entries()].map(([providerId, credential]) => ({
      providerId,
      type: credential.type,
    }));
  }

  async modify(
    providerId: string,
    fn: (current: Credential | undefined) => Promise<Credential | undefined>,
    _options?: AuthOperationOptions,
  ): Promise<Credential | undefined> {
    const next = await fn(this.values.get(providerId));
    if (next === undefined) return this.values.get(providerId);
    this.values.set(providerId, next);
    return next;
  }

  async delete(providerId: string, _options?: AuthOperationOptions): Promise<void> {
    this.values.delete(providerId);
  }
}


export async function createModelRuntime(agentDir: string): Promise<ModelRuntime> {
  const { ModelRuntime } = await import('@earendil-works/pi-coding-agent');
  return ModelRuntime.create({
    credentials: new MemoryCredentialStore(),
    modelsPath: path.join(agentDir, MODELS_FILE),
    modelsStorePath: path.join(agentDir, 'models-store.json'),
    allowModelNetwork: false,
    refreshOnCreate: false,
  });
}

interface ProviderRegistration {
  provider: string;
  model?: string;
  baseUrl?: string;
  reasoning?: boolean;
  name?: string;
}

function customModel(modelId: string, reasoning: boolean): ProviderModelConfig {
  return {
    type: 'chat',
    id: modelId,
    name: modelId,
    reasoning,
    input: ['text'],
    contextWindow: CUSTOM_CONTEXT_WINDOW,
    maxTokens: CUSTOM_MAX_TOKENS,
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
  };
}






export function registerProviderEndpoint(runtime: ModelRuntime, registration: ProviderRegistration): void {
  if (registration.provider === 'chatgpt') return;
  const provider = canonicalProviderId(registration.provider);
  const baseUrl = registration.baseUrl;
  const existing = runtime.getProvider(provider);
  const customLike = provider === CUSTOM_PROVIDER || !existing;
  if (!baseUrl) return;
  if (!customLike) {

    runtime.registerProvider(provider, { ...(registration.name ? { name: registration.name } : {}), baseUrl });
    return;
  }
  const model = registration.model?.trim();
  runtime.registerProvider(provider, {
    name: registration.name || (provider === CUSTOM_PROVIDER ? CUSTOM_PROVIDER_NAME : provider),
    baseUrl,
    api: 'openai-completions',
    ...(model ? { models: [customModel(model, registration.reasoning === true)] } : {}),
  });
}


export function effectiveApiKey(apiKey: string | undefined, provider: string, baseUrl: string | undefined): string | undefined {
  if (apiKey) return apiKey;
  if (canonicalProviderId(provider) === CUSTOM_PROVIDER && baseUrl && isLocalEndpoint(baseUrl)) return CUSTOM_LOCAL_KEY;
  return undefined;
}


export async function setProviderKey(runtime: ModelRuntime, provider: string, apiKey: string | undefined): Promise<void> {
  if (!apiKey) return;
  rememberSecret(apiKey);
  await runtime.setRuntimeApiKey(canonicalProviderId(provider), apiKey);
}

function providerName(runtime: ModelRuntime, provider: string): string {
  const runtimeProvider = canonicalProviderId(provider);
  return runtime.getProvider(runtimeProvider)?.name ||
    (runtimeProvider === CUSTOM_PROVIDER ? CUSTOM_PROVIDER_NAME : provider);
}

function isNativeProvider(runtime: ModelRuntime, provider: string): boolean {
  const runtimeProvider = canonicalProviderId(provider);
  if (runtimeProvider === CUSTOM_PROVIDER) return false;
  if (runtime.getRegisteredProviderConfig(runtimeProvider) !== undefined) return false;
  if (runtime.getRegisteredProviderIds().includes(runtimeProvider)) return false;
  return runtime.getProvider(runtimeProvider) !== undefined;
}

interface ProviderModelDescription {
  id: string;
  name: string;
  reasoning: boolean;
  thinkingLevels: string[];
  contextWindow: number;
  maxTokens: number;
}

interface ProviderModelList {
  provider: string;
  name: string;
  native: boolean;
  models: ProviderModelDescription[];
}


async function describeModels(runtime: ModelRuntime, provider: string): Promise<ProviderModelDescription[]> {
  const { getSupportedThinkingLevels } = await import('@earendil-works/pi-ai');
  return runtime.getModels(canonicalProviderId(provider)).map(model => ({
    id: model.id,
    name: model.name,
    reasoning: model.reasoning === true,
    thinkingLevels: getSupportedThinkingLevels(model).map(level => String(level)),
    contextWindow: model.contextWindow,
    maxTokens: model.maxTokens,
    input: Array.isArray(model.input) ? model.input.map(value => String(value)) : ['text'],
  }));
}

export async function listProviderModels(runtime: ModelRuntime, provider: string): Promise<ProviderModelList> {
  return {
    provider,
    name: providerName(runtime, provider),
    native: isNativeProvider(runtime, provider),
    models: await describeModels(runtime, provider),
  };
}

export function resolveModel(runtime: ModelRuntime, provider: string, modelId: string | undefined): Model<any> {
  const runtimeProvider = canonicalProviderId(provider);
  let model: Model<any> | undefined;
  if (modelId) {
    model = runtime.getModel(runtimeProvider, modelId);
  } else {
    const models = runtime.getModels(runtimeProvider);


    const skip = /deep-research|computer-use|(?:^|[-/])(?:image|embedding|tts|whisper|audio|video|realtime|classifier)(?:$|[-/])/i;
    const affordable = /(?:^|[-/])(?:mini|flash|haiku|auto)(?:$|[-/])/i;
    model = models.find(candidate => affordable.test(candidate.id) && !skip.test(candidate.id)) ??
      models.find(candidate => !skip.test(candidate.id)) ??
      models[0];
  }
  if (!model) {
    if (modelId) throw new Error(`Unknown model: ${provider}/${modelId}`);
    if (!runtime.getProvider(runtimeProvider)) throw new Error(`Unknown provider: ${provider}`);
    throw new Error(`No models are available for provider: ${provider}`);
  }
  return model;
}

interface ProviderValidation {
  provider: string;
  name: string;
  native: boolean;
  model: string;
  authenticated: true;
  latencyMs: number;
}

async function prepareProvider(
  runtime: ModelRuntime,
  provider: string,
  modelId: string | undefined,
  apiKey: string | undefined,
  baseUrl: string | undefined,
  reasoning: boolean,
): Promise<{ model: Model<any>; key: string | undefined }> {
  registerProviderEndpoint(runtime, { provider, model: modelId, baseUrl, reasoning });
  const model = resolveModel(runtime, provider, modelId);
  const key = effectiveApiKey(apiKey, provider, baseUrl || model.baseUrl);
  if (key) rememberSecret(key);
  if (!key && !(await runtime.getAuth(model))) {
    throw new Error(`Provider is not configured: ${provider}`);
  }
  return { model, key };
}





export async function validateProvider(
  runtime: ModelRuntime,
  provider: string,
  modelId: string | undefined,
  apiKey: string | undefined,
  baseUrl: string | undefined,
  reasoning: boolean,
): Promise<ProviderValidation> {
  const { model, key } = await prepareProvider(runtime, provider, modelId, apiKey, baseUrl, reasoning);
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), PROVIDER_PROBE_TIMEOUT_MS);
  const startedAt = Date.now();
  try {
    const message = await runtime.completeSimple(model, {
      messages: [{ role: 'user', content: 'Reply with OK.', timestamp: Date.now() }],
    }, {
      ...(key ? { apiKey: key } : {}),
      maxTokens: PROVIDER_PROBE_MAX_TOKENS,
      signal: controller.signal,
      sessionId: 'talo-provider-check',
    });
    if (message.stopReason === 'error') {
      throw new Error(message.errorMessage || 'Provider rejected the authentication check');
    }
    if (message.stopReason === 'aborted') {
      throw new Error(controller.signal.aborted
        ? 'Provider validation timed out'
        : 'Provider validation was cancelled');
    }
    return {
      provider,
      name: providerName(runtime, provider),
      native: isNativeProvider(runtime, provider),
      model: model.id,
      authenticated: true,
      latencyMs: Date.now() - startedAt,
    };
  } finally {
    clearTimeout(timer);
  }
}

const THEME_TIMEOUT_MS = 60_000;
const THEME_MAX_TOKENS = 2_000;
const THEME_SURFACES = new Set<string>([
  '--bg', '--paper', '--soft', '--sidebar', '--text', '--muted', '--faint', '--line', '--line-strong', '--black',
]);

const THEME_PROMPT = [
  'You design a color theme for Talo, a dark desktop AI assistant. Answer with ONLY a JSON object - no markdown fences, no commentary - shaped exactly like:',
  '{"name":"Theme name","accent":"#rrggbb","surfaces":{"--bg":"#rrggbb","--paper":"#rrggbb","--soft":"#rrggbb","--sidebar":"#rrggbb","--text":"#rrggbb","--muted":"#rrggbb","--faint":"#rrggbb","--line":"#rrggbb","--line-strong":"#rrggbb","--black":"#rrggbb"}}',
  'The accent tints buttons, highlights and focus rings and carries dark text, so keep it bright enough for near-black text on top. Surfaces go darkest at --bg and --sidebar, raised at --paper and --soft; --text is the brightest, --muted is softer, --faint is the dimmest; --line and --line-strong sit between the surfaces; --black is the darkest ink, used on accent buttons. Keep every color in one coherent palette that honors the request. Omit "surfaces" only when the request asks for a plain accent tint.',
].join('\n');

export interface ThemeDraft { name: string; accent: string; surfaces?: Record<string, string> }

function normalizeHexColor(value: unknown): string | undefined {
  if (typeof value !== 'string') return undefined;
  const text = value.trim().toLowerCase();
  const short = /^#([0-9a-f]{3})$/u.exec(text);
  if (short) return `#${short[1].split('').map(character => character + character).join('')}`;
  return /^#[0-9a-f]{6}$/u.test(text) ? text : undefined;
}

function extractJsonObject(text: string): unknown {
  const fenced = text.match(/```(?:json)?\s*([\s\S]*?)```/i);
  const source = fenced ? fenced[1] : text;
  const start = source.indexOf('{');
  const end = source.lastIndexOf('}');
  if (start === -1 || end <= start) throw new Error('The model did not return a theme');
  return JSON.parse(source.slice(start, end + 1));
}

function normalizeThemeDraft(value: unknown): ThemeDraft {
  if (typeof value !== 'object' || value === null) throw new Error('The model did not return a theme');
  const source = value as Record<string, unknown>;
  const accent = normalizeHexColor(source.accent);
  if (!accent) throw new Error('The model returned an invalid theme color');
  const name = typeof source.name === 'string' ? source.name.replace(/\s+/gu, ' ').trim() : '';
  const surfaces: Record<string, string> = {};
  if (typeof source.surfaces === 'object' && source.surfaces !== null) {
    for (const [key, color] of Object.entries(source.surfaces as Record<string, unknown>)) {
      const normalized = normalizeHexColor(color);
      if (THEME_SURFACES.has(key) && normalized) surfaces[key] = normalized;
    }
  }
  return {
    name: (name || 'AI theme').slice(0, 24),
    accent,
    ...(Object.keys(surfaces).length ? { surfaces } : {}),
  };
}

export async function generateTheme(
  runtime: ModelRuntime,
  provider: string,
  modelId: string | undefined,
  apiKey: string | undefined,
  baseUrl: string | undefined,
  reasoning: boolean,
  description: string,
): Promise<ThemeDraft> {
  const { model, key } = await prepareProvider(runtime, provider, modelId, apiKey, baseUrl, reasoning);
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), THEME_TIMEOUT_MS);
  try {
    const message = await runtime.completeSimple(model, {
      messages: [{ role: 'user', content: `${THEME_PROMPT}\n\nTheme request: ${description}`, timestamp: Date.now() }],
    }, {
      ...(key ? { apiKey: key } : {}),
      maxTokens: THEME_MAX_TOKENS,
      signal: controller.signal,
      sessionId: 'talo-theme-draft',
    });
    if (message.stopReason === 'error') {
      throw new Error(message.errorMessage || 'The theme request failed');
    }
    if (message.stopReason === 'aborted') {
      throw new Error(controller.signal.aborted
        ? 'The theme request timed out'
        : 'The theme request was cancelled');
    }
    const text = extractText(message).trim();
    if (!text) throw new Error('The model returned an empty theme');
    return normalizeThemeDraft(extractJsonObject(text));
  } finally {
    clearTimeout(timer);
  }
}

export function resolveSessionPath(value: unknown, sessionDir: string): string | undefined {
  if (value === undefined || value === null || value === '') return undefined;
  if (typeof value !== 'string') throw new Error('Session path must be a string or null');
  const raw = value.trim();
  if (!raw || raw.length > SESSION_PATH_MAX) throw new Error('Invalid session path');
  const resolved = path.resolve(raw);
  const relative = path.relative(path.resolve(sessionDir), resolved);
  if (!relative || relative.startsWith('..') || path.isAbsolute(relative)) {
    throw new Error('Session path must be inside Talo data');
  }
  if (path.extname(resolved).toLowerCase() !== '.jsonl') {
    throw new Error('Session path must be a Pi session file');
  }
  return resolved;
}


export function sessionFilePath(session: AgentSession): string | null {
  return session.sessionFile ?? session.sessionManager.getSessionFile() ?? null;
}

export { CUSTOM_PROVIDER };
