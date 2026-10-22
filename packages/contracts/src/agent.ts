/** Slice 9 public contract: things the agent may author for the user. */

/** What a workspace bundle may enable. An empty list means every capability is on. */
export type Capability = 'web' | 'browser' | 'files';
export type MemoryScope = 'inherit' | 'workspace' | 'off';

/**
 * A workspace is a small bundle, not just a label: the brief and pinned notes are
 * injected into every turn in that workspace, and capabilities narrow its tools.
 * Agent-authored bundles land as `draft` and wait for the user in the Inbox.
 */
export interface WorkspaceBundle {
  id: string; name: string; folder: string | null; archived: boolean;
  instructions: string; pinned: string[]; memoryScope: MemoryScope;
  capabilities: Capability[]; agentOwned: boolean; draft: boolean; createdAt: string;
}

/** A to-do the agent hands the user. `why` is the one-line reason it exists now. */
export interface Assignment {
  id: string; workspaceId: string; title: string; detail: string; why: string;
  due: string | null; status: 'open' | 'done' | 'dismissed';
  conversationId: string | null; createdAt: string; updatedAt: string;
}

export interface AskOption { id: string; label: string; detail: string }
export interface AskQuestion {
  id: string; question: string; options: AskOption[];
  /** Option used when the user chooses "You decide" or never answers. */
  recommended: string; other: boolean;
}
/** A blocking question card owned by a live Pi tool call. Never persisted. */
export interface PendingAsk {
  id: string; conversationId: string; header: string; questions: AskQuestion[];
  createdAt: number; expiresAt: number;
}
/** One resolved answer: an option, a custom "something else", or the recommendation. */
export interface AskAnswer { questionId: string; optionId?: string; label?: string; other?: string; decided?: boolean }

export interface AgentBudget { used: number; limit: number }
export interface AgentState {
  assignments: Assignment[]; asks: PendingAsk[];
  drafts: WorkspaceBundle[]; budget: AgentBudget;
}

/**
 * Tauri API (camelCase arguments; all failures reject with sanitized errors):
 * agent_action({ operation, payload? }) -> value
 * Operations: state | answer | assignment | space_accept | space_dismiss | space_permission.
 * state returns AgentState.
 * answer({payload:{id, answers:{<questionId>: <optionId> | 'decided' | {other: text}}}})
 * assignment({payload:{id, status:'open'|'done'|'dismissed'}})
 * space_accept({payload:{id}}) publishes a draft; space_dismiss({payload:{id}}) deletes it.
 * space_permission({payload:{id, agentOwned:boolean}}) lets the agent keep editing a workspace.
 * Event name: talo://agent, payload {}. Listeners re-read agent_action state.
 *
 * The worker reaches the same data through the `talo-agent` MCP server's single `agent`
 * tool (operations: state, space_list, space_create, space_update, task_assign, task_list,
 * ask). The host validates every write, caps agent-created objects per day, and resolves an
 * unanswered question card to its recommended options after five minutes.
 */
