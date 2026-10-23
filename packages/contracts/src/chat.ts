
export type GenerationState = 'idle' | 'generating' | 'cancelling' | 'completed' | 'cancelled' | 'failed';
export interface ModelSelection { connectionId: string; modelId: string }
export interface ConnectionInfo { id: string; providerId: string; name: string; connected: boolean; modelId?: string; baseUrl?: string }
export interface ConversationInfo {
  id: string; title: string; sessionId: string | null; sessionPath: string | null;
  createdAt: string; updatedAt: string; selection: ModelSelection | null;
  workspaceId?: string; archived?: boolean;
}
export type DelegationState = 'queued' | 'running' | 'waiting' | 'completed' | 'failed' | 'cancelled' | 'interrupted';

export interface DelegationTask {
  id: string; parentConversationId: string; childConversationId: string;
  title: string; brief: string; state: DelegationState;
  agentName?: string;
  result?: string; error?: string; delivered: boolean;

  currentActivity?: string;
  createdAt: string; updatedAt: string;
}
export interface QueuedTurn { id: string; text: string; createdAt: string }

export interface DelegationActivity {
  id: string; tool: string; label: string; detail?: string;
  state: 'running' | 'completed' | 'failed' | 'cancelled';
  createdAt?: string;
}
export interface ChatMessage {
  id: string; role: 'user' | 'assistant'; text: string;

  reasoning?: string;
  status: 'completed' | 'generating' | 'cancelled' | 'failed';
  generationId?: string; error?: string;

  attachments?: ChatAttachment[];
}

export interface ChatAttachmentInput { name?: string; mimeType: string; data: string }

export interface ChatAttachment { id: string; name: string; mimeType: string; bytes: number; url?: string }
export interface ConversationSnapshot {
  conversation: ConversationInfo; messages: ChatMessage[];
  before: string | null; after: string | null;
  generationId: string | null; state: GenerationState; sequence: number; error?: string;

  thinkingLevel?: string;

  delegations?: DelegationTask[];

  queued?: QueuedTurn[];

  parentConversation?: ConversationInfo;

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


export type WorkerMethod = 'ping' | 'shutdown' | 'models.list' | 'provider.validate' | 'theme.draft' |
  'chatgpt.begin' | 'chatgpt.poll' | 'chatgpt.refresh' | 'chatgpt.revoke' |
  'session.open' | 'session.prompt' | 'session.retry' | 'session.deliver' | 'session.abort' | 'session.snapshot' |
  'session.history' | 'session.thinking' | 'tools.configure' | 'tools.status' | 'tools.test' | 'tools.cancel';
export interface WorkerRequest { id: string; method: WorkerMethod; params?: Record<string, unknown> }
export type WorkerEvent =
  | { type: 'ready' | 'sleeping' | 'error'; message?: string }
  | { type: 'response'; id: string; result?: unknown; error?: string }
  | { type: 'chat'; event: ChatEvent | { type: 'context'; conversationId: string; compacted: boolean; usage: unknown } };
