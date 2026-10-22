/**
 * Model providers Talo exposes.
 *
 * Hosted providers use the provider IDs Pi already ships (`opencode` = OpenCode Zen,
 * `opencode-go` = OpenCode Go, `openrouter`, `nvidia`). Keeping the native IDs means Pi's
 * bundled catalog keeps supplying each model's endpoint, limits, compatibility flags and
 * reasoning/thinking metadata. Talo must not redeclare those providers in models.json,
 * because a `models` list there replaces the built-in list.
 *
 * `custom` is the only provider Talo writes to Pi's models.json. Its model is declared with
 * `reasoning` so Pi's registry exposes thinking levels for OpenAI-compatible endpoints.
 */
export type ProviderId = 'opencode' | 'opencode-go' | 'openrouter' | 'nvidia' | 'custom';
export type ProviderApi = 'openai-completions' | 'anthropic-messages' | 'openai-responses' | 'google-generative-ai' | 'mixed';

/** Suggested model IDs. Pi's registry stays authoritative for the live list and capabilities. */
export interface ProviderModelPreset { id: string; name: string; reasoning: boolean }
export interface ProviderDescriptor {
  id: ProviderId;
  name: string;
  tagline: string;
  /** Endpoint Pi calls. Native providers take each model's endpoint from the bundled catalog. */
  apiBaseUrl: string;
  api: ProviderApi;
  /** Environment variable Pi reads when no credential is stored. */
  envVar: string;
  /** Where to create an API key. Empty when the endpoint is user supplied. */
  keysUrl: string;
  docsUrl: string;
  /** True when Pi ships this provider, endpoint and reasoning metadata. */
  native: boolean;
  /** True when the provider's catalog models can reason. */
  reasoning: boolean;
  models: readonly ProviderModelPreset[];
}

export const PROVIDERS: readonly ProviderDescriptor[] = [
  {
    id: 'opencode',
    name: 'OpenCode Zen',
    tagline: 'Curated, tested models billed through OpenCode Zen.',
    apiBaseUrl: 'https://opencode.ai/inference',
    api: 'mixed',
    envVar: 'OPENCODE_API_KEY',
    keysUrl: 'https://opencode.ai/auth',
    docsUrl: 'https://opencode.ai/docs/zen/',
    native: true,
    reasoning: true,
    models: [
      { id: 'gpt-5.2', name: 'GPT-5.2', reasoning: true },
      { id: 'claude-sonnet-4-5', name: 'Claude Sonnet 4.5', reasoning: true },
      { id: 'gemini-3.5-flash-lite', name: 'Gemini 3.5 Flash Lite', reasoning: true },
      { id: 'glm-5.3', name: 'GLM-5.3', reasoning: true },
      { id: 'kimi-k2.7-code', name: 'Kimi K2.7 Code', reasoning: true },
      { id: 'deepseek-v4.1-flash-free', name: 'DeepSeek V4.1 Flash Free', reasoning: true },
    ],
  },
  {
    id: 'opencode-go',
    name: 'OpenCode Go',
    tagline: 'A flat-rate OpenCode subscription for open coding models.',
    apiBaseUrl: 'https://opencode.ai/inference/go',
    api: 'mixed',
    envVar: 'OPENCODE_API_KEY',
    keysUrl: 'https://opencode.ai/auth',
    docsUrl: 'https://opencode.ai/docs/go/',
    native: true,
    reasoning: true,
    models: [
      { id: 'glm-5.3', name: 'GLM-5.3', reasoning: true },
      { id: 'kimi-k2.7-code', name: 'Kimi K2.7 Code', reasoning: true },
      { id: 'deepseek-v4.1-flash', name: 'DeepSeek V4.1 Flash', reasoning: true },
      { id: 'qwen3.8-max', name: 'Qwen3.8 Max', reasoning: true },
      { id: 'claude-haiku-5-5', name: 'Claude Haiku 5.5', reasoning: true },
      { id: 'minimax-m3', name: 'MiniMax-M3', reasoning: true },
    ],
  },
  {
    id: 'openrouter',
    name: 'OpenRouter',
    tagline: 'One key for hundreds of hosted models.',
    apiBaseUrl: 'https://openrouter.ai/api/v1',
    api: 'mixed',
    envVar: 'OPENROUTER_API_KEY',
    keysUrl: 'https://openrouter.ai/keys',
    docsUrl: 'https://openrouter.ai/docs/api_reference/authentication',
    native: true,
    reasoning: true,
    models: [
      { id: 'openrouter/auto', name: 'Auto Router', reasoning: true },
      { id: 'openai/gpt-5.6-sol', name: 'OpenAI: GPT-5.6 Sol', reasoning: true },
      { id: 'anthropic/claude-sonnet-4.6', name: 'Anthropic: Claude Sonnet 4.6', reasoning: true },
      { id: 'google/gemini-4-argon', name: 'Google: Gemini 4 Argon', reasoning: true },
      { id: 'meta/muse-spark-1.3', name: 'Meta: Muse Spark 1.3', reasoning: true },
      { id: 'z-ai/glm-5.1', name: 'Z.ai: GLM 5.1', reasoning: true },
    ],
  },
  {
    id: 'nvidia',
    name: 'NVIDIA NIM',
    tagline: 'NVIDIA hosted NIM endpoints, OpenAI-compatible.',
    apiBaseUrl: 'https://integrate.api.nvidia.com/v1',
    api: 'openai-completions',
    envVar: 'NVIDIA_API_KEY',
    keysUrl: 'https://build.nvidia.com/settings/api-keys',
    docsUrl: 'https://docs.api.nvidia.com/nim/reference/llm-apis',
    native: true,
    reasoning: true,
    models: [
      { id: 'nvidia/nemotron-3-ultra-550b-a55b', name: 'Nemotron 3 Ultra 550B A55B', reasoning: true },
      { id: 'nvidia/nemotron-3.5-super-vl-120b-a12b', name: 'Nemotron 3.5 Super VL 120B A12B', reasoning: true },
      { id: 'deepseek-ai/deepseek-v4.1-flash', name: 'DeepSeek V4.1 Flash', reasoning: true },
      { id: 'z-ai/glm-5.3', name: 'GLM-5.3', reasoning: true },
      { id: 'moonshotai/kimi-k2.6', name: 'Kimi K2.6', reasoning: true },
      { id: 'openai/gpt-oss-20b', name: 'GPT OSS 20B', reasoning: true },
    ],
  },
  {
    id: 'custom',
    name: 'Custom OpenAI-compatible',
    tagline: 'Ollama, LM Studio, vLLM or any OpenAI-compatible endpoint.',
    apiBaseUrl: '',
    api: 'openai-completions',
    envVar: 'TALO_CUSTOM_API_KEY',
    keysUrl: '',
    docsUrl: 'https://pi.dev/models',
    native: false,
    reasoning: true,
    models: [],
  },
];

