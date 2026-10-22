/** Public chat contract. Credentials are accepted only by provider_connect, never events. */
export type GenerationState = 'idle' | 'generating' | 'cancelling' | 'completed' | 'cancelled' | 'failed';
export interface ModelSelection { connectionId: string; modelId: string }
export interface ConnectionInfo { id: string; providerId: string; name: string; connected: boolean; modelId?: string; baseUrl?: string }
export interface ConversationInfo {
  id: string; title: string; sessionId: string | null; sessionPath: string | null;
  createdAt: string; updatedAt: string; selection: ModelSelection | null;
  workspaceId?: string; archived?: boolean;
}
export interface ChatMessage {
  id: string; role: 'user' | 'assistant'; text: string;
  /** Complete accumulated thinking text when the model exposes it; absent otherwise. */
  reasoning?: string;
  status: 'completed' | 'generating' | 'cancelled' | 'failed';
  generationId?: string; error?: string;
}
export interface ConversationSnapshot {
  conversation: ConversationInfo; messages: ChatMessage[];
  generationId: string | null; state: GenerationState; sequence: number; error?: string;
}
export interface ChatBootstrap {
  conversations: ConversationInfo[]; connections: ConnectionInfo[];
  active: ConversationSnapshot | null;
}
export type ChatEvent = {
  type: 'generation'; conversationId: string; generationId: string; sequence: number;
  state: GenerationState; text?: string; reasoning?: string; message?: ChatMessage; error?: string;
} | { type: 'worker'; state: 'starting' | 'ready' | 'stopped' | 'failed'; error?: string };

/** Private host/worker operations. apiKey is ephemeral and must never be echoed or logged. */
export type WorkerMethod = 'ping' | 'shutdown' | 'models.list' | 'provider.validate' |
  'session.open' | 'session.prompt' | 'session.retry' | 'session.abort' | 'session.snapshot';
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
 * chat_send({ conversationId, generationId, text }) -> void (accepted; events carry result)
 * chat_retry({ conversationId, generationId }) -> void (reuse the failed/stopped generation ID)
 * chat_stop({ generationId }) -> void
 * provider_list() -> ConnectionInfo[]
 * provider_connect({ providerId, name, apiKey, modelId?, baseUrl? }) -> ConnectionInfo
 * provider_disconnect({ connectionId }) -> void
 * provider_models({ connectionId }) -> ProviderModelInfo[]
 * Event name: talo://chat, payload ChatEvent.
 * session.open private params: conversationId, sessionPath? (null creates), provider, model,
 * apiKey?, baseUrl?. Result: { sessionId, path, messages: ChatMessage[] }.
 * session.prompt params: conversationId, generationId, text.
 * session.retry params: conversationId, generationId.
 * session.abort params: generationId. session.snapshot returns {messages,state,generationId,sequence}.
 * models.list/provider.validate params: provider, model?, apiKey?, baseUrl?. models.list returns
 * {provider,name,native,models}; provider.validate must perform an authenticated probe.
 * A generation event's text is the COMPLETE accumulated assistant text (not a delta).
 * reasoning, when present, is the COMPLETE accumulated thinking text (not a delta).
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
