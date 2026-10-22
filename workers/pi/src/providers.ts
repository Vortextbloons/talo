import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { getSupportedThinkingLevels } from '@earendil-works/pi-ai';
import type { AgentSession } from '@earendil-works/pi-coding-agent';
import { modelsJson, parseProviderPreference, type ProviderId, type ProviderModelInfo, type ProviderPreference } from '../../../packages/contracts/src/index.js';

/** Talo-owned, non-secret provider projection written by the host after onboarding. */
export const PREFERENCE_FILE = 'provider.json';
/** Pi's compatible-endpoint file inside Talo's agent directory. */
export const MODELS_FILE = 'models.json';

export async function readPreference(dataDir: string): Promise<ProviderPreference> {
  try {
    return parseProviderPreference(JSON.parse(await readFile(path.join(dataDir, PREFERENCE_FILE), 'utf8')));
  } catch { return parseProviderPreference(null); }
}

/**
 * Synchronize Talo's Pi models.json from the provider preference before a session is created.
 * Pi loads this file while building its model registry, so writing it afterwards has no effect.
 * Native providers produce an empty provider map and keep Pi's bundled catalog intact.
 */
export async function syncModelsJson(agentDir: string, preference: ProviderPreference): Promise<boolean> {
  const target = path.join(agentDir, MODELS_FILE);
  const text = `${JSON.stringify(modelsJson(preference), null, 2)}\n`;
  try { if (await readFile(target, 'utf8') === text) return false; } catch { /* Missing file: write it. */ }
  await mkdir(agentDir, { recursive: true });
  const temporary = `${target}.tmp`;
  await writeFile(temporary, text);
  await rename(temporary, target);
  return true;
}

/** Reasoning and thinking-level metadata exactly as Pi's registry resolves it for the provider. */
export function registryModels(session: AgentSession, provider: ProviderId): ProviderModelInfo[] {
  return session.modelRuntime.getModels(provider).map(model => ({
    id: model.id,
    name: model.name,
    reasoning: model.reasoning === true,
    thinkingLevels: getSupportedThinkingLevels(model),
    contextWindow: model.contextWindow,
    maxTokens: model.maxTokens,
  }));
}
