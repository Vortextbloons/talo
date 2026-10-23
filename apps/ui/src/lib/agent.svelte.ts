import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { AgentState, PendingAsk, Skill, WorkspaceBundle } from '../../../../packages/contracts/src/agent';





export const agent = $state({
  asks: [] as PendingAsk[],
  drafts: [] as WorkspaceBundle[],
  skills: [] as Skill[],
  skillProposals: [] as Skill[],
  budget: { used: 0, limit: 5 },
  error: '',
  busy: false,
  loaded: false,
});
let stateRevision = 0;

export function applyAgentState(state: AgentState): void {
  agent.asks = state.asks ?? [];
  agent.drafts = state.drafts ?? [];
  agent.skills = state.skills ?? [];
  agent.skillProposals = state.skillProposals ?? [];
  agent.budget = state.budget ?? { used: 0, limit: 5 };
  agent.loaded = true;
  stateRevision += 1;
}
export function agentStateRevision(): number { return stateRevision; }

export async function refreshAgent(): Promise<void> {
  if (!isTauri()) return;
  try { applyAgentState(await invoke<AgentState>('agent_action', { operation: 'state' })); }
  catch (e) { agent.error = String(e); }
}

export async function agentAction(operation: string, payload: Record<string, unknown> = {}): Promise<boolean> {
  if (!isTauri()) { agent.error = 'Open the desktop app to work with the Inbox.'; return false; }
  agent.busy = true; agent.error = '';
  try { await invoke('agent_action', { operation, payload }); await refreshAgent(); return true; }
  catch (e) { agent.error = String(e); return false; }
  finally { agent.busy = false; }
}


export function initAgent(initialState?: AgentState | null): () => void {
  let active = true;
  let stop: (() => void) | undefined;
  if (initialState) applyAgentState(initialState);
  else if (initialState === undefined) void refreshAgent();
  if (isTauri()) {
    void listen('agent-changed', () => { stateRevision += 1; if (active) void refreshAgent(); })
      .then(dispose => { if (active) stop = dispose; else dispose(); });
  }

  const timer = setInterval(() => { if (active) void refreshAgent(); }, 20_000);
  return () => { active = false; stop?.(); clearInterval(timer); };
}


export function asksFor(conversationId: string | undefined): PendingAsk[] {
  return conversationId
    ? agent.asks.filter(ask => ask.conversationId === conversationId || ask.parentConversationId === conversationId)
    : [];
}
