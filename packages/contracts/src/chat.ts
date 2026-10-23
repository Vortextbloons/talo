/** Public chat contract. Credentials are accepted only by provider_connect, never events. */
export type GenerationState = 'idle' | 'generating' | 'cancelling' | 'completed' | 'cancelled' | 'failed';
export interface ModelSelection { connectionId: string; modelId: string }
export interface ConnectionInfo { id: string; providerId: string; name: string; connected: boolean; modelId?: string; baseUrl?: string }
export interface ConversationInfo {
  id: string; title: string; sessionId: string | null; sessionPath: string | null;
  createdAt: string; updatedAt: string; selection: ModelSelection | null;
  workspaceId?: string; archived?: boolean;
}
export type DelegationState = 'queued' | 'running' | 'waiting' | 'completed' | 'failed' | 'cancelled' | 'interrupted';
/** One child task attached to a parent conversation. Child transcripts are not main chats. */
export interface DelegationTask {
  id: string; parentConversationId: string; childConversationId: string;
  title: string; brief: string; state: DelegationState;
  result?: string; error?: string; delivered: boolean;
  /** Latest user-readable action, shown on the agent card while it works. */
  currentActivity?: string;
  createdAt: string; updatedAt: string;
}
export interface QueuedTurn { id: string; text: string; createdAt: string }
/** One user-readable step a delegated child took. Raw arguments and results are never exposed. */
export interface DelegationActivity {
  id: string; tool: string; label: string; detail?: string;
  state: 'running' | 'completed' | 'failed' | 'cancelled';
  createdAt?: string;
}
export interface ChatMessage {
  id: string; role: 'user' | 'assistant'; text: string;
  /** Complete accumulated thinking text when the model exposes it; absent otherwise. */
  reasoning?: string;
  status: 'completed' | 'generating' | 'cancelled' | 'failed';
  generationId?: string; error?: string;
  /** Images the user attached to this message. */
  attachments?: ChatAttachment[];
}
/** An image sent with a chat_send call. `data` is standard base64 without the data: prefix. */
export interface ChatAttachmentInput { name?: string; mimeType: string; data: string }
/** A stored attachment. `url` loads its preview from the native host on demand. */
export interface ChatAttachment { id: string; name: string; mimeType: string; bytes: number; url?: string }
export interface ConversationSnapshot {
  conversation: ConversationInfo; messages: ChatMessage[];
  before: string | null; after: string | null;
  generationId: string | null; state: GenerationState; sequence: number; error?: string;
  /** Effective Pi thinking level for the open session, when known. */
  thinkingLevel?: string;
  /** Child tasks owned by this (main) conversation. */
  delegations?: DelegationTask[];
  /** User turns waiting behind the active generation, oldest first. */
  queued?: QueuedTurn[];
  /** Set when this snapshot is a child task transcript. */
  parentConversation?: ConversationInfo;
  /** Chronological actions for a child task transcript. */
  delegationActivity?: DelegationActivity[];
}
export interface MessagePage {
  messages: ChatMessage[]; before: string | null; after: string | null;
}
export interface StreamDelta { text?: string; reasoning?: string; reset?: boolean }
export interface ChatBootstrap {
  conversations: ConversationInfo[]; connections: ConnectionInfo[];
  active: ConversationSnapshot | null;
}
export type ChatEvent = {
  type: 'generation'; conversationId: string; generationId: string; sequence: number;
  state: GenerationState; text?: string; reasoning?: string; delta?: StreamDelta; message?: ChatMessage; error?: string;
} | { type: 'worker'; state: 'starting' | 'ready' | 'stopped' | 'failed'; error?: string }
  | { type: 'research'; conversationId: string; generationId: string; activity: ResearchActivity }
  | { type: 'delegation'; task: DelegationTask }
  | { type: 'queue'; conversationId: string; queued: QueuedTurn[] }
  | { type: 'session'; conversationId: string; state: 'closed' }
  | { type: 'activity'; taskId: string; parentConversationId: string; conversationId: string; generationId: string; activity: DelegationActivity };
export interface ResearchActivity {
  id: string; tool: string; input: string; state: 'running' | 'completed' | 'failed' | 'cancelled';
  preview?: string; sources?: string[]; artifact?: string; error?: string;
  results?: { title: string; url: string; snippet: string }[];
}