export const DEFAULT_PROVIDER: ProviderId = 'opencode';
export const CUSTOM_PROVIDER: ProviderId = 'custom';
export const CUSTOM_API_KEY_ENV = 'TALO_CUSTOM_API_KEY';
export const CUSTOM_BASE_URL_EXAMPLE = 'http://localhost:11434/v1';
export const CUSTOM_CONTEXT_WINDOW = 128000;
export const CUSTOM_MAX_TOKENS = 16384;
export const PI_MODELS_SCHEMA = 'https://pi.dev/schemas/models.schema.json';

export function isProviderId(value: string): value is ProviderId {
  return PROVIDERS.some(provider => provider.id === value);
}

/** Resolve a persisted provider name, falling back to the default for older settings. */
export function providerDescriptor(value: string): ProviderDescriptor {
  return PROVIDERS.find(provider => provider.id === value) ?? PROVIDERS.find(provider => provider.id === DEFAULT_PROVIDER)!;
}

/** The provider fields persisted with onboarding and projected to the Pi worker. */
export interface ProviderPreference { provider: ProviderId; model: string; baseUrl: string; reasoning: boolean }
export const DEFAULT_PREFERENCE: ProviderPreference = { provider: DEFAULT_PROVIDER, model: '', baseUrl: '', reasoning: false };

export function parseProviderPreference(value: unknown): ProviderPreference {
  const record = typeof value === 'object' && value !== null ? value as Record<string, unknown> : {};
  const provider = typeof record.provider === 'string' ? record.provider : DEFAULT_PROVIDER;
  return {
    provider: isProviderId(provider) ? provider : DEFAULT_PROVIDER,
    model: typeof record.model === 'string' ? record.model.trim().slice(0, 200) : '',
    baseUrl: typeof record.baseUrl === 'string' ? record.baseUrl.trim().slice(0, 2048) : '',
    reasoning: record.reasoning === true,
  };
}

export type ThinkingLevelName = 'off' | 'minimal' | 'low' | 'medium' | 'high' | 'xhigh' | 'max';

export interface PiModelEntry {
  id: string;
  name?: string;
  api?: string;
  baseUrl?: string;
  reasoning?: boolean;
  thinkingLevelMap?: Partial<Record<ThinkingLevelName, string | null>>;
  input?: ('text' | 'image')[];
  contextWindow?: number;
  maxTokens?: number;
  cost?: { input: number; output: number; cacheRead: number; cacheWrite: number };
}
export interface PiProviderEntry { baseUrl?: string; api?: string; apiKey?: string; models?: PiModelEntry[] }
export interface PiModelsJson { $schema: string; providers: Record<string, PiProviderEntry> }

function httpUrl(value: string): boolean {
  try {
    const url = new URL(value);
    return url.protocol === 'http:' || url.protocol === 'https:';
  } catch { return false; }
}

export function isLocalEndpoint(value: string): boolean {
  try {
    const host = new URL(value).hostname.toLowerCase();
    return host === 'localhost' || host === '127.0.0.1' || host === '0.0.0.0' || host === '::1' || host === '[::1]' || host.endsWith('.localhost');
  } catch { return false; }
}

/**
 * Build the models.json document Talo owns inside Talo's Pi agent directory.
 *
 * Native providers are deliberately omitted: Pi's bundled catalog already carries their
 * endpoints and reasoning metadata, and a `models` list here would replace it. Local
 * endpoints receive a literal placeholder key so they work without configuration. Hosted
 * custom endpoints read `TALO_CUSTOM_API_KEY` from the environment until Talo has a
 * credential store.
 */
export function modelsJson(preference: ProviderPreference): PiModelsJson {
  const empty: PiModelsJson = { $schema: PI_MODELS_SCHEMA, providers: {} };
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
        apiKey: isLocalEndpoint(baseUrl) ? 'talo-local' : `$${CUSTOM_API_KEY_ENV}`,
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

/** Models with reasoning that Talo's bundled Pi registry reports for a provider. */
export interface ProviderModelInfo {
  id: string;
  name: string;
  reasoning: boolean;
  thinkingLevels: ThinkingLevelName[];
  contextWindow: number;
  maxTokens: number;
  /** Input modalities from Pi's registry, e.g. ['text'] or ['text','image']. */
  input: string[];
}
export interface ProviderModelList { provider: ProviderId; name: string; native: boolean; models: ProviderModelInfo[] }
