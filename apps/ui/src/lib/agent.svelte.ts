import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { AgentState, Assignment, PendingAsk, Skill, WorkspaceBundle } from '../../../../packages/contracts/src/agent';





export const agent = $state({
  assignments: [] as Assignment[],
  asks: [] as PendingAsk[],
  drafts: [] as WorkspaceBundle[],
  skills: [] as Skill[],
  skillProposals: [] as Skill[],
  budget: { used: 0, limit: 5 },
  error: '',
  busy: false,
  loaded: false,
});

function apply(state: AgentState): void {
  agent.assignments = state.assignments ?? [];
  agent.asks = state.asks ?? [];
  agent.drafts = state.drafts ?? [];
  agent.skills = state.skills ?? [];
  agent.skillProposals = state.skillProposals ?? [];
  agent.budget = state.budget ?? { used: 0, limit: 5 };
  agent.loaded = true;
}

export async function refreshAgent(): Promise<void> {
  if (!isTauri()) return;
  try { apply(await invoke<AgentState>('agent_action', { operation: 'state' })); }
  catch (e) { agent.error = String(e); }
}

export async function agentAction(operation: string, payload: Record<string, unknown> = {}): Promise<boolean> {
  if (!isTauri()) { agent.error = 'Open the desktop app to work with the Inbox.'; return false; }
  agent.busy = true; agent.error = '';
  try { await invoke('agent_action', { operation, payload }); await refreshAgent(); return true; }
  catch (e) { agent.error = String(e); return false; }
  finally { agent.busy = false; }
}


export function initAgent(): () => void {
  let active = true;
  let stop: (() => void) | undefined;
  void refreshAgent();
  if (isTauri()) {
    void listen('agent-changed', () => { if (active) void refreshAgent(); })
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