/** Private host/worker operations. apiKey is ephemeral and must never be echoed or logged. */
export type WorkerMethod = 'ping' | 'shutdown' | 'models.list' | 'provider.validate' |
  'chatgpt.begin' | 'chatgpt.poll' | 'chatgpt.refresh' | 'chatgpt.revoke' |
  'session.open' | 'session.prompt' | 'session.retry' | 'session.deliver' | 'session.abort' | 'session.snapshot' |
  'session.history' | 'session.thinking' | 'tools.configure' | 'tools.status' | 'tools.test' | 'tools.cancel';
export interface WorkerRequest { id: string; method: WorkerMethod; params?: Record<string, unknown> }
export type WorkerEvent =
  | { type: 'ready' | 'sleeping' | 'error'; message?: string }
  | { type: 'response'; id: string; result?: unknown; error?: string }
  | { type: 'chat'; event: ChatEvent | { type: 'context'; conversationId: string; compacted: boolean; usage: unknown } };

/**
 * Tauri API (camelCase arguments; all failures reject with sanitized errors):
 * chat_bootstrap() -> ChatBootstrap
 * conversation_create({ selection?: ModelSelection }) -> ConversationSnapshot
 * conversation_open({ conversationId }) -> ConversationSnapshot
 * conversation_select_model({ conversationId, selection }) -> ConversationInfo
 * chat_send({ conversationId, generationId, text, attachments? }) -> void (accepted; events
 * carry result). attachments is a bounded list of {name?, mimeType, data}; the host stores
 * each image under the Pi agent workspace and forwards it to the model as image content.
 * While the same conversation is generating, a text follow-up is durably queued and the
 * snapshot's `queued` list shows it in order. Other conversations keep generating.
 * delegation_action({operation,payload}) -> value. Operations: list({conversationId}),
 * open({taskId}) returns the child transcript snapshot without changing the active main
 * chat, cancel({taskId}) and cancel_all({conversationId}).
 * chat_retry({ conversationId, generationId }) -> void (reuse the failed/stopped generation ID)
 * chat_stop({ generationId }) -> void
 * provider_list() -> ConnectionInfo[]
 * provider_connect({ providerId, name, apiKey, modelId?, baseUrl? }) -> ConnectionInfo
 * provider_disconnect({ connectionId }) -> void
 * provider_models({ connectionId }) -> ProviderModelInfo[]
 * Event name: talo://chat, payload ChatEvent.
 * session.open private params: conversationId, sessionPath? (null creates), provider, model,
 * apiKey?, baseUrl?. Result: bounded metadata { sessionId, path, provider, model, messageCount,
 * thinkingLevel, state, generationId, sequence }, never the transcript.
 * session.history params: conversationId, offset?, limit? (max 200). Result: { messages,
 * nextOffset } where each page is also bounded by bytes; nextOffset is null on the last page.
 * session.prompt params: conversationId, generationId, text, images? (each {name, mimeType,
 * data, path}), capabilities? (subset of 'web'|'browser'|'files'|'google'; absent or empty means all).
 * The worker sends images natively when the model accepts image input and
 * otherwise appends the on-disk paths so Pi's own tools can read them.
 * session.retry params: conversationId, generationId, images?, capabilities?.
 * session.deliver params: conversationId, generationId, text (structured child results; delivered
 * as a hidden custom message, never as a fabricated user turn).
 * session.abort params: generationId. session.snapshot returns {messages,state,generationId,sequence}.
 * session.thinking params: conversationId, level ('minimal'|'low'|'medium'|'high'|'xhigh'|'max');
 * result { level } is the effective level after Pi clamps it to the model's registry levels.
 * models.list/provider.validate params: provider, model?, apiKey?, baseUrl?. models.list returns
 * {provider,name,native,models}; provider.validate must perform an authenticated probe.
 * A generation event's text/reasoning replace accumulated content; delta appends content.
 * delta.reset clears both text and reasoning before appending its fields.
 * Event sequence numbers increase within a generation, including across its retries.
 */

/** Slice 3 host API:
 * library_action({operation,payload?}) coordinates all workspace/chat metadata writes.
 * Operations: state, workspace_save/switch/archive/delete, conversation_update/delete,
 * history_search, index_rebuild, memory_enable, context_usage.
 * memory_save/update({payload:{content,scope,workspaceId?,source?,personal?,consent?,id?}})
 * memory_list/search({payload:{workspaceId?,scope?,query?,history?}})
 * memory_delete({payload:{id}}) erases the selected version and its superseded ancestors.
 * Personal information defaults to true and requires consent=true on every save/update.
 * session.prompt/retry private params also accept bounded `context` reference snapshots.
 * Session references survive sleep; Pi's native JSONL is authoritative, SQLite is a rebuildable projection.
 */
