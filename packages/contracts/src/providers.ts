











export type ProviderId = 'opencode' | 'opencode-go' | 'openrouter' | 'nvidia' | 'custom';
export type ProviderApi = 'openai-completions' | 'anthropic-messages' | 'openai-responses' | 'google-generative-ai' | 'mixed';


export interface ProviderModelPreset { id: string; name: string; reasoning: boolean }
export interface ProviderDescriptor {
  id: ProviderId;
  name: string;
  tagline: string;

  apiBaseUrl: string;
  api: ProviderApi;

  envVar: string;

  keysUrl: string;
  docsUrl: string;

  native: boolean;

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
export const CUSTOM_BASE_URL_EXAMPLE = 'http://localhost:11434/v1';

export function isProviderId(value: string): value is ProviderId {
  return PROVIDERS.some(provider => provider.id === value);
}


export function providerDescriptor(value: string): ProviderDescriptor {
  return PROVIDERS.find(provider => provider.id === value) ?? PROVIDERS.find(provider => provider.id === DEFAULT_PROVIDER)!;
}


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


export interface ProviderModelInfo {
  id: string;
  name: string;
  reasoning: boolean;
  thinkingLevels: ThinkingLevelName[];
  contextWindow: number;
  maxTokens: number;

  input: string[];
}


export interface ProviderRoleModel { model: string; effort: ThinkingLevelName }







export interface ProviderRoles {
  economyPilot: ProviderRoleModel;
  smartPilot: ProviderRoleModel;
  flashWorker: ProviderRoleModel;
  smartWorker: ProviderRoleModel;
  tierLabels: { economy: string; recommended: string };
}

export const PROVIDER_ROLES: Partial<Record<ProviderId | 'chatgpt', ProviderRoles>> = {
  chatgpt: {
    economyPilot: { model: 'gpt-6-luna', effort: 'high' },
    smartPilot: { model: 'gpt-6.1-sol', effort: 'medium' },
    flashWorker: { model: 'gpt-6-luna', effort: 'low' },
    smartWorker: { model: 'gpt-6-luna', effort: 'high' },
    tierLabels: { economy: 'Economy', recommended: 'Recommended' },
  },
  opencode: {
    economyPilot: { model: 'deepseek-v4.1-flash', effort: 'high' },
    smartPilot: { model: 'deepseek-v4.1-flash', effort: 'high' },
    flashWorker: { model: 'deepseek-v4.1-flash', effort: 'low' },
    smartWorker: { model: 'deepseek-v4.1-flash', effort: 'high' },
    tierLabels: { economy: 'Economy', recommended: 'Recommended' },
  },
  'opencode-go': {
    economyPilot: { model: 'beam-free', effort: 'max' },
    smartPilot: { model: 'deepseek-v4.1-flash', effort: 'high' },
    flashWorker: { model: 'muse-spark-1.3-contributor', effort: 'high' },
    smartWorker: { model: 'deepseek-v4.1-flash', effort: 'high' },
    tierLabels: { economy: 'Economy', recommended: 'Recommended' },
  },
  openrouter: {
    economyPilot: { model: 'nvidia/nemotron-3.5-super-vl', effort: 'medium' },
    smartPilot: { model: 'deepseek/deepseek-v4.1-flash', effort: 'high' },
    flashWorker: { model: 'openrouter/free', effort: 'low' },
    smartWorker: { model: 'deepseek/deepseek-v4.1-flash', effort: 'max' },
    tierLabels: { economy: 'Economy', recommended: 'Recommended' },
  },
  nvidia: {
    economyPilot: { model: 'nvidia/nemotron-3.5-super-vl-120b-a12b', effort: 'medium' },
    smartPilot: { model: 'stepfun-ai/step-5-preview', effort: 'high' },
    flashWorker: { model: 'nvidia/nemotron-3.5-lightning-30b-a3b', effort: 'low' },
    smartWorker: { model: 'nvidia/nemotron-3.5-lightning-30b-a3b', effort: 'high' },
    tierLabels: { economy: 'Fast', recommended: 'Smart' },
  },
};

export function rolesFor(providerId: string): ProviderRoles | undefined {
  return PROVIDER_ROLES[providerId as keyof typeof PROVIDER_ROLES];
}
