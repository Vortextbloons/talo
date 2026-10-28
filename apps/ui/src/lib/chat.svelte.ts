import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
  ChatAttachment,
  ChatAttachmentInput,
  ChatBootstrap,
  ChatEvent,
  ChatMessage,
  ConnectionInfo,
  ConversationInfo,
  ConversationSnapshot,
  DelegationActivity,
  DelegationTask,
  GenerationState,
  ModelSelection,
  MessagePage,
  ResearchActivity,
} from '../../../../packages/contracts/src/chat';
import type { ProviderModelInfo, StartupSnapshot, ThinkingLevelName } from '../../../../packages/contracts/src/index';
import { rolesFor } from '../../../../packages/contracts/src/index';
import { providerName } from './connection-providers';

const THINKING_LEVEL_NAMES: ThinkingLevelName[] = ['off', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'];

function parseThinkingLevel(value: string | undefined | null): ThinkingLevelName | null {
  if (value && THINKING_LEVEL_NAMES.includes(value as ThinkingLevelName)) return value as ThinkingLevelName;
  return null;
}

export type WorkerState = 'starting' | 'ready' | 'stopped' | 'failed';

export interface DiscoveryState {
  loading: boolean;
  error: string;
}

export interface ChatStore {
  research: Record<string, ResearchActivity[]>;

  delegated: Record<string, string[]>;
  initialized: boolean;
  preview: boolean;
  phase: 'idle' | 'loading' | 'ready' | 'error';
  conversations: ConversationInfo[];
  connections: ConnectionInfo[];
  models: Record<string, ProviderModelInfo[]>;
  discovery: Record<string, DiscoveryState>;
  active: ConversationSnapshot | null;
  draftSelection: ModelSelection | null;
  worker: { state: WorkerState; error: string };
  error: string;
  notice: string;
  openingId: string | null;
  creating: boolean;
  sending: boolean;
  connecting: boolean;
  connectingProvider: string;
  connectingName: string;
  selectingModel: boolean;
  thinkingLevel: ThinkingLevelName | null;
  thinkingBusy: boolean;
  historyLoading: boolean;
  historyRevision: number;
  child: ConversationSnapshot | null;
  childParent: ConversationInfo | null;
  childLoading: boolean;
  activity: Record<string, DelegationActivity[]>;
}

export const chat: ChatStore = $state({
  research: {},
  delegated: {},
  initialized: false,
  preview: false,
  phase: 'idle',
  conversations: [],
  connections: [],
  models: {},
  discovery: {},
  active: null,
  draftSelection: null,
  worker: { state: 'stopped', error: '' },
  error: '',
  notice: '',
  openingId: null,
  creating: false,
  sending: false,
  connecting: false,
  connectingProvider: '',
  connectingName: '',
  selectingModel: false,
  thinkingLevel: null,
  thinkingBusy: false,
  historyLoading: false,
  historyRevision: 0,
  child: null,
  childParent: null,
  childLoading: false,
  activity: {},
});

const EVENT_NAME = 'talo://chat';
const PREVIEW_MESSAGE = 'Open the desktop app to chat.';

let telegramUnlisten: UnlistenFn | undefined;
let unlisten: UnlistenFn | undefined;
let queuedEvents: ChatEvent[] | null = null;
let openToken = 0;
let pendingSnapshot: { conversationId: string; events: ChatEvent[] } | null = null;
const MESSAGE_WINDOW = 120;
let reconcileTimer: ReturnType<typeof setTimeout> | undefined;
let stopFallback: ReturnType<typeof setTimeout> | undefined;
let delegationOpenRequest = 0;
let sequenceByGeneration = new Map<string, number>();
let finishedGenerations = new Set<string>();
let promotedQueued = new Map<string, string>();

function errorMessage(value: unknown): string {
  if (typeof value === 'string') return value.slice(0, 800);
  if (value instanceof Error) return value.message.slice(0, 800);
  try {
    const text = String(value);
    return text === '[object Object]' ? 'Something went wrong.' : text.slice(0, 800);
  } catch {
    return 'Something went wrong.';
  }
}

function newId(): string {
  try {
    return crypto.randomUUID();
  } catch {
    return `generation-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
  }
}

function isWorking(): boolean {
  return chat.active?.state === 'generating' || chat.active?.state === 'cancelling';
}

function statusFor(state: GenerationState): ChatMessage['status'] {
  if (state === 'failed') return 'failed';
  if (state === 'cancelled') return 'cancelled';
  if (state === 'generating' || state === 'cancelling') return 'generating';
  return 'completed';
}

function byNewest(left: ConversationInfo, right: ConversationInfo): number {
  return Date.parse(right.updatedAt) - Date.parse(left.updatedAt);
}

function upsertConversation(info: ConversationInfo): void {
  const index = chat.conversations.findIndex(conversation => conversation.id === info.id);
  if (index >= 0) chat.conversations[index] = info;
  else chat.conversations.push(info);
  chat.conversations.sort(byNewest);
}

function touchConversation(conversationId: string, state: GenerationState): void {
  const info = chat.conversations.find(conversation => conversation.id === conversationId);
  if (!info) return;
  info.updatedAt = new Date().toISOString();
  if (state === 'completed' || state === 'cancelled' || state === 'failed') {
    chat.conversations.sort(byNewest);
  }
}

function applyBootstrap(bootstrap: ChatBootstrap): void {
  chat.conversations = [...bootstrap.conversations].sort(byNewest);
  chat.connections = bootstrap.connections;
  if (bootstrap.active) {
    applySnapshot(bootstrap.active);
  } else {
    chat.draftSelection = pickDefaultSelection();
  }
}

function applySnapshot(snapshot: ConversationSnapshot): void {
  const previous = chat.active;
  const localPending = previous?.conversation.id === snapshot.conversation.id
    && (chat.sending || previous.state === 'generating' || previous.state === 'cancelling')
    ? previous.messages.filter(message => message.id.startsWith('local-'))
    : [];

  chat.active = {
    ...snapshot,
    delegations: snapshot.delegations ?? [],
    queued: snapshot.queued ?? [],
    messages: snapshot.messages.map(message => ({ ...message })),
  };


  for (const message of localPending) {
    if (chat.active.messages.some(existing => existing.id === message.id)) continue;
    if (message.role === 'user' && !chat.active.messages.some(existing => existing.role === 'user' && existing.text === message.text)) {
      const firstAssistant = chat.active.messages.findIndex(existing => existing.role === 'assistant' && existing.generationId === message.id.slice('local-user-'.length));
      if (firstAssistant >= 0) chat.active.messages.splice(firstAssistant, 0, message);
      else chat.active.messages.push(message);
    }
  }

  if (snapshot.generationId) {
    sequenceByGeneration.set(snapshot.generationId, Math.max(sequenceByGeneration.get(snapshot.generationId) ?? -1, snapshot.sequence));
  }
  chat.active.error = snapshot.error ?? '';
  chat.thinkingLevel = parseThinkingLevel(snapshot.thinkingLevel);

  upsertConversation(snapshot.conversation);
  if (snapshot.conversation.selection) chat.draftSelection = snapshot.conversation.selection;
}

function chooseAssistant(conversation: ConversationSnapshot, generationId: string): ChatMessage | undefined {
  return conversation.messages.find(message => message.role === 'assistant' && message.generationId === generationId);
}

function upsertStreamText(generationId: string, text: string, state: GenerationState, reasoning?: string): void {
  const conversation = chat.active;
  if (!conversation) return;
  const existing = chooseAssistant(conversation, generationId);
  if (existing) {
    existing.text = text;
    if (reasoning !== undefined) {
      if (reasoning) existing.reasoning = reasoning;
      else delete existing.reasoning;
    }
    existing.status = statusFor(state);
    return;
  }
  conversation.messages.push({
    id: `assistant-${generationId}`,
    role: 'assistant',
    text,
    status: statusFor(state),
    generationId,
    ...(reasoning ? { reasoning } : {}),
  });
}

function upsertMessage(message: ChatMessage, generationId: string): void {
  const conversation = chat.active;
  if (!conversation) return;
  const byId = conversation.messages.findIndex(existing => existing.id === message.id);
  if (byId >= 0) {
    conversation.messages[byId] = { ...conversation.messages[byId], ...message };
    return;
  }
  if (message.role === 'assistant') {
    const existing = chooseAssistant(conversation, generationId);
    if (existing) {
      Object.assign(existing, message);
      return;
    }
  } else {
    const local = conversation.messages.findIndex(existing =>
      existing.id.startsWith('local-user-') && existing.text === message.text,
    );
    if (local >= 0) {
      conversation.messages[local] = { ...message };
      return;
    }
  }
  const firstAssistant = conversation.messages.findIndex(existing =>
    existing.role === 'assistant' && existing.generationId === generationId,
  );
  if (firstAssistant >= 0 && message.role === 'user') conversation.messages.splice(firstAssistant, 0, message);
  else conversation.messages.push(message);
}

function finishGeneration(event: Extract<ChatEvent, { type: 'generation' }>): void {
  const conversation = chat.active;
  if (!conversation) return;
  const terminal = event.state === 'completed' || event.state === 'cancelled' || event.state === 'failed';
  if (!terminal) return;

  let assistant = chooseAssistant(conversation, event.generationId);
  if (!assistant && event.state === 'failed' && !conversation.after) {
    assistant = {
      id: event.message?.id ?? `assistant-${event.generationId}`,
      role: 'assistant',
      text: event.text ?? '',
      status: 'failed',
      generationId: event.generationId,
      error: event.error ?? 'The reply did not finish.',
      ...(event.reasoning ? { reasoning: event.reasoning } : {}),
    };
    conversation.messages.push(assistant);
  }
  if (assistant) {
    assistant.status = statusFor(event.state);
    if (event.error) assistant.error = event.error;
    if (event.text !== undefined && !assistant.text) assistant.text = event.text;
  }
  conversation.state = event.state;
  conversation.generationId = null;
  if (event.error) conversation.error = event.error;
  finishedGenerations.add(event.generationId);
  if (stopFallback) {
    clearTimeout(stopFallback);
    stopFallback = undefined;
  }
}

function scheduleReconcile(conversationId: string, delay = 300): void {
  if (reconcileTimer) clearTimeout(reconcileTimer);
  reconcileTimer = setTimeout(() => {
    reconcileTimer = undefined;
    const active = chat.active;
    if (!active || active.conversation.id !== conversationId) return;
    void refreshSnapshot(conversationId, true);
  }, delay);
}

function handleChildGenerationEvent(event: Extract<ChatEvent, { type: 'generation' }>): void {
  const child = chat.child;
  if (!child || child.conversation.id !== event.conversationId) return;
  const terminal = event.state === 'completed' || event.state === 'cancelled' || event.state === 'failed';
  if (event.delta) {
    const existing = child.messages.find(message => message.role === 'assistant' && message.generationId === event.generationId);
    if (existing) {
      existing.text = (event.delta.reset ? '' : existing.text) + (event.delta.text ?? '');
      const reasoning = (event.delta.reset ? '' : existing.reasoning ?? '') + (event.delta.reasoning ?? '');
      if (reasoning) existing.reasoning = reasoning;
      else delete existing.reasoning;
      existing.status = statusFor(event.state);
    } else {
      child.messages.push({
        id: `assistant-${event.generationId}`,
        role: 'assistant',
        text: event.delta.text ?? '',
        status: statusFor(event.state),
        generationId: event.generationId,
        ...(event.delta.reasoning ? { reasoning: event.delta.reasoning } : {}),
      });
    }
  }
  if (event.text !== undefined) {
    const existing = child.messages.find(message => message.role === 'assistant' && message.generationId === event.generationId);
    if (existing) existing.text = event.text;
  }
  if (event.message) {
    const index = child.messages.findIndex(message => message.id === event.message!.id);
    if (index >= 0) child.messages[index] = { ...child.messages[index], ...event.message };
    else child.messages.push({ ...event.message });
  }
  child.state = event.state;
  child.generationId = terminal ? null : event.generationId;
  child.sequence = Math.max(child.sequence, event.sequence);
  if (event.error) child.error = event.error;
}

function applyDelegation(task: DelegationTask): void {
  const active = chat.active;
  if (active && active.conversation.id === task.parentConversationId) {
    active.delegations = [...(active.delegations ?? []).filter(item => item.id !== task.id), task]
      .sort((left, right) => left.createdAt.localeCompare(right.createdAt));
    const generationId = active.generationId;
    if (generationId) {
      const ids = chat.delegated[generationId] ?? [];
      if (!ids.includes(task.id)) {
        chat.delegated[generationId] = [...ids, task.id];
        const keys = Object.keys(chat.delegated);
        if (keys.length > 100) delete chat.delegated[keys[0]];
      }
    }
  }
  if (chat.child && chat.child.conversation.id === task.childConversationId) {
    chat.child.state = task.state === 'waiting' ? 'generating' : chat.child.state;
  }
}

function promoteQueuedTurn(conversation: ConversationSnapshot, generationId: string): void {
  const text = promotedQueued.get(generationId);
  if (text === undefined) return;
  promotedQueued.delete(generationId);
  if (conversation.messages.some(message => message.role === 'user' && message.text === text)) return;
  conversation.messages.push({
    id: `local-user-${generationId}`,
    role: 'user',
    text,
    status: 'completed',
  });
}

function handleGenerationEvent(event: Extract<ChatEvent, { type: 'generation' }>): void {
  const conversation = chat.active;
  if (!conversation || conversation.conversation.id !== event.conversationId) {
    touchConversation(event.conversationId, event.state);
    return;
  }
  if (finishedGenerations.has(event.generationId) && event.state !== 'failed' && event.state !== 'cancelled' && event.state !== 'completed') {
    return;
  }
  if (conversation.generationId && conversation.generationId !== event.generationId && isWorking()) {

    return;
  }


  const lastGenerationSequence = sequenceByGeneration.get(event.generationId) ?? -1;
  if (event.sequence <= lastGenerationSequence) return;
  const gap = event.sequence > lastGenerationSequence + 1;
  if (event.delta && gap && !conversation.after) {
    void refreshSnapshot(event.conversationId, true, event);
    return;
  }
  promoteQueuedTurn(conversation, event.generationId);
  sequenceByGeneration.set(event.generationId, event.sequence);
  conversation.sequence = event.sequence;

  if (event.state === 'generating' || event.state === 'cancelling') {
    conversation.state = event.state;
    conversation.generationId = event.generationId;
  }
  if (!conversation.after) {
    if (event.delta) {
      const existing = chooseAssistant(conversation, event.generationId);
      upsertStreamText(event.generationId, (event.delta.reset ? '' : existing?.text ?? '') + (event.delta.text ?? ''), event.state,
        (event.delta.reset ? '' : existing?.reasoning ?? '') + (event.delta.reasoning ?? ''));
    }
    if (event.text !== undefined || event.reasoning !== undefined) upsertStreamText(event.generationId, event.text ?? chooseAssistant(conversation, event.generationId)?.text ?? '', event.state, event.reasoning);
    if (event.message) upsertMessage(event.message, event.generationId);
    trimLatestWindow();
  }
  if (event.error) conversation.error = event.error;

  finishGeneration(event);
  touchConversation(event.conversationId, event.state);

  if (gap) scheduleReconcile(event.conversationId);
}

function handleEvent(event: ChatEvent): void {
  if (event.type === 'steering') {
    if (chat.active?.conversation.id === event.conversationId) {
      const index = chat.active.messages.findIndex(message => message.id === event.message.id);
      if (index < 0) chat.active.messages.push(event.message);
      else chat.active.messages[index] = event.message;
    }
    return;
  }
  if (event.type === 'delegation') {
    applyDelegation(event.task);
    return;
  }
  if (event.type === 'activity') {
    const list = [...(chat.activity[event.taskId] ?? [])];
    const index = list.findIndex(item => item.id === event.activity.id);
    if (index >= 0) list[index] = { ...list[index], ...event.activity };
    else list.push(event.activity);
    chat.activity[event.taskId] = list.slice(-100);
    const task = chat.active?.delegations?.find(item => item.id === event.taskId);
    if (task) task.currentActivity = event.activity.label;
    return;
  }
  if (event.type === 'queue') {
    if (chat.active?.conversation.id === event.conversationId) {
      const nextIds = new Set(event.queued.map(turn => turn.id));
      for (const turn of chat.active.queued ?? []) {
        if (!nextIds.has(turn.id)) promotedQueued.set(turn.id, turn.text);
      }
      chat.active.queued = event.queued;
    }
    return;
  }
  if (event.type === 'session') return;
  if (event.type === 'generation' && chat.child?.conversation.id === event.conversationId) {
    handleChildGenerationEvent(event);
    return;
  }
  if (event.type === 'generation' && pendingSnapshot?.conversationId === event.conversationId) {
    pendingSnapshot.events.push(event);
    return;
  }
  if (event.type === 'research') {
    const items = chat.research[event.generationId] ?? [];
    const previous = items.findIndex(item => item.id === event.activity.id);
    if (previous >= 0) items[previous] = event.activity;
    else items.push(event.activity);
    chat.research[event.generationId] = items.slice(-24);
    const keys = Object.keys(chat.research);
    if (keys.length > 100) delete chat.research[keys[0]];
    return;
  }
  if (event.type === 'worker') {
    chat.worker = { state: event.state, error: event.error ? errorMessage(event.error) : '' };
    if (event.state === 'ready') {
      void refreshConnections();
      void refreshSnapshot(chat.active?.conversation.id, false);
    } else if (event.state === 'failed') {
      chat.error = event.error ? `Pi stopped: ${errorMessage(event.error)}` : 'Pi stopped unexpectedly. Send again to wake it.';
    }
    return;
  }
  handleGenerationEvent(event);
}


export async function discoverModels(connectionId: string): Promise<void> {
  if (!isTauri()) return;
  const current = chat.discovery[connectionId];
  if (current?.loading) return;
  chat.discovery[connectionId] = { loading: true, error: '' };
  try {
    const models = await invoke<ProviderModelInfo[]>('provider_models', { connectionId });
    chat.models[connectionId] = models;
    chat.discovery[connectionId] = { loading: false, error: '' };
    if (!chat.draftSelection) chat.draftSelection = pickDefaultSelection();
  } catch (value) {
    chat.discovery[connectionId] = { loading: false, error: errorMessage(value) };
  }
}

async function discoverAll(): Promise<void> {
  if (chat.connecting) return;
  const connected = chat.connections.filter(connection => connection.connected);
  await Promise.allSettled(connected.map(connection => discoverModels(connection.id)));
  if (!chat.draftSelection) chat.draftSelection = pickDefaultSelection();
}

export async function refreshConnections(): Promise<void> {
  if (!isTauri() || chat.connecting) return;
  try {
    chat.connections = await invoke<ConnectionInfo[]>('provider_list');
    if (chat.draftSelection && !isSelectionAvailable(chat.draftSelection)) {
      chat.draftSelection = pickDefaultSelection();
    }
    const missing = chat.connections
      .filter(connection => connection.connected && (!chat.discovery[connection.id] || chat.discovery[connection.id].error))
      .map(connection => connection.id);
    await Promise.allSettled(missing.map(id => discoverModels(id)));
  } catch (value) {
    chat.error = errorMessage(value);
  }
}

export async function initChat(): Promise<StartupSnapshot | null> {
  if (chat.initialized) return null;
  chat.initialized = true;
  chat.phase = 'loading';
  chat.preview = !isTauri();

  if (!isTauri()) {
    chat.phase = 'ready';
    return null;
  }

  let startup: StartupSnapshot | null = null;
  queuedEvents = [];
  try {
    unlisten = await listen<ChatEvent>(EVENT_NAME, event => {
      if (queuedEvents) queuedEvents.push(event.payload);
      else handleEvent(event.payload);
    });
    telegramUnlisten = await listen('telegram-message', () => {
      if (chat.active?.conversation.id === 'talo-primary') scheduleReconcile('talo-primary', 0);
    });
    startup = await invoke<StartupSnapshot>('startup_snapshot');
    applyBootstrap(startup.chat);
    chat.phase = 'ready';
  } catch (value) {
    chat.phase = 'error';
    chat.error = errorMessage(value);
  } finally {
    const pending = queuedEvents ?? [];
    queuedEvents = null;
    for (const event of pending) handleEvent(event);
  }

  window.addEventListener('focus', reconcileOnFocus);
  document.addEventListener('visibilitychange', reconcileOnFocus);
  if (startup) deferInitialWork();
  return startup;
}

function deferInitialWork(): void {
  requestAnimationFrame(() => requestAnimationFrame(() => {
    void invoke('warm_worker_after_first_frame').catch(() => {});
    if (chat.active) void refreshSnapshot(chat.active.conversation.id, true);
    void discoverAll();
  }));
}

export function disposeChat(): void {
  unlisten?.();
  unlisten = undefined;
  telegramUnlisten?.();
  telegramUnlisten = undefined;
  if (reconcileTimer) clearTimeout(reconcileTimer);
  if (stopFallback) clearTimeout(stopFallback);
  reconcileTimer = undefined;
  stopFallback = undefined;
  window.removeEventListener('focus', reconcileOnFocus);
  document.removeEventListener('visibilitychange', reconcileOnFocus);
  queuedEvents = null;
  promotedQueued = new Map();
  chat.initialized = false;
}

function reconcileOnFocus(): void {
  if (document.visibilityState === 'hidden') return;
  if (!chat.active || chat.phase !== 'ready') return;
  const conversationId = chat.active.conversation.id;
  requestAnimationFrame(() => {
    if (document.visibilityState !== 'hidden' && chat.active?.conversation.id === conversationId) {
      void refreshSnapshot(conversationId, true);
    }
  });
}

export async function retryChat(): Promise<void> {
  disposeChat();
  chat.phase = 'idle';
  chat.error = '';
  await initChat();
}


async function refreshSnapshot(conversationId: string | undefined, quiet: boolean, initialEvent?: ChatEvent, preserveHistory = true): Promise<void> {
  if (!conversationId || !isTauri()) return;
  if (pendingSnapshot?.conversationId === conversationId) {
    if (initialEvent) pendingSnapshot.events.push(initialEvent);
    return;
  }
  const token = ++openToken;
  const pending = { conversationId, events: initialEvent ? [initialEvent] : [] as ChatEvent[] };
  pendingSnapshot = pending;
  let succeeded = false;
  try {
    const snapshot = await invoke<ConversationSnapshot>('conversation_refresh', { conversationId });
    if (token !== openToken) return;
    if (chat.active?.conversation.id === conversationId) {
      if (chat.active.after && !initialEvent && preserveHistory) {
        const messages = chat.active.messages;
        const before = chat.active.before;
        const after = chat.active.after;
        applySnapshot({ ...snapshot, messages, before, after });
      } else applySnapshot(snapshot);
      succeeded = true;
    }
  } catch (value) {
    if (!quiet && token === openToken) chat.error = errorMessage(value);
  } finally {
    if (pendingSnapshot === pending) pendingSnapshot = null;
    if (succeeded) for (const event of pending.events) handleEvent(event);
    else if (pending.events.length && token === openToken) scheduleReconcile(conversationId, 1000);
  }
}

function trimLatestWindow(): void {
  const active = chat.active;
  if (!active || active.messages.length <= MESSAGE_WINDOW) return;
  active.messages = active.messages.slice(-MESSAGE_WINDOW);
  active.before = active.messages[0].id;
}

export async function loadHistory(direction: 'before' | 'after'): Promise<boolean> {
  const active = chat.active;
  const cursor = active?.[direction];
  if (!active || !cursor || chat.historyLoading || !isTauri()) return false;
  chat.historyLoading = true;
  const conversationId = active.conversation.id;
  try {
    const page = await invoke<MessagePage>('conversation_history', { conversationId, [direction]: cursor });
    if (chat.active !== active || active[direction] !== cursor) return false;
    const known = new Set(active.messages.map(message => message.id));
    const additional = page.messages.filter(message => !known.has(message.id));
    if (direction === 'before') {
      const combined = [...additional, ...active.messages];
      active.messages = combined.slice(0, MESSAGE_WINDOW);
      active.before = page.before;
      if (combined.length > MESSAGE_WINDOW) active.after = active.messages.at(-1)!.id;
    } else {
      const combined = [...active.messages, ...additional];
      active.messages = combined.slice(-MESSAGE_WINDOW);
      active.after = page.after;
      if (combined.length > MESSAGE_WINDOW) active.before = active.messages[0].id;
    }
    return true;
  } catch (value) {
    chat.error = errorMessage(value);
    return false;
  } finally {
    chat.historyLoading = false;
  }
}

export async function jumpToLatest(): Promise<boolean> {
  const conversationId = chat.active?.conversation.id;
  if (!conversationId) return false;
  await refreshSnapshot(conversationId, false, undefined, false);
  if (chat.active?.conversation.id !== conversationId || chat.active.after) return false;
  chat.historyRevision++;
  return true;
}

export function isSelectionAvailable(selection: ModelSelection | null): boolean {
  if (!selection) return false;
  const connection = chat.connections.find(item => item.id === selection.connectionId);
  if (!connection?.connected) return false;
  const models = chat.models[connection.id];
  if (!models || models.length === 0) return connection.modelId === undefined || connection.modelId === selection.modelId;
  return models.some(model => model.id === selection.modelId);
}


export const routing = $state({ mode: 'auto' as 'auto' | 'manual', tier: 'recommended' as 'economy' | 'recommended' });

export function pickDefaultSelection(): ModelSelection | null {
  const current = chat.draftSelection;
  if (current && isSelectionAvailable(current)) return current;

  if (routing.mode === 'auto') {
    for (const connection of chat.connections) {
      if (!connection.connected) continue;
      const roles = rolesFor(connection.providerId);
      if (!roles) continue;
      const pick = routing.tier === 'economy' ? roles.economyPilot : roles.smartPilot;
      return { connectionId: connection.id, modelId: pick.model };
    }
  }

  for (const connection of chat.connections) {
    if (!connection.connected) continue;
    const modelId = chat.models[connection.id]?.[0]?.id ?? connection.modelId;
    if (modelId) return { connectionId: connection.id, modelId };
  }

  for (const connection of chat.connections) {
    if (!connection.connected) continue;
    const first = chat.models[connection.id]?.[0];
    if (first) return { connectionId: connection.id, modelId: first.id };
  }
  return null;
}

export function modelsFor(connection: ConnectionInfo): ProviderModelInfo[] {
  const discovered = chat.models[connection.id];
  if (discovered?.length) return discovered;
  if (connection.modelId) {
    return [{
      id: connection.modelId,
      name: connection.modelId,
      reasoning: false,
      thinkingLevels: [],
      contextWindow: 0,
      maxTokens: 0,
      input: ['text'],
    }];
  }
  return [];
}

export function selectionKey(selection: ModelSelection | null): string {
  return selection ? `${selection.connectionId}::${selection.modelId}` : '';
}

export function parseSelectionKey(key: string): ModelSelection | null {
  const split = key.indexOf('::');
  if (split <= 0 || split >= key.length - 2) return null;
  return { connectionId: key.slice(0, split), modelId: key.slice(split + 2) };
}

export function selectionLabel(selection: ModelSelection | null): string {
  if (!selection) return 'No model selected';
  const connection = chat.connections.find(item => item.id === selection.connectionId);
  const model = chat.models[selection.connectionId]?.find(item => item.id === selection.modelId);
  return `${model?.name ?? selection.modelId} · ${connection?.name ?? (connection ? providerName(connection.providerId) : 'Disconnected provider')}`;
}

export async function createConversation(): Promise<boolean> {
  if (!isTauri()) {
    chat.error = PREVIEW_MESSAGE;
    return false;
  }
  chat.creating = true;
  chat.error = '';
  try {
    const selection = chat.draftSelection ?? pickDefaultSelection();
    const snapshot = await invoke<ConversationSnapshot>(
      'conversation_create',
      selection ? { selection } : {},
    );
    applySnapshot(snapshot);
    chat.child = null;
    chat.childParent = null;
    if (!snapshot.conversation.selection) chat.draftSelection = null;
    if (selection) applyRoutingEffort(selection.connectionId);
    return true;
  } catch (value) {
    chat.error = errorMessage(value);
    return false;
  } finally {
    chat.creating = false;
  }
}

export async function openConversation(conversationId: string): Promise<boolean> {
  if (!isTauri()) {
    chat.error = PREVIEW_MESSAGE;
    return false;
  }
  if (chat.active?.conversation.id === conversationId) return true;
  if (chat.sending) return false;
  chat.child = null;
  chat.childParent = null;
  const token = ++openToken;
  chat.openingId = conversationId;
  chat.error = '';
  try {
    const snapshot = await invoke<ConversationSnapshot>('conversation_open', { conversationId });
    if (token === openToken) applySnapshot(snapshot);
    return true;
  } catch (value) {
    if (token === openToken) chat.error = errorMessage(value);
    return false;
  } finally {
    if (token === openToken) chat.openingId = null;
  }
}


function applyRoutingEffort(connectionId: string): void {
  if (routing.mode !== 'auto') return;
  const roles = rolesFor(chat.connections.find(item => item.id === connectionId)?.providerId ?? '');
  if (roles) void setThinkingLevel(routing.tier === 'economy' ? roles.economyPilot.effort : roles.smartPilot.effort);
}

export async function chooseModel(selection: ModelSelection | null): Promise<void> {
  if (!selection) return;
  if (isWorking()) {
    chat.error = 'Stop the current reply before changing models.';
    return;
  }
  chat.draftSelection = selection;
  const conversation = chat.active;
  if (!conversation || !isTauri()) return;

  chat.selectingModel = true;
  chat.error = '';
  try {
    const info = await invoke<ConversationInfo>('conversation_select_model', {
      conversationId: conversation.conversation.id,
      selection,
    });
    upsertConversation(info);
    if (chat.active?.conversation.id === info.id) chat.active.conversation = info;
    applyRoutingEffort(selection.connectionId);
  } catch (value) {
    chat.error = errorMessage(value);
  } finally {
    chat.selectingModel = false;
  }
}


export async function setThinkingLevel(level: ThinkingLevelName): Promise<void> {
  const conversation = chat.active;
  if (!conversation) return;
  if (isWorking()) {
    chat.error = 'Stop the current reply before changing thinking.';
    return;
  }
  if (!isTauri()) {
    chat.thinkingLevel = level;
    return;
  }
  chat.thinkingBusy = true;
  chat.error = '';
  try {
    chat.thinkingLevel = parseThinkingLevel(await invoke<string>('conversation_thinking', {
      conversationId: conversation.conversation.id,
      level,
    }));
  } catch (value) {
    chat.error = errorMessage(value);
  } finally {
    chat.thinkingBusy = false;
  }
}

function noteFirstMessage(conversationId: string, text: string, hasImages = false): void {
  const conversation = chat.active;
  if (!conversation || conversation.conversation.id !== conversationId) return;
  const current = conversation.conversation.title;
  if (current && current !== 'New conversation') return;
  const compact = text.split(/\s+/).filter(Boolean).join(' ');
  if (!compact) {

    if (!hasImages) return;
    conversation.conversation.title = 'Photo';
    const info = chat.conversations.find(item => item.id === conversationId);
    if (info) info.title = 'Photo';
    return;
  }
  const title = compact.length > 60 ? compact.slice(0, 60) + '…' : compact;
  conversation.conversation.title = title;
  const info = chat.conversations.find(item => item.id === conversationId);
  if (info) info.title = title;
}

function appendOptimistic(
  conversationId: string,
  generationId: string,
  text: string,
  attachments: ChatAttachment[] = [],
): void {
  const conversation = chat.active;
  if (!conversation || conversation.conversation.id !== conversationId) return;
  conversation.messages.push(
    {
      id: `local-user-${generationId}`,
      role: 'user',
      text,
      status: 'completed',
      ...(attachments.length ? { attachments } : {}),
    },
    {
      id: `local-assistant-${generationId}`,
      role: 'assistant',
      text: '',
      status: 'generating',
      generationId,
    },
  );
  conversation.generationId = generationId;
  conversation.state = 'generating';
  conversation.error = '';
}

function rollbackOptimistic(generationId: string): void {
  const conversation = chat.active;
  if (!conversation) return;
  conversation.messages = conversation.messages.filter(message =>
    message.id !== `local-user-${generationId}` && message.id !== `local-assistant-${generationId}`,
  );
  if (conversation.generationId === generationId) {
    conversation.generationId = null;
    conversation.state = 'idle';
  }
}

export async function sendMessage(rawText: string, attachments: ChatAttachmentInput[] = []): Promise<boolean> {
  const text = rawText.trim();
  if (!text && !attachments.length) return false;
  if (!isTauri()) {
    chat.error = PREVIEW_MESSAGE;
    return false;
  }
  if (chat.sending) return false;

  let selection = chat.active?.conversation.selection ?? chat.draftSelection;
  if (!selection) {
    chat.error = 'Connect a provider and choose a model in Settings before sending.';
    return false;
  }
  if (!isSelectionAvailable(selection)) {
    chat.error = 'That model is no longer connected. Choose another model in Settings.';
    return false;
  }

  chat.sending = true;
  chat.error = '';
  chat.notice = '';
  let generationId = '';
  try {
    if (!chat.active) {
      const snapshot = await invoke<ConversationSnapshot>('conversation_create', { selection });
      applySnapshot(snapshot);
    }
    if (chat.active?.after && !await jumpToLatest()) return false;
    const conversation = chat.active;
    if (!conversation) throw new Error('The conversation could not be opened.');

    if (!conversation.conversation.selection) {
      const info = await invoke<ConversationInfo>('conversation_select_model', {
        conversationId: conversation.conversation.id,
        selection,
      });
      upsertConversation(info);
      conversation.conversation = info;
      selection = info.selection ?? selection;
    }

    generationId = newId();

    if (conversation.conversation.id === chat.active?.conversation.id && isWorking()) {
      if (attachments.length) {
        chat.error = 'Wait for the current reply before attaching an image.';
        return false;
      }
      if (conversation.state !== 'generating' || !conversation.generationId) throw new Error('The reply is stopping. Send your message when it finishes.');
      await invoke('chat_steer', {
        conversationId: conversation.conversation.id,
        generationId: conversation.generationId,
        messageId: generationId,
        text,
      });
      return true;
    }

    const previews: ChatAttachment[] = attachments.map((attachment, index) => ({
      id: `local-image-${generationId}-${index}`,
      name: attachment.name || 'image',
      mimeType: attachment.mimeType,
      bytes: Math.round((attachment.data.length * 3) / 4),
      url: `data:${attachment.mimeType};base64,${attachment.data}`,
    }));
    appendOptimistic(conversation.conversation.id, generationId, text, previews);
    await invoke('chat_send', {
      conversationId: conversation.conversation.id,
      generationId,
      text,
      attachments,
    });
    noteFirstMessage(conversation.conversation.id, text, previews.length > 0);
    return true;
  } catch (value) {
    if (generationId) {
      rollbackOptimistic(generationId);
      const active = chat.active;
      if (active) active.queued = (active.queued ?? []).filter(turn => turn.id !== generationId);
    }
    chat.error = errorMessage(value);
    return false;
  } finally {
    chat.sending = false;
  }
}

export async function stopGeneration(): Promise<void> {
  const conversation = chat.active;
  if (!conversation?.generationId || !isTauri()) return;
  const generationId = conversation.generationId;
  conversation.state = 'cancelling';
  chat.error = '';
  try {
    await invoke('chat_stop', { generationId });
    if (stopFallback) clearTimeout(stopFallback);
    stopFallback = setTimeout(() => {
      stopFallback = undefined;
      if (chat.active?.state === 'cancelling') {
        void refreshSnapshot(chat.active.conversation.id, true);
      }
    }, 15000);
  } catch (value) {
    if (conversation.generationId === generationId) conversation.state = 'generating';
    chat.error = errorMessage(value);
  }
}

export async function openDelegationTask(taskId: string): Promise<boolean> {
  if (!isTauri()) return false;
  const requestId = ++delegationOpenRequest;
  const parentConversationId = chat.active?.conversation.id;
  if (!parentConversationId) {
    chat.childLoading = false;
    return false;
  }
  chat.childLoading = true;
  chat.error = '';
  try {
    const snapshot = await invoke<ConversationSnapshot>('delegation_action', {
      operation: 'open',
      payload: { taskId },
    });
    if (requestId !== delegationOpenRequest || chat.active?.conversation.id !== parentConversationId) return false;
    chat.child = { ...snapshot, delegations: snapshot.delegations ?? [], queued: snapshot.queued ?? [] };
    chat.childParent = snapshot.parentConversation ?? null;
    chat.activity[taskId] = snapshot.delegationActivity ?? [];
    return true;
  } catch (value) {
    if (requestId !== delegationOpenRequest || chat.active?.conversation.id !== parentConversationId) return false;
    chat.error = errorMessage(value);
    return false;
  } finally {
    if (requestId === delegationOpenRequest) chat.childLoading = false;
  }
}

export function closeDelegationTask(): void {
  delegationOpenRequest++;
  chat.child = null;
  chat.childParent = null;
  chat.childLoading = false;
}

export async function cancelDelegation(taskId: string): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke('delegation_action', { operation: 'cancel', payload: { taskId } });
  } catch (value) {
    chat.error = errorMessage(value);
  }
}

export async function cancelAllDelegations(conversationId: string): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke('delegation_action', { operation: 'cancel_all', payload: { conversationId } });
  } catch (value) {
    chat.error = errorMessage(value);
  }
}

export async function retryGeneration(): Promise<void> {
  if (chat.active?.after && !await jumpToLatest()) return;
  const conversation = chat.active;
  if (!conversation || !isTauri() || isWorking() || chat.sending) return;
  const selection = conversation.conversation.selection ?? chat.draftSelection;
  if (!selection || !isSelectionAvailable(selection)) {
    chat.error = 'Choose a connected model before retrying.';
    return;
  }
  const hasUserMessage = conversation.messages.some(message => message.role === 'user');
  if (!hasUserMessage) {
    chat.error = 'There is no message to retry yet.';
    return;
  }


  const failedReply = [...conversation.messages].reverse().find(message =>
    message.role === 'assistant'
      && (message.status === 'failed' || message.status === 'cancelled')
      && Boolean(message.generationId),
  );
  if (!failedReply?.generationId) {
    chat.error = 'That reply cannot be retried. Start a new message instead.';
    return;
  }
  const generationId = failedReply.generationId;
  finishedGenerations.delete(generationId);
  failedReply.text = '';
  failedReply.status = 'generating';
  failedReply.error = undefined;
  conversation.generationId = generationId;
  conversation.state = 'generating';
  conversation.error = '';
  chat.error = '';
  try {
    await invoke('chat_retry', {
      conversationId: conversation.conversation.id,
      generationId,
    });
  } catch (value) {
    const assistant = chooseAssistant(conversation, generationId);
    if (assistant) {
      assistant.status = 'failed';
      assistant.error = errorMessage(value);
    }
    conversation.state = 'failed';
    conversation.generationId = null;
    conversation.error = errorMessage(value);
    chat.error = errorMessage(value);
  }
}

export interface ConnectInput {
  providerId: string;
  name: string;
  apiKey: string;
  modelId?: string;
  connectionId?: string;
}

let chatgptAttemptId = '';
let chatgptCancelled = false;

export function cancelChatGPT(): void {
  chatgptCancelled = true;
}

async function signInChatGPT(connectionId?: string): Promise<ConnectionInfo> {
  chatgptCancelled = false;
  chatgptAttemptId = await invoke<string>('chatgpt_begin', { connectionId: connectionId ?? null });
  try {
    while (!chatgptCancelled) {
      const connection = await invoke<ConnectionInfo | null>('chatgpt_poll', { attemptId: chatgptAttemptId, cancel: false });
      if (connection) { chatgptAttemptId = ''; return connection; }
      await new Promise(resolve => setTimeout(resolve, 1000));
    }
    throw new Error('ChatGPT sign-in cancelled.');
  } finally {
    if (chatgptAttemptId) {
      await invoke('chatgpt_poll', { attemptId: chatgptAttemptId, cancel: true }).catch(() => {});
      chatgptAttemptId = '';
    }
  }
}


export async function connectProvider(input: ConnectInput): Promise<ConnectionInfo | null> {
  if (!isTauri()) {
    chat.error = PREVIEW_MESSAGE;
    return null;
  }
  chat.error = '';
  chat.connecting = true;
  chat.connectingProvider = input.providerId;
  chat.connectingName = input.name;
  try {
    const info = input.providerId === 'chatgpt' ? await signInChatGPT(input.connectionId) : await invoke<ConnectionInfo>('provider_connect', {
      providerId: input.providerId,
      name: input.name,
      apiKey: input.apiKey,
      modelId: input.modelId || null,
    });
    chat.connecting = false;
    chat.connectingProvider = '';
    chat.connectingName = '';
    const index = chat.connections.findIndex(connection => connection.id === info.id);
    if (index >= 0) chat.connections[index] = info;
    else chat.connections.push(info);
    if (info.modelId) chat.draftSelection = { connectionId: info.id, modelId: info.modelId };
    await discoverModels(info.id);
    if (!chat.draftSelection) chat.draftSelection = pickDefaultSelection();
    chat.notice = `${info.name} is connected.`;
    return info;
  } catch (value) {
    chat.connecting = false;
    chat.connectingProvider = '';
    chat.connectingName = '';
    chat.error = errorMessage(value);
    return null;
  }
}

export async function disconnectProvider(connectionId: string): Promise<void> {
  if (!isTauri()) {
    chat.error = PREVIEW_MESSAGE;
    return;
  }
  chat.error = '';
  try {
    const warning = await invoke<string | null>('provider_disconnect', { connectionId });
    const connection = chat.connections.find(connection => connection.id === connectionId);
    if (connection?.providerId === 'chatgpt') connection.connected = false;
    else chat.connections = chat.connections.filter(connection => connection.id !== connectionId);
    delete chat.models[connectionId];
    delete chat.discovery[connectionId];
    if (chat.draftSelection?.connectionId === connectionId) chat.draftSelection = pickDefaultSelection();
    chat.notice = warning || 'Provider disconnected.';
  } catch (value) {
    chat.error = errorMessage(value);
  }
}

export function dismissError(): void {
  chat.error = '';
}

export function dismissNotice(): void {
  chat.notice = '';
}
