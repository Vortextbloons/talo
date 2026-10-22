import { PROVIDERS, providerDescriptor } from '../../../../packages/contracts/src/index';
import type { ProviderDescriptor } from '../../../../packages/contracts/src/index';

/**
 * Providers offered by the secure connection form.
 *
 * These are Pi's native provider IDs, so the bundled catalog keeps supplying each model's
 * endpoint and reasoning metadata. The shared contract catalog is preferred when it already
 * describes a provider; the direct entries below cover the hosted providers Talo connects to
 * with an API key. A missing contract entry is still connectable because `provider_connect`
 * takes a provider ID rather than a closed enum.
 */
export interface ProviderChoice {
  id: string;
  name: string;
  tagline: string;
  keysUrl: string;
  docsUrl: string;
  apiBaseUrl: string;
  /** Environment variable Pi falls back to when no Talo credential exists. */
  envVar: string;
  /** A placeholder choice that explains what is not connected yet. */
  placeholder: boolean;
  suggestedModels: readonly string[];
}

interface DirectChoice {
  id: string;
  name: string;
  tagline: string;
  keysUrl: string;
  docsUrl: string;
  apiBaseUrl: string;
  envVar: string;
  suggestedModels: string[];
}

const DIRECT: DirectChoice[] = [
  {
    id: 'openai',
    name: 'OpenAI',
    tagline: 'GPT models through the OpenAI API.',
    keysUrl: 'https://platform.openai.com/api-keys',
    docsUrl: 'https://platform.openai.com/docs/api-reference',
    apiBaseUrl: 'https://api.openai.com/v1',
    envVar: 'OPENAI_API_KEY',
    suggestedModels: [],
  },
  {
    id: 'anthropic',
    name: 'Anthropic',
    tagline: 'Claude models through the Anthropic API.',
    keysUrl: 'https://console.anthropic.com/settings/keys',
    docsUrl: 'https://docs.anthropic.com/en/api',
    apiBaseUrl: 'https://api.anthropic.com/v1',
    envVar: 'ANTHROPIC_API_KEY',
    suggestedModels: [],
  },
  {
    id: 'google',
    name: 'Google Gemini',
    tagline: 'Gemini models through the Google AI API.',
    keysUrl: 'https://aistudio.google.com/apikey',
    docsUrl: 'https://ai.google.dev/gemini-api/docs',
    apiBaseUrl: 'https://generativelanguage.googleapis.com/v1beta',
    envVar: 'GEMINI_API_KEY',
    suggestedModels: [],
  },
  {
    id: 'openrouter',
    name: 'OpenRouter',
    tagline: 'One key for models from many providers.',
    keysUrl: 'https://openrouter.ai/keys',
    docsUrl: 'https://openrouter.ai/docs/api_reference/authentication',
    apiBaseUrl: 'https://openrouter.ai/api/v1',
    envVar: 'OPENROUTER_API_KEY',
    suggestedModels: [],
  },
];

const order = ['openai', 'anthropic', 'google', 'openrouter', 'opencode', 'opencode-go', 'nvidia', 'custom'];

function fromDescriptor(descriptor: ProviderDescriptor): ProviderChoice {
  return {
    id: descriptor.id,
    name: descriptor.name,
    tagline: descriptor.tagline,
    keysUrl: descriptor.keysUrl,
    docsUrl: descriptor.docsUrl,
    apiBaseUrl: descriptor.apiBaseUrl,
    envVar: descriptor.envVar,
    placeholder: descriptor.id === 'custom',
    suggestedModels: descriptor.models.map(model => model.id),
  };
}

function fromDirect(choice: DirectChoice): ProviderChoice {
  const contract = PROVIDERS.find(provider => provider.id === choice.id);
  if (contract) return fromDescriptor(contract);
  return { ...choice, placeholder: false };
}

/** Hosted providers first, then any additional providers described by the shared catalog. */
export const PROVIDER_CHOICES: readonly ProviderChoice[] = (() => {
  const combined = new Map<string, ProviderChoice>();
  for (const choice of DIRECT) combined.set(choice.id, fromDirect(choice));
  for (const descriptor of PROVIDERS) {
    if (!combined.has(descriptor.id)) combined.set(descriptor.id, fromDescriptor(descriptor));
  }
  return [...combined.values()].sort(
    (left, right) => (order.indexOf(left.id) + 1 || 99) - (order.indexOf(right.id) + 1 || 99),
  );
})();

export function providerChoice(providerId: string): ProviderChoice {
  return PROVIDER_CHOICES.find(choice => choice.id === providerId)
    ?? fromDescriptor(providerDescriptor(providerId));
}

/** A friendly provider name even when the connection predates the current catalog. */
export function providerName(providerId: string): string {
  return PROVIDER_CHOICES.find(choice => choice.id === providerId)?.name ?? providerId;
}
