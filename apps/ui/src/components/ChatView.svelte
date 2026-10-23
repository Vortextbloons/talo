<script lang="ts">
  import { PRIMARY_CONVERSATION_ID } from '../../../../packages/contracts/src/telegram';
  import LibrarySidebar from './LibrarySidebar.svelte';
  import { library, mutateLibrary } from '../lib/library.svelte';
  import { tick, untrack, onMount } from 'svelte';
  import mascot from '../../../../packages/design/mascot.svg';
  import type { ChatMessage } from '../../../../packages/contracts/src/chat';
  import type { ThinkingLevelName } from '../../../../packages/contracts/src/index';
  import { rolesFor } from '../../../../packages/contracts/src/index';
  import VirtualMessage from './VirtualMessage.svelte';
  import MessageBody from './MessageBody.svelte';
  import Reasoning from './Reasoning.svelte';
  import WebResearch from './WebResearch.svelte';
  import AgentCompanion from './AgentCompanion.svelte';
  import AskCard from './AskCard.svelte';
  import ChildTaskView from './ChildTaskView.svelte';
  import DelegationCalls from './DelegationCalls.svelte';
  import { asksFor } from '../lib/agent.svelte';
  import { browser, openBrowser, closeBrowser, selectBrowserSession, setBrowserWorkspaceVisible } from '../lib/browser.svelte';
  import {
    chat,
    loadHistory,
    jumpToLatest,
    chooseModel,
    closeDelegationTask,
    createConversation,
    dismissError,
    dismissNotice,
    modelsFor,
    openConversation,
    openDelegationTask,
    parseSelectionKey,
    isSelectionAvailable,
    retryGeneration,
    retryChat,
    selectionKey,
    selectionLabel,
    sendMessage,
    stopGeneration,
    setThinkingLevel,
    routing,
  } from '../lib/chat.svelte';

  let { onOpenSettings, name, tagline, showReasoning, agentName = 'Talo', onRenameAgent }: { onOpenSettings: () => void; name: string; tagline: string; showReasoning?: boolean; agentName?: string; onRenameAgent: (name: string) => Promise<void> } = $props();

  let showArchived = $state(false);
  let confirmingDelete = $state<string | null>(null);
  let text = $state('');

  interface DraftImage { id: string; name: string; mimeType: string; bytes: number; data: string; url: string }
  const IMAGE_TYPES = ['image/png', 'image/jpeg', 'image/webp', 'image/gif'];
  const MAX_IMAGES = 4;
  const MAX_IMAGE_BYTES = 8 * 1024 * 1024;
  let draftImages = $state<DraftImage[]>([]);
  let attachError = $state('');
  let dragOver = $state(false);
  let viewingImage = $state<string | null>(null);
  let viewingName = $state<string | null>(null);
  let fileInput = $state<HTMLInputElement>();
  let logEl = $state<HTMLDivElement>();
  let composerEl = $state<HTMLTextAreaElement>();
  let pinned = true;
  let changingHistory = false;
  let lastConversationId: string | null = null;
  let lastHistoryRevision = 0;

  interface PickerOption { key: string; label: string; reasoning: boolean; disabled: boolean; selected: boolean }
  interface PickerGroup { id: string; name: string; options: PickerOption[] }

  let pickerOpen = $state(false);
  let pickerActive = $state('');
  let pickerQuery = $state('');
  let pickerEl = $state<HTMLDivElement>();
  let menuEl = $state<HTMLDivElement>();
  let pickerSearchEl = $state<HTMLInputElement>();
  let triggerEl = $state<HTMLButtonElement>();
  let thinkingOpen = $state(false);
  let thinkingActive = $state('');
  let thinkingWrapEl = $state<HTMLDivElement>();
  let thinkingMenuEl = $state<HTMLDivElement>();
  let thinkingTriggerEl = $state<HTMLButtonElement>();

  const active = $derived(chat.active);
  const primary = $derived(active?.conversation.id === PRIMARY_CONVERSATION_ID);
  const primaryWaiting = $derived(asksFor(PRIMARY_CONVERSATION_ID).length);
  const otherChats = $derived(chat.conversations.filter(c => c.id !== PRIMARY_CONVERSATION_ID && (c.workspaceId ?? 'general') === library.activeWorkspace && Boolean(c.archived) === showArchived));
  const messages = $derived(chat.active?.messages ?? []);
  const delegations = $derived(chat.active?.delegations ?? []);
  function calledTasks(generationId?: string | null) {
    const ids = generationId ? chat.delegated[generationId] : undefined;
    if (!ids?.length) return [];
    return ids.flatMap(id => { const task = delegations.find(item => item.id === id); return task ? [task] : []; });
  }

  const unattachedTasks = $derived.by(() => {
    const attached = new Set(Object.values(chat.delegated).flat());
    return delegations.filter(task => !attached.has(task.id));
  });
  const queued = $derived(chat.active?.queued ?? []);
  const working = $derived(chat.active?.state === 'generating' || chat.active?.state === 'cancelling');
  const activeTasks = $derived(delegations.filter(task => ['queued', 'running', 'waiting'].includes(task.state)).length);
  const primaryStatus = $derived(primaryWaiting ? 'Needs your answer' : primary && working ? 'Working on your request' : primary && activeTasks ? `Working on ${activeTasks} ${activeTasks === 1 ? 'task' : 'tasks'}` : 'Your ongoing conversation');
  const cancelling = $derived(chat.active?.state === 'cancelling');
  const connected = $derived(chat.connections.filter(connection => connection.connected));
  const selection = $derived(chat.active?.conversation.selection ?? chat.draftSelection);
  const selectionAvailable = $derived(isSelectionAvailable(selection));
  const selectedKey = $derived(selectionKey(selection));
  const lastAssistantIndex = $derived(
    messages.reduce((last, message, index) => (message.role === 'assistant' ? index : last), -1),
  );
  const modelGroups = $derived(
    connected.map(connection => ({ connection, models: modelsFor(connection) })),
  );
  const selectionInGroups = $derived(
    Boolean(selection) && modelGroups.some(group =>
      group.connection.id === selection!.connectionId && group.models.some(model => model.id === selection!.modelId),
    ),
  );
  const greeting = $derived(name ? `Hello, ${name}.` : 'Hello, you.');
  const pickerGroups = $derived.by(() => {
    const groups: PickerGroup[] = [];
    for (const group of modelGroups) {

      const roles = routing.mode === 'auto' ? rolesFor(group.connection.providerId) : undefined;
      const pick = roles ? (routing.tier === 'economy' ? roles.economyPilot : roles.smartPilot) : undefined;
      if (pick) {
        const pilot = group.models.find(model => model.id === pick.model);
        groups.push({
          id: group.connection.id,
          name: group.connection.name,
          options: [{
            key: `${group.connection.id}::${pick.model}`,
            label: pilot?.name ?? pick.model,
            reasoning: pilot?.reasoning ?? false,
            disabled: working,
            selected: selectedKey === `${group.connection.id}::${pick.model}`,
          }],
        });
        continue;
      }
      const options: PickerOption[] = group.models.map(model => ({
        key: `${group.connection.id}::${model.id}`,
        label: model.name,
        reasoning: model.reasoning,
        disabled: working,
        selected: selectedKey === `${group.connection.id}::${model.id}`,
      }));
      if (!options.length) {
        const note = chat.discovery[group.connection.id]?.loading ? 'Discovering models…' : 'No models found — refresh in Settings';
        options.push({ key: `${group.connection.id}::`, label: note, reasoning: false, disabled: true, selected: false });
      }
      groups.push({ id: group.connection.id, name: group.connection.name, options });
    }
    return groups;
  });
  const pickerOptions = $derived(pickerGroups.flatMap(group => group.options));
  const pickerFilteredGroups = $derived.by(() => {
    const query = pickerQuery.trim().toLowerCase();
    if (!query) return pickerGroups;
    return pickerGroups
      .map(group => ({
        ...group,
        options: group.options.filter(option => option.label.toLowerCase().includes(query) || group.name.toLowerCase().includes(query)),
      }))
      .filter(group => group.options.length);
  });
  const pickerFilteredOptions = $derived(pickerFilteredGroups.flatMap(group => group.options));

  const imagesSupported = $derived.by(() => {
    if (!selection) return false;
    const connection = connected.find(item => item.id === selection.connectionId);
    if (!connection) return false;
    const model = modelsFor(connection).find(item => item.id === selection.modelId);
    return Boolean(model?.input?.includes('image'));
  });
  const pickerSelected = $derived.by(() => {
    const found = pickerOptions.find(option => option.selected);
    if (found) return found;
    if (selection && !selectionInGroups) {
      return { key: selectedKey, label: selectionLabel(selection), reasoning: false, disabled: true, selected: true };
    }
    return null;
  });
  const pickerValue = $derived(pickerSelected ? pickerSelected.label : 'Choose a model');
  const selectedModel = $derived.by(() => {
    if (!selection) return null;
    const group = modelGroups.find(item => item.connection.id === selection!.connectionId);
    return group?.models.find(model => model.id === selection!.modelId) ?? null;
  });

  const thinkingOptions = $derived(selectedModel?.thinkingLevels ?? []);
  const thinkingValue = $derived.by(() => {
    if (chat.thinkingLevel && thinkingOptions.includes(chat.thinkingLevel)) return chat.thinkingLevel;
    return thinkingOptions.includes('medium') ? 'medium' : thinkingOptions[0] ?? '';
  });
  const composerHint = $derived(
    !connected.length ? 'Connect a provider to begin'
      : !selection ? 'Choose a model below'
        : !selectionAvailable ? 'Choose a connected model'
          : working ? 'Talo is replying — new messages queue'
            : '',
  );
  const placeholder = $derived(
    !connected.length ? 'Connect a provider to start'
      : !selection ? 'Choose a model below'
        : !selectionAvailable ? 'Choose a connected model below'
          : 'What is on your mind?',
  );

  $effect(() => {
    const id = chat.child?.conversation.id ?? chat.active?.conversation.id ?? 'primary';
    untrack(() => selectBrowserSession(id));
  });
  onMount(() => { setBrowserWorkspaceVisible(true); return () => setBrowserWorkspaceVisible(false); });

  $effect(() => {
    const conversationId = chat.active?.conversation.id ?? null;
    const last = messages[messages.length - 1];

    const revision = chat.historyRevision;
    const signal = `${revision}:${conversationId ?? ''}:${messages.length}:${last?.text.length ?? 0}:${last?.reasoning?.length ?? 0}:${last?.status ?? ''}:${chat.active?.state ?? ''}`;
    if (revision !== lastHistoryRevision) {
      lastHistoryRevision = revision;
      pinned = true;
    }
    if (conversationId !== lastConversationId) {
      lastConversationId = conversationId;
      pinned = true;
    }
    if (signal && pinned && !chat.active?.after && !changingHistory) {
      tick().then(() => {
        if (logEl) logEl.scrollTop = logEl.scrollHeight;
      });
    }
  });

  function resize(): void {
    const element = composerEl;
    if (!element) return;
    element.style.height = '0px';
    element.style.height = `${Math.min(190, Math.max(48, element.scrollHeight))}px`;
  }

  function useSuggestion(value: string): void {
    text = value;
    tick().then(() => {
      resize();
      composerEl?.focus();
    });
  }


  const SKILL_REQUEST = 'Save how we just did this as a skill I can reuse. Use the talo-agent tool with operation skill_create. Write the instructions from what actually worked: when to use it, required tools and prerequisites, the steps and any naming conventions, the decisions that need my input, mistakes to avoid, and how to verify the outcome. Leave out passwords, account numbers, document contents and login cookies.';
  function skillRequest(detail: string): string {
    return detail ? SKILL_REQUEST + '\n\nThe workflow: ' + detail : SKILL_REQUEST;
  }

  function expandShortcut(value: string): string {
    const match = /^\/learn\b\s*([\s\S]*)$/i.exec(value.trim());
    return match ? skillRequest(match[1].trim()) : value;
  }
  async function submit(): Promise<void> {
    const images = draftImages.map(image => ({ name: image.name, mimeType: image.mimeType, data: image.data }));
    if (chat.sending || (!text.trim() && !images.length)) return;
    const value = expandShortcut(text);
    const accepted = await sendMessage(value, images);
    if (accepted) {
      text = '';
      draftImages = [];
      attachError = '';
      pinned = true;
      await tick();
      resize();
      if (logEl) logEl.scrollTop = logEl.scrollHeight;
    }
  }

  function removeDraft(id: string): void {
    draftImages = draftImages.filter(image => image.id !== id);
    attachError = '';
  }





  async function addImages(files: File[] | FileList | null): Promise<void> {
    if (!files) return;
    attachError = '';
    for (const file of Array.from(files)) {
      if (draftImages.length >= MAX_IMAGES) {
        attachError = `Attach up to ${MAX_IMAGES} images per message.`;
        break;
      }
      const mimeType = (file.type || '').toLowerCase();
      if (!IMAGE_TYPES.includes(mimeType)) {
        attachError = 'Images must be PNG, JPEG, WebP or GIF.';
        continue;
      }
      if (file.size > MAX_IMAGE_BYTES) {
        attachError = 'Images must be 8 MB or smaller.';
        continue;
      }
      const reader = await new Promise<string>(resolve => {
        const stream = new FileReader();
        stream.onload = () => resolve(typeof stream.result === 'string' ? stream.result : '');
        stream.onerror = () => resolve('');
        stream.readAsDataURL(file);
      });
      const comma = reader.indexOf(',');
      if (comma < 0) {
        attachError = 'That image could not be read.';
        continue;
      }
      draftImages = [...draftImages, {
        id: crypto.randomUUID(),
        name: file.name || `image.${mimeType.split('/')[1]}`,
        mimeType,
        bytes: file.size,
        data: reader.slice(comma + 1),
        url: reader,
      }];
    }
  }


  function onComposerPaste(event: ClipboardEvent): void {
    const clipboard = event.clipboardData;
    if (!clipboard) return;
    const files = Array.from(clipboard.items)
      .filter(item => item.kind === 'file' && IMAGE_TYPES.includes((item.type || '').toLowerCase()))
      .map(item => item.getAsFile())
      .filter((file): file is File => Boolean(file));
    if (files.length) {
      event.preventDefault();
      void addImages(files);
      return;
    }
    const html = clipboard.getData('text/html');
    const match = html ? /<img[^>]+src="(data:image\/[a-z]+;base64,[^"]+)"/i.exec(html) : null;
    if (match) {
      event.preventDefault();
      const header = /^data:(image\/[a-z]+);base64,/i.exec(match[1]);
      if (!header) return;
      const base64 = match[1].slice(header[0].length);
      if (base64.length * 0.75 > MAX_IMAGE_BYTES) {
        attachError = 'Images must be 8 MB or smaller.';
        return;
      }
      if (draftImages.length >= MAX_IMAGES) {
        attachError = `Attach up to ${MAX_IMAGES} images per message.`;
        return;
      }
      draftImages = [...draftImages, {
        id: crypto.randomUUID(),
        name: 'pasted-image',
        mimeType: header[1].toLowerCase(),
        bytes: Math.round(base64.length * 0.75),
        data: base64,
        url: match[1],
      }];
    }
  }

  function onComposerDrop(event: DragEvent): void {
    dragOver = false;
    const files = event.dataTransfer?.files;
    if (!files?.length) return;
    event.preventDefault();
    void addImages(files);
  }

  function onComposerKeydown(event: KeyboardEvent): void {
    if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
      event.preventDefault();
      void submit();
    }
  }


  function openPicker(): void {
    pickerQuery = '';
    const enabled = pickerFilteredOptions.filter(option => !option.disabled);
    const selectedUsable = pickerSelected && !pickerSelected.disabled && pickerFilteredOptions.some(option => option.key === pickerSelected.key);
    pickerActive = selectedUsable ? pickerSelected!.key : enabled[0]?.key ?? '';
    pickerOpen = true;

    tick().then(() => {
      pickerSearchEl?.focus();
    });
  }

  function closePicker(refocus = false): void {
    pickerOpen = false;
    if (refocus) triggerEl?.focus();
  }

  function movePicker(delta: number, jump?: number): void {
    const enabled = pickerFilteredOptions.filter(option => !option.disabled);
    if (!enabled.length) return;
    const index = enabled.findIndex(option => option.key === pickerActive);
    let next: number;
    if (jump !== undefined) next = jump < 0 ? 0 : enabled.length - 1;
    else if (index < 0) next = delta > 0 ? 0 : enabled.length - 1;
    else next = (index + delta + enabled.length) % enabled.length;
    pickerActive = enabled[next].key;
    menuEl?.querySelector<HTMLElement>(`[data-key="${CSS.escape(pickerActive)}"]`)?.focus();
  }

  function onPickerKeydown(event: KeyboardEvent): void {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp' || event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      openPicker();
    }
  }

  function onMenuKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape' || event.key === 'Tab') { event.preventDefault(); closePicker(true); return; }
    if (event.key === 'ArrowDown') { event.preventDefault(); movePicker(1); return; }
    if (event.key === 'ArrowUp') { event.preventDefault(); movePicker(-1); return; }
    if (event.key === 'Home') { event.preventDefault(); movePicker(0, 0); return; }
    if (event.key === 'End') { event.preventDefault(); movePicker(0, 1); return; }
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      const option = pickerFilteredOptions.find(item => item.key === pickerActive);
      if (option && !option.disabled) void pickModel(option);
    }
  }

  function onPickerSearch(event: Event): void {
    pickerQuery = (event.currentTarget as HTMLInputElement).value;
    const enabled = pickerFilteredOptions.filter(option => !option.disabled);
    if (!enabled.some(option => option.key === pickerActive)) pickerActive = enabled[0]?.key ?? '';
  }

  function onPickerSearchKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape' || event.key === 'Tab') { event.preventDefault(); closePicker(true); return; }
    if (event.key === 'ArrowDown') { event.preventDefault(); movePicker(1); return; }
    if (event.key === 'ArrowUp') { event.preventDefault(); movePicker(-1); return; }
    if (event.key === 'Enter') {
      event.preventDefault();
      const option = pickerFilteredOptions.find(item => item.key === pickerActive);
      if (option && !option.disabled) void pickModel(option);
    }
  }

  async function pickModel(option: PickerOption): Promise<void> {
    if (option.disabled) return;
    closePicker(true);
    const next = parseSelectionKey(option.key);
    if (next) await chooseModel(next);
  }

  function onWindowPointerDown(event: PointerEvent): void {
    if (pickerOpen && !(pickerEl && event.target instanceof Node && pickerEl.contains(event.target))) closePicker();
    if (thinkingOpen && !(thinkingWrapEl && event.target instanceof Node && thinkingWrapEl.contains(event.target))) closeThinking();
    if (confirmingDelete && !(event.target instanceof Node && (event.target as Element).closest?.('.thread-confirm, .thread-delete'))) confirmingDelete = null;
  }


  async function deleteThread(id: string): Promise<void> {
    const wasActive = chat.active?.conversation.id === id;
    const next = chat.conversations.find(conversation => conversation.id !== id
      && !conversation.archived
      && (conversation.workspaceId ?? 'general') === library.activeWorkspace);
    confirmingDelete = null;
    if (!(await mutateLibrary('conversation_delete', { id }))) return;
    if (wasActive && next) await openConversation(next.id);
  }

  function thinkingLabel(level: string): string {
    return level === 'xhigh' ? 'X-High' : level.charAt(0).toUpperCase() + level.slice(1);
  }


  function openThinking(): void {
    thinkingActive = thinkingValue;
    thinkingOpen = true;
    tick().then(() => {
      thinkingMenuEl?.querySelector<HTMLElement>(`[data-thinking="${CSS.escape(thinkingActive)}"]`)?.focus();
    });
  }

  function closeThinking(refocus = false): void {
    thinkingOpen = false;
    if (refocus) thinkingTriggerEl?.focus();
  }

  function moveThinking(delta: number, jump?: number): void {
    if (!thinkingOptions.length) return;
    const index = thinkingOptions.indexOf(thinkingActive as never);
    let next: number;
    if (jump !== undefined) next = jump < 0 ? 0 : thinkingOptions.length - 1;
    else if (index < 0) next = delta > 0 ? 0 : thinkingOptions.length - 1;
    else next = (index + delta + thinkingOptions.length) % thinkingOptions.length;
    thinkingActive = thinkingOptions[next];
    thinkingMenuEl?.querySelector<HTMLElement>(`[data-thinking="${CSS.escape(thinkingActive)}"]`)?.focus();
  }

  function onThinkingPillKeydown(event: KeyboardEvent): void {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp' || event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      openThinking();
    }
  }

  function onThinkingKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape' || event.key === 'Tab') { event.preventDefault(); closeThinking(true); return; }
    if (event.key === 'ArrowDown') { event.preventDefault(); moveThinking(1); return; }
    if (event.key === 'ArrowUp') { event.preventDefault(); moveThinking(-1); return; }
    if (event.key === 'Home') { event.preventDefault(); moveThinking(0, 0); return; }
    if (event.key === 'End') { event.preventDefault(); moveThinking(0, 1); return; }
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      if (thinkingActive) void pickThinking(thinkingActive as ThinkingLevelName);
    }
  }

  async function pickThinking(level: ThinkingLevelName): Promise<void> {
    closeThinking(true);
    await setThinkingLevel(level);
  }

  function onLogScroll(): void {
    if (!logEl || changingHistory) return;
    pinned = logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight < 90;
    if (logEl.scrollTop < 500 && active?.before) void pageHistory('before');
    else if (pinned && active?.after) void pageHistory('after');
  }

  async function pageHistory(direction: 'before' | 'after'): Promise<void> {
    const viewport = logEl;
    if (!viewport || changingHistory || chat.historyLoading) return;
    changingHistory = true;
    pinned = false;
    const top = viewport.getBoundingClientRect().top;
    const anchor = [...viewport.querySelectorAll<HTMLElement>('[data-message-id]')]
      .find(element => element.getBoundingClientRect().bottom > top);
    const id = anchor?.dataset.messageId;
    const offset = anchor ? anchor.getBoundingClientRect().top - top : 0;
    await loadHistory(direction);
    await tick();
    if (id) {
      const replacement = [...viewport.querySelectorAll<HTMLElement>('[data-message-id]')].find(element => element.dataset.messageId === id);
      if (replacement) viewport.scrollTop += replacement.getBoundingClientRect().top - top - offset;
    }
    changingHistory = false;
  }

  async function latest(): Promise<void> {
    await jumpToLatest();
    pinned = true;
    await tick();
    if (logEl) logEl.scrollTop = logEl.scrollHeight;
  }

  function relativeDate(value: string): string {
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return '';
    const now = new Date();
    const sameDay = date.toDateString() === now.toDateString();
    if (sameDay) return date.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
    const yesterday = new Date(now);
    yesterday.setDate(now.getDate() - 1);
    if (date.toDateString() === yesterday.toDateString()) return 'Yesterday';
    if (now.getTime() - date.getTime() < 7 * 24 * 60 * 60 * 1000) {
      return date.toLocaleDateString([], { weekday: 'short' });
    }
    return date.toLocaleDateString([], { month: 'short', day: 'numeric' });
  }

  function modelName(message: ChatMessage): string {
    return message.status === 'failed' ? 'Reply failed'
      : message.status === 'cancelled' ? 'Reply stopped'
        : agentName;
  }

  function canRetry(index: number, message: ChatMessage): boolean {
    return message.role === 'assistant'
      && index === lastAssistantIndex
      && (message.status === 'failed' || message.status === 'cancelled')
      && !working;
  }



  type RenderedTurn =
    | { kind: 'message'; message: ChatMessage; index: number; firstOfGeneration: boolean }
    | { kind: 'turns'; id: string; turns: ChatMessage[]; firstOfGeneration: boolean };

  let expandedTurns = $state<Record<string, boolean>>({});
  let expandedReasoning = $state<Record<string, boolean>>({});
  function toggleTurns(id: string): void {
    expandedTurns[id] = !expandedTurns[id];
  }

  function isFirstOfGeneration(index: number): boolean {
    const message = messages[index];
    if (!message?.generationId) return false;
    return !messages.slice(0, index).some(previous => previous.role === 'assistant' && previous.generationId === message.generationId);
  }

  const renderedTurns = $derived.by(() => {
    const items: RenderedTurn[] = [];
    let pending: { turns: ChatMessage[]; head: number } | null = null;
    const flush = (): void => {
      if (!pending) return;
      if (pending.turns.length >= 2) {
        items.push({ kind: 'turns', id: `turns-${pending.turns[0].id}`, turns: pending.turns, firstOfGeneration: isFirstOfGeneration(pending.head) });
      } else {
        items.push({ kind: 'message', message: pending.turns[0], index: pending.head, firstOfGeneration: isFirstOfGeneration(pending.head) });
      }
      pending = null;
    };
    for (let index = 0; index < messages.length; index++) {
      const message = messages[index];


      const quiet = message.role === 'assistant' && message.status === 'completed'
        && !message.text && !message.error && !message.attachments?.length;
      if (quiet) {
        if (!pending) pending = { turns: [], head: index };
        pending.turns.push(message);
        continue;
      }
      flush();
      items.push({ kind: 'message', message, index, firstOfGeneration: isFirstOfGeneration(index) });
    }
    flush();
    return items;
  });

</script>

<svelte:window onpointerdown={onWindowPointerDown} onkeydown={(event) => { if (event.key === 'Escape' && (confirmingDelete || viewingImage)) { confirmingDelete = null; viewingImage = null; } }} />

<div class="chat-workspace">
<div class="chat-layout">
  <aside class="threads" aria-label="Conversations">
    <div class="rail-inner">
      <div class="rail-greeting">
        <strong>{greeting}</strong>
        <span>{tagline}</span>
      </div>
      <button class="primary-chat" class:selected={primary} disabled={chat.creating || chat.preview} aria-current={primary ? 'true' : undefined} onclick={() => { void openConversation(PRIMARY_CONVERSATION_ID); }}>
        <span class="primary-avatar"><img src={mascot} alt="" /><span class:busy={primary && (working || activeTasks > 0)}></span></span>
        <span class="primary-copy"><strong>{agentName}</strong><small>{primaryStatus}</small></span>
      </button>
      <LibrarySidebar bind:showArchived />
      <div class="threads-head">
        <span>Other chats</span>
        <div class="threads-actions">
          <button
            class="new-thread"
            disabled={chat.creating || chat.preview}
            title="Start a new conversation"
            onclick={() => { void createConversation(); }}
          >New</button>
        </div>
      </div>
      {#if otherChats.length}
        <ul class="thread-list">
          {#each otherChats as conversation (conversation.id)}
            <li class:active={chat.active?.conversation.id === conversation.id}>
              <button
                class="thread-open"
                disabled={chat.creating}
                aria-current={chat.active?.conversation.id === conversation.id ? 'true' : undefined}
                title={conversation.title || 'New conversation'}
                onclick={() => { confirmingDelete = null; void openConversation(conversation.id); }}
              >
                <span class="thread-title">{conversation.title || 'New conversation'}</span>
                <span class="thread-meta">
                  <span>{relativeDate(conversation.updatedAt)}</span>
                  {#if conversation.selection}<span class="thread-model">{selectionLabel(conversation.selection)}</span>{/if}
                </span>
                {#if chat.openingId === conversation.id}<span class="thread-loading">Opening</span>{/if}
              </button>
              {#if confirmingDelete === conversation.id}
                <div class="thread-confirm" role="group" aria-label="Delete this chat">
                  <span>Delete this chat?</span>
                  <div class="thread-confirm-actions">
                    <button class="danger" disabled={library.busy} onclick={() => { void deleteThread(conversation.id); }}>Delete</button>
                    <button onclick={() => { confirmingDelete = null; }}>Keep</button>
                  </div>
                </div>
              {:else}
                <button
                  class="thread-delete"
                  title="Delete chat"
                  aria-label={`Delete ${conversation.title || 'this conversation'}`}
                  disabled={library.busy}
                  onclick={() => { confirmingDelete = conversation.id; }}
                ><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><path d="M4 7h16" /><path d="M9.5 4.5h5" /><path d="M6.5 7l.9 12a1.8 1.8 0 0 0 1.8 1.6h5.6a1.8 1.8 0 0 0 1.8-1.6l.9-12" /><path d="M10.5 11v6" /><path d="M13.5 11v6" /></svg></button>
              {/if}
            </li>
          {/each}
        </ul>
      {:else}
        <p class="threads-empty">Start a separate chat for a focused topic.</p>
      {/if}
      {#if working}<p class="threads-note">Talo is replying. You can switch chats; new messages to this chat queue in order.</p>{/if}
    </div>
  </aside>

  <section class="conversation" aria-label="Conversation">
    {#if chat.child}
      <ChildTaskView snapshot={chat.child} parent={chat.childParent} task={delegations.find(task => task.childConversationId === chat.child?.conversation.id) ?? null} activity={chat.activity[delegations.find(task => task.childConversationId === chat.child?.conversation.id)?.id ?? ''] ?? []} agentName={agentName} onBack={closeDelegationTask} />
    {:else}
    <div class="conversation-head">
      <div class="conversation-lead">
        <div class="conversation-title">
          <strong>{primary ? agentName : active?.conversation.title || 'A new conversation'}</strong>
          <span>{primary ? primaryStatus : selection ? selectionLabel(selection) : 'No model chosen yet'}</span>
        </div>
      </div>

    </div>

    {#if active?.error && active.error !== chat.error}<div class="chat-message error" role="alert">{active.error}</div>{/if}
    {#if chat.error}
      <div class="chat-message error" role="alert">
        <span>{chat.error}</span>
        <button aria-label="Dismiss error" onclick={dismissError}>×</button>
      </div>
    {/if}
    {#if chat.notice}
      <div class="chat-message notice" role="status">
        <span>{chat.notice}</span>
        <button aria-label="Dismiss notice" onclick={dismissNotice}>×</button>
      </div>
    {/if}

    {#if chat.phase === 'loading'}
      <div class="welcome connect" role="status">
        <img src={mascot} alt="Talo, a rounded yellow companion with two black eyes" />
        <span class="loading-dot" aria-hidden="true"></span>
        <h2>Opening your conversations…</h2>
        <p>Bringing your threads and connected models back into view.</p>
      </div>
    {:else if chat.phase === 'error'}
      <div class="welcome connect">
        <img src={mascot} alt="Talo, a rounded yellow companion with two black eyes" />
        <h2>Your conversations did not open.</h2>
        <p>The desktop app is still running. Try once more, or check Settings if this keeps happening.</p>
        <button class="primary" onclick={() => { void retryChat(); }}>Try again</button>
        <button class="secondary" onclick={onOpenSettings}>Open Settings</button>
      </div>
    {:else if !connected.length}
      <div class="welcome connect">
        <img src={mascot} alt="Talo, a rounded yellow companion with two black eyes" />
        <h2>Let's get you connected.</h2>
        <p>Add a provider key in Settings, then choose a model.<br />Your side of the conversation is waiting.</p>
        <button class="primary" disabled={chat.preview} onclick={onOpenSettings}>Connect a provider</button>
        <small>Keys are sent once to the desktop app and are never kept in this window.</small>
      </div>
    {:else if messages.length === 0}
      <div class="welcome">
        <img src={mascot} alt="Talo, a rounded yellow companion with two black eyes" />
        <h2>Here when you need a hand.</h2>
        <p>A thought, a question, a place to begin.<br />Your assistant will meet you here.</p>
        <div class="suggestions">
          <button onclick={() => useSuggestion('Help me think through something on my mind.')}>
            <span>Think together</span>
            <small>Make room for your next idea.</small>
          </button>
          <button onclick={() => useSuggestion('Can you help me find my bearings on this?')}>
            <span>Find your bearings</span>
            <small>Bring a little clarity to your day.</small>
          </button>
          <button onclick={() => useSuggestion('Let’s pick up the thread from where I left off.')}>
            <span>Pick up the thread</span>
            <small>Keep what matters close.</small>
          </button>
        </div>
      </div>
    {:else}
      <div class="log" bind:this={logEl} onscroll={onLogScroll} aria-live="polite" aria-busy={working} role="log">
        {#each renderedTurns as item (item.kind === 'turns' ? item.id : item.message.id)}
          <VirtualMessage id={item.kind === 'turns' ? item.id : item.message.id} root={logEl} onResize={() => { if (pinned && !active?.after && !changingHistory && logEl) logEl.scrollTop = logEl.scrollHeight; }}>
          {#if item.kind === 'turns'}
            <article class="message assistant turns-group">
              <img class="avatar" src={mascot} alt="" aria-hidden="true" />
              <div class="message-body">
                <div class="message-role"><span>{agentName}</span></div>
                {#if item.firstOfGeneration && item.turns[0].generationId && chat.research[item.turns[0].generationId]?.length}
                  <WebResearch items={chat.research[item.turns[0].generationId]} working={chat.active?.generationId === item.turns[0].generationId} onCancel={() => void stopGeneration()} />
                {/if}
                {#if calledTasks(item.turns[0].generationId).length}
                  <DelegationCalls tasks={calledTasks(item.turns[0].generationId)} onOpen={(task) => { void openDelegationTask(task.id); }} />
                {/if}
                {#if showReasoning && item.turns.some(turn => turn.reasoning)}
                  <button type="button" class="turns-toggle" aria-expanded={Boolean(expandedTurns[item.id])} onclick={() => toggleTurns(item.id)}>
                    <svg class="turns-chevron" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><path d="m6 9.5 6 6 6-6" /></svg>
                    Worked through it · {item.turns.length} steps
                  </button>
                  {#if expandedTurns[item.id]}
                    <div class="turns-list">
                      {#each item.turns as turn (turn.id)}
                        {#if turn.reasoning}<Reasoning text={turn.reasoning} thinking={false} hasReply={false} savedOpen={expandedReasoning[turn.id]} onToggle={open => { expandedReasoning[turn.id] = open; }} />{/if}
                      {/each}
                    </div>
                  {/if}
                {:else}
                  <p class="turns-summary">Worked through it in {item.turns.length} steps.</p>
                {/if}
              </div>
            </article>
          {:else}
            {@const message = item.message}
            {@const index = item.index}
            <article class="message {message.role}" class:streaming={message.status === 'generating'}>
            {#if message.role === 'assistant'}
              <img class="avatar" class:busy={message.status === 'generating'} src={mascot} alt="" aria-hidden="true" />
            {/if}
            <div class="message-body">
              <div class="message-role">
                <span title={message.role === 'assistant' ? modelName(message) : undefined}>{message.role === 'user' ? 'You' : agentName}</span>
                {#if message.status === 'cancelled'}<span class="status-tag">Stopped</span>{/if}
                {#if message.status === 'failed'}<span class="status-tag failed">Failed</span>{/if}
              </div>
              {#if message.role === 'assistant' && message.generationId && chat.research[message.generationId]?.length && !messages.slice(0,index).some(previous => previous.role === 'assistant' && previous.generationId === message.generationId)}
                <WebResearch items={chat.research[message.generationId]} working={message.status === 'generating'} onCancel={() => void stopGeneration()} />
              {/if}
              {#if message.role === 'assistant' && message.generationId && calledTasks(message.generationId).length}
                <DelegationCalls tasks={calledTasks(message.generationId)} onOpen={(task) => { void openDelegationTask(task.id); }} />
              {/if}
              {#if message.attachments?.length}
                <div class="message-images">
                  {#each message.attachments as attachment (attachment.id)}
                    {#if attachment.url}
                      <button
                        type="button"
                        class="message-image"
                        title={attachment.name}
                        onclick={() => { viewingImage = attachment.url ?? null; viewingName = attachment.name; }}
                      ><img src={attachment.url} alt={attachment.name} loading="lazy" /></button>
                    {:else}
                      <span class="message-image missing" title="Too large to preview here">{attachment.name}</span>
                    {/if}
                  {/each}
                </div>
              {/if}
              {#if showReasoning && message.reasoning}
                <Reasoning
                  text={message.reasoning}
                  savedOpen={expandedReasoning[message.id]}
                  onToggle={open => { expandedReasoning[message.id] = open; }}
                  thinking={message.status === 'generating' && !message.text}
                  hasReply={Boolean(message.text)}
                />
              {/if}
              {#if message.text}
                <MessageBody text={message.text} />
              {:else if message.status === 'generating'}
                <span class="typing" aria-label="Talo is replying"><i></i><i></i><i></i></span>
              {/if}
              {#if message.status === 'generating' && message.text}
                <span class="cursor" aria-hidden="true"></span>
              {/if}
              {#if message.error}
                <p class="message-error">{message.error}</p>
              {/if}
              {#if canRetry(index, message)}
                <button class="retry" disabled={chat.preview} onclick={() => { void retryGeneration(); }}>Try again</button>
              {/if}
            </div>
          </article>
          {/if}
          </VirtualMessage>
        {/each}
        {#if unattachedTasks.length}
          <DelegationCalls tasks={unattachedTasks} onOpen={(task) => { void openDelegationTask(task.id); }} />
        {/if}
      </div>
      {#if active?.after}<button class="secondary" onclick={() => void latest()}>Jump to latest</button>{/if}
    {/if}

    {#if chat.phase === 'ready'}
    {#each asksFor(active?.conversation.id) as ask (ask.id)}
      <AskCard {ask} />
    {/each}
    {#if queued.length}
      <div class="queued" role="status" aria-label="Queued messages">
        <span>{queued.length === 1 ? 'Queued message' : `${queued.length} queued messages`} — sent after the current reply.</span>
        {#each queued as turn (turn.id)}<p>{turn.text}</p>{/each}
      </div>
    {/if}
    <form
      class="composer"
      class:dragging={dragOver}
      onsubmit={(event) => { event.preventDefault(); void submit(); }}
      ondragover={(event) => { if (!event.dataTransfer?.types.includes('Files')) return; event.preventDefault(); dragOver = true; }}
      ondragleave={() => { dragOver = false; }}
      ondrop={onComposerDrop}
    >
      {#if active?.state === 'failed' && !chat.sending}
        <div class="composer-note">That reply did not finish. <button type="button" disabled={chat.preview} onclick={() => { void retryGeneration(); }}>Try again</button></div>
      {/if}
      {#if draftImages.length}
        <div class="drafts" role="list" aria-label="Attached images">
          {#each draftImages as image (image.id)}
            <div class="draft" role="listitem">
              <img src={image.url} alt={image.name} />
              <button
                type="button"
                class="draft-remove"
                aria-label={`Remove ${image.name}`}
                disabled={working}
                onclick={() => removeDraft(image.id)}
              >×</button>
            </div>
          {/each}
        </div>
      {/if}
      {#if attachError}
        <p class="attach-note" role="alert">{attachError}</p>
      {:else if draftImages.length && !imagesSupported}
        <p class="attach-note">This model can’t see images, so Talo names the file path in Pi’s workspace instead.</p>
      {/if}
      <textarea
        bind:this={composerEl}
        bind:value={text}
        aria-label="Message to Talo"
        placeholder={draftImages.length ? 'Add a note about the image…' : placeholder}
        rows="1"
        disabled={!connected.length}
        oninput={resize}
        onkeydown={onComposerKeydown}
        onpaste={onComposerPaste}
      ></textarea>
      <div class="composer-foot">
        <div class="model-picker" bind:this={pickerEl}>
          <label class="picker-label" id="chat-model-label" for="chat-model">Model</label>
          <div class="picker">
            <button
              type="button"
              id="chat-model"
              class="picker-trigger"
              bind:this={triggerEl}
              aria-haspopup="listbox"
              aria-expanded={pickerOpen}
              aria-controls={pickerOpen ? 'chat-model-menu' : undefined}
              aria-labelledby="chat-model-label"
              disabled={working || chat.selectingModel || !connected.length}
              onclick={() => { if (pickerOpen) closePicker(); else openPicker(); }}
              onkeydown={onPickerKeydown}
            >
              <span class="picker-value" class:unset={!pickerSelected}>{pickerValue}</span>
              <svg class="picker-chevron" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><path d="m6 9.5 6 6 6-6" /></svg>
            </button>
            {#if pickerOpen}
              <div class="picker-menu" id="chat-model-menu" bind:this={menuEl}>
                <input
                  class="picker-search"
                  type="text"
                  placeholder="Search models…"
                  aria-label="Search models"
                  bind:this={pickerSearchEl}
                  value={pickerQuery}
                  oninput={onPickerSearch}
                  onkeydown={onPickerSearchKeydown}
                />
                <div class="picker-list" role="listbox" aria-labelledby="chat-model-label">
                  {#each pickerFilteredGroups as group (group.id)}
                    <div class="picker-group">{group.name}</div>
                    {#each group.options as option (option.key)}
                      <div
                        role="option"
                        data-key={option.key}
                        class="picker-option"
                        class:selected={option.selected}
                        class:active={pickerActive === option.key}
                        class:disabled={option.disabled}
                        aria-selected={option.selected}
                        aria-disabled={option.disabled}
                        tabindex={pickerActive === option.key ? 0 : -1}
                        onfocus={() => { pickerActive = option.key; }}
                        onpointerenter={() => { if (!option.disabled) pickerActive = option.key; }}
                        onkeydown={(event) => { event.stopPropagation(); onMenuKeydown(event); }}
                        onclick={() => { void pickModel(option); }}
                      >
                        <span class="picker-dot" aria-hidden="true"></span>
                        <span class="picker-name">{option.label}</span>
                        {#if option.reasoning}<span class="picker-tag">reasoning</span>{/if}
                      </div>
                    {/each}
                  {/each}
                  {#if !pickerFilteredOptions.length}
                    <p class="picker-empty">No models match “{pickerQuery.trim()}”</p>
                  {/if}
                </div>
              </div>
            {/if}
          </div>
          {#if chat.selectingModel}<span class="picker-status" role="status">Saving…</span>{/if}
        </div>
        <div class="send-area">
          <button
            type="button"
            class="skill-chip"
            title="Talo drafts the steps you just used, then asks you to review them in the Inbox"
            aria-label="Save this workflow as a skill"
            disabled={!connected.length || working || chat.sending || chat.preview}
            onclick={() => void sendMessage(skillRequest(''))}
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3.8 13.5 8l4.2 1.5-4.2 1.5L12 15.2l-1.5-4.2L6.3 9.5 10.5 8 12 3.8Z"/><path d="M17.4 15.2l.7 1.8 1.8.7-1.8.7-.7 1.8-.7-1.8-1.8-.7 1.8-.7.7-1.8Z"/></svg><span>Save as skill</span>
          </button>
          <button type="button" class="browser-chip" class:browser-active={browser.status.state === 'connected'} aria-label="Open browser preview" aria-expanded={browser.view !== 'closed'} disabled={chat.preview} onclick={() => browser.view === 'side' ? closeBrowser() : void openBrowser('side')} title="Watch the browser without interrupting Pi">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="3"/><path d="M3 9h18M7 6.5h.01M10 6.5h.01"/></svg><span>Browser</span>{#if browser.status.busy}<span class="browser-dot"></span>{/if}
          </button>
          {#if composerHint}<span class="composer-hint">{composerHint}</span>{/if}
          {#if thinkingOptions.length && routing.mode === 'manual'}
            <div class="thinking-wrap" bind:this={thinkingWrapEl}>
              <button
                type="button"
                id="chat-thinking"
                class="thinking-pill"
                bind:this={thinkingTriggerEl}
                aria-haspopup="listbox"
                aria-expanded={thinkingOpen}
                aria-controls={thinkingOpen ? 'chat-thinking-menu' : undefined}
                disabled={working || chat.thinkingBusy || !connected.length}
                onclick={() => { if (thinkingOpen) closeThinking(); else openThinking(); }}
                onkeydown={onThinkingPillKeydown}
                title="How much the model reasons before replying"
              >
                <span class="thinking-name">Thinking</span>
                <span class="thinking-value">{thinkingLabel(thinkingValue)}</span>
                <svg class="thinking-chevron" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><path d="m6 9.5 6 6 6-6" /></svg>
              </button>
              {#if thinkingOpen}
                <div class="thinking-menu" id="chat-thinking-menu" role="listbox" tabindex="-1" aria-labelledby="chat-thinking" bind:this={thinkingMenuEl}>
                  {#each thinkingOptions as option (option)}
                    <div
                      role="option"
                      data-thinking={option}
                      class="thinking-option"
                      class:selected={option === thinkingValue}
                      class:active={thinkingActive === option}
                      aria-selected={option === thinkingValue}
                      tabindex={thinkingActive === option ? 0 : -1}
                      onfocus={() => { thinkingActive = option; }}
                      onpointerenter={() => { thinkingActive = option; }}
                      onkeydown={(event) => { event.stopPropagation(); onThinkingKeydown(event); }}
                      onclick={() => { void pickThinking(option as ThinkingLevelName); }}
                    >
                      <span class="picker-dot" aria-hidden="true"></span>
                      <span>{thinkingLabel(option)}</span>
                    </div>
                  {/each}
                </div>
              {/if}
            </div>
          {/if}
          {#if working}
            <button
              type="button"
              class="stop"
              class:stopping={cancelling}
              disabled={cancelling || chat.sending}
              onclick={() => { void stopGeneration(); }}
              aria-label={cancelling ? 'Stopping reply' : 'Stop reply'}
            >{cancelling ? 'Stopping…' : 'Stop'}</button>
          {/if}
          <button
            type="button"
            class="attach"
            title="Attach an image"
            aria-label="Attach an image"
            disabled={!connected.length || working || chat.sending || draftImages.length >= MAX_IMAGES}
            onclick={() => fileInput?.click()}
          ><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><path d="M12 15.5V10a2.5 2.5 0 0 0-5 0v6a4.5 4.5 0 0 0 9 0V8a2 2 0 1 0-4 0v7" /><path d="M17 6.5h1a3 3 0 0 1 3 3v5" /></svg></button>
          <input
            class="file-input"
            type="file"
            accept={IMAGE_TYPES.join(',')}
            multiple
            bind:this={fileInput}
            onchange={(event) => {
              const input = event.currentTarget;
              void addImages(input.files).then(() => { input.value = ''; });
            }}
          />
          <button
            type="submit"
            class="send"
            disabled={!connected.length || !selection || !selectionAvailable || (!text.trim() && !draftImages.length) || chat.sending || chat.preview}
            aria-label={working ? 'Queue message' : chat.sending ? 'Sending message' : 'Send message'}
            aria-busy={chat.sending}
          ><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><path d="M12 19.5V5.5" /><path d="m5.5 11.5 6.5-6.5 6.5 6.5" /></svg></button>
        </div>
      </div>
    </form>
    {/if}
    {/if}
  </section>
</div>
<AgentCompanion name={agentName} onRename={onRenameAgent} />
</div>

{#if viewingImage}
  <div class="lightbox" role="dialog" aria-modal="true" aria-label="Image preview">
    <button type="button" class="lightbox-backdrop" aria-label="Close preview" onclick={() => { viewingImage = null; }}></button>
    <img src={viewingImage} alt={viewingName ?? 'Attachment'} />
    <button type="button" class="lightbox-close" aria-label="Close preview" onclick={() => { viewingImage = null; }}>×</button>
  </div>
{/if}

<style>
  .primary-chat{display:flex;align-items:center;gap:12px;text-align:left;width:100%;padding:14px 12px;margin:2px 0 18px;border:1px solid var(--line-strong);border-radius:12px;background:var(--soft);color:var(--text);cursor:pointer}
  .primary-chat.selected{background:#302d20;border-color:#655a32}.primary-chat:hover:not(:disabled){border-color:var(--yellow-deep)}.primary-chat:disabled{opacity:.5;cursor:not-allowed}.primary-avatar{position:relative;flex-shrink:0}.primary-avatar img{display:block;width:34px;height:34px}.primary-avatar>span{position:absolute;right:-2px;bottom:0;width:8px;height:8px;background:#9fb398;border:2px solid var(--panel);border-radius:50%}.primary-avatar>span.busy{background:var(--yellow)}.primary-copy{display:grid;gap:5px;min-width:0}.primary-copy strong{font-size:14px;font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.primary-copy small{font-size:10.5px;color:var(--muted);line-height:1.4}


  .chat-workspace{flex:1;min-height:0;min-width:0;display:flex;position:relative}
  .browser-chip,.skill-chip{display:inline-flex;align-items:center;gap:6px;background:transparent;border:1px solid var(--line);border-radius:8px;padding:6px 9px;color:var(--muted);font-size:10px;white-space:nowrap}.browser-chip svg,.skill-chip svg{width:14px;height:14px}.browser-chip:hover:not(:disabled),.browser-chip.browser-active,.skill-chip:hover:not(:disabled){color:var(--yellow);border-color:var(--line-strong)}.skill-chip:disabled{opacity:.45;cursor:not-allowed}.browser-dot{width:4px;height:4px;background:var(--yellow);border-radius:50%;animation:pulse 1.5s ease-in-out infinite}
  .chat-layout{--rail-w:240px;flex:1;min-width:0;min-height:0;display:grid;grid-template-columns:var(--rail-w) minmax(0,1fr)}
  .chat-layout>*{min-width:0}
  .threads{position:relative;z-index:2;border-right:1px solid var(--line);min-height:0;overflow:visible}
  .rail-inner{width:100%;height:100%;padding:2px 20px 8px 0;overflow-y:auto;display:flex;flex-direction:column;scrollbar-gutter:stable}
  .rail-greeting{padding:6px 2px 20px}
  .rail-greeting strong{display:block;font-size:21px;letter-spacing:-.8px;font-weight:560}
  .rail-greeting span{display:block;margin-top:6px;font-size:12px;color:var(--muted);line-height:1.55}
  .threads-head{display:flex;align-items:center;justify-content:space-between;gap:8px;margin-bottom:12px}
  .threads-actions{display:flex;align-items:center;gap:6px;flex-shrink:0}
  .threads-head>span{font-size:10.5px;color:var(--faint);letter-spacing:.1em;text-transform:uppercase}
  .new-thread{background:transparent;border:1px solid var(--line);color:var(--text);padding:5px 10px;font-size:11px;border-radius:7px}
  .new-thread:hover:not(:disabled){border-color:var(--yellow-deep);color:var(--yellow)}
  .thread-list{list-style:none;padding:0;margin:0;display:grid;gap:3px;min-width:0}
  .thread-list li{position:relative;min-width:0}
  .thread-open{position:relative;width:100%;text-align:left;background:transparent;border:1px solid transparent;color:var(--muted);padding:10px 32px 10px 10px;border-radius:9px;display:block;line-height:1.4}
  .thread-open:hover:not(:disabled){background:#1d1d19;color:var(--text)}
  .thread-list li.active .thread-open{background:#26261f;color:var(--text);border-color:#34342c}
  .thread-list li.active .thread-open::before{content:'';position:absolute;left:-1px;top:12px;bottom:12px;width:2px;border-radius:2px;background:var(--yellow)}

  .thread-delete{position:absolute;top:7px;right:6px;z-index:2;display:grid;place-items:center;width:22px;height:22px;padding:0;background:transparent;border:1px solid transparent;border-radius:6px;color:var(--faint);opacity:0;transition:opacity .13s ease,color .15s ease,border-color .15s ease,background .15s ease}
  .thread-delete svg{width:13px;height:13px;display:block}
  .thread-list li:hover .thread-delete,.thread-list li:focus-within .thread-delete,.thread-list li.active .thread-delete{opacity:1}
  .thread-delete:hover:not(:disabled){color:var(--error-text);border-color:var(--error-line);background:var(--error-bg)}
  @media(hover:none){.thread-delete{opacity:1}}
  .thread-confirm{position:absolute;inset:0;z-index:3;display:grid;align-content:center;gap:7px;padding:9px 11px;background:#1c1c17;border:1px solid var(--error-line);border-radius:9px;box-shadow:0 10px 26px var(--shadow)}
  .thread-confirm>span{font-size:10.5px;color:var(--muted);line-height:1.35}
  .thread-confirm-actions{display:flex;gap:6px}
  .thread-confirm-actions button{width:auto;flex:1;text-align:center;padding:5px 8px;font-size:10.5px;line-height:1.3;border:1px solid var(--line-strong);border-radius:6px;background:var(--soft);color:var(--text)}
  .thread-confirm-actions .danger{border-color:var(--error-line);background:var(--error-bg);color:var(--error-text)}
  .thread-confirm-actions button:hover:not(:disabled){border-color:var(--line-strong);background:#2c2c26}
  .thread-confirm-actions .danger:hover:not(:disabled){border-color:#7a5a30;background:#33261a}
  .thread-title{display:block;font-size:12.5px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
  .thread-meta{display:flex;gap:7px;margin-top:4px;font-size:10px;color:var(--faint);white-space:nowrap;overflow:hidden}
  .thread-model{overflow:hidden;text-overflow:ellipsis;max-width:115px}
  .thread-loading{display:block;margin-top:4px;font-size:10px;color:var(--yellow-deep)}
  .threads-empty{font-size:11.5px;line-height:1.6;color:var(--faint);margin:10px 2px}
  .threads-note{font-size:10.5px;line-height:1.5;color:var(--faint);margin:14px 2px 0;border-top:1px solid var(--line);padding-top:12px}
  .conversation{width:100%;max-width:none;margin:0;min-height:0;display:flex;flex-direction:column;padding-left:18px}
  .conversation-head{display:flex;align-items:flex-start;justify-content:space-between;gap:14px;padding-bottom:13px;border-bottom:1px solid var(--line);margin-bottom:14px}
  .conversation-lead{display:flex;align-items:flex-start;gap:12px;min-width:0}
  .threads-reopen{align-self:center;padding:6px 10px;font-size:11px;color:var(--muted);gap:8px}
  .conversation-title{min-width:0}
  .conversation-title strong{display:block;font-size:15px;font-weight:550;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
  .conversation-title span{display:block;font-size:11px;color:var(--muted);margin-top:4px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
  .chat-message{display:flex;gap:12px;align-items:center;justify-content:space-between;padding:10px 13px;border-radius:9px;font-size:12.5px;margin-bottom:12px;line-height:1.5}
  .chat-message.error{background:var(--error-bg);border:1px solid var(--error-line);color:var(--error-text)}
  .chat-message.notice{background:#211f16;border:1px solid #4e4324;color:#e8d59e}
  .chat-message button{background:transparent;border:0;color:inherit;padding:0 4px;font-size:16px;line-height:1}

  .welcome{flex:1;min-height:0;overflow-y:auto;display:flex;flex-direction:column;align-items:center;justify-content:center;text-align:center;padding:24px 12px;width:100%;max-width:640px;align-self:center}
  .welcome img{width:84px;margin-bottom:22px;filter:drop-shadow(0 0 36px rgba(255,212,92,.22)) drop-shadow(0 0 90px rgba(255,212,92,.1));animation:mascot-bob 5.5s ease-in-out infinite}
  .welcome h2{font-size:23px;letter-spacing:-.6px;font-weight:550;margin:0 0 12px}
  .welcome p{color:var(--muted);font-size:14.5px;line-height:1.7;margin:0 0 21px}
  .welcome small{display:block;margin-top:18px;color:var(--faint);font-size:11px;line-height:1.6}
  .secondary{background:transparent;margin-left:8px}
  .loading-dot{display:block;width:7px;height:7px;border-radius:50%;background:var(--yellow);margin:0 auto 18px;animation:pulse 1.4s ease-in-out infinite}
  .suggestions{display:grid;grid-template-columns:repeat(3,1fr);gap:16px;margin-top:26px;text-align:left;width:100%}
  .suggestions button{background:transparent;border:0;border-top:1px solid var(--line);padding:14px 2px 0;color:var(--text);border-radius:0;display:block}
  .suggestions button:hover:not(:disabled){background:transparent;border-top-color:var(--yellow-deep)}
  .suggestions button:hover:not(:disabled) span{color:var(--yellow)}
  .suggestions span{display:block;font-size:13px;font-weight:600;transition:color .15s ease}
  .suggestions small{display:block;margin-top:6px;font-size:11.5px;color:var(--muted);line-height:1.5}
  .log{flex:1;min-height:0;overflow-y:auto;overflow-anchor:none;padding:8px 10px 20px 2px;scrollbar-gutter:stable}
  .message{display:flex;gap:12px;padding:14px 0 20px;border-bottom:1px solid var(--line);animation:msg-in .32s cubic-bezier(.2,.7,.3,1)}
  .message:last-child{border-bottom:0}
  .message.user{justify-content:flex-end}
  .avatar{width:27px;height:27px;margin-top:2px;flex-shrink:0;transform-origin:50% 80%}
  .avatar.busy{animation:avatar-think 1.4s ease-in-out infinite;filter:drop-shadow(0 0 9px rgba(255,212,92,.35))}
  .message-body{min-width:0;max-width:92%}
  .message.user .message-body{max-width:82%;background:#23231e;border:1px solid #33332a;border-radius:13px;padding:10px 14px}
  .message-role{display:flex;align-items:center;gap:8px;font-size:11px;color:var(--muted);margin-bottom:6px;letter-spacing:.01em}
  .message.user .message-role{justify-content:flex-end;margin-bottom:4px}
  .message.streaming .message-role>span:first-child{color:var(--yellow)}
  .status-tag{font-size:9.5px;border:1px solid var(--line);border-radius:20px;padding:2px 7px;color:var(--muted)}
  .status-tag.failed{border-color:var(--error-line);color:var(--error-text)}
  .typing{display:inline-flex;gap:5px;padding:7px 0}
  .typing i{width:5px;height:5px;border-radius:50%;background:var(--yellow-deep);animation:bob 1.1s ease-in-out infinite}
  .typing i:nth-child(2){animation-delay:.15s}
  .typing i:nth-child(3){animation-delay:.3s}
  .cursor{display:inline-block;width:2px;height:15px;background:var(--yellow);vertical-align:-3px;margin-left:2px;animation:blink 1s step-end infinite}
  .message-error{margin-top:9px;font-size:11.5px;color:var(--error-text);line-height:1.6}
  .retry{background:transparent;border:1px solid var(--line-strong);color:var(--text);font-size:11px;padding:6px 11px;border-radius:7px;margin-top:11px}
  .retry:hover:not(:disabled){border-color:var(--yellow-deep);color:var(--yellow)}

  .turns-group .message-role{margin-bottom:4px}
  .turns-toggle{display:inline-flex;align-items:center;gap:6px;background:transparent;border:0;padding:2px 0;color:var(--faint);font-size:10.5px;letter-spacing:.08em;text-transform:uppercase;cursor:pointer}
  .turns-toggle:hover{color:var(--yellow-deep)}
  .turns-chevron{width:11px;height:11px;transition:transform .15s ease}
  .turns-toggle[aria-expanded="true"] .turns-chevron{transform:rotate(180deg)}
  .turns-list{margin-top:10px}
  .turns-list :global(.reasoning){margin:0 0 10px}
  .turns-summary{margin:2px 0 0;font-size:12px;color:var(--faint)}
  .queued{flex-shrink:0;border:1px dashed var(--line-strong);border-radius:10px;padding:8px 11px;margin-top:8px;font-size:11px;color:var(--muted)}
  .queued>span{display:block;font-size:10px;color:var(--faint);margin-bottom:4px}
  .queued p{margin:2px 0;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
  .composer{flex-shrink:0;border:1px solid var(--line-strong);border-radius:15px;padding:15px 16px 13px;background:var(--paper);box-shadow:0 12px 32px var(--shadow);margin-top:10px;transition:border-color .15s ease,background .15s ease}

  .composer.dragging{border-color:var(--yellow-deep);background:#1d1c16}
  .drafts{display:flex;flex-wrap:wrap;gap:8px;margin:0 0 11px}
  .draft{position:relative;width:62px;height:62px;border:1px solid var(--line-strong);border-radius:10px;overflow:hidden;background:var(--soft)}
  .draft img{width:100%;height:100%;object-fit:cover;display:block}
  .draft-remove{position:absolute;top:2px;right:2px;width:17px;height:17px;display:grid;place-items:center;padding:0;border:1px solid rgba(0,0,0,.45);border-radius:50%;background:rgba(11,11,10,.78);color:var(--text);font-size:11px;line-height:1}
  .draft-remove:hover:not(:disabled){background:var(--error-bg);color:var(--error-text);border-color:var(--error-line)}
  .attach-note{font-size:10.5px;color:var(--muted);margin:0 2px 9px;line-height:1.5}
  .file-input{display:none}
  .message-images{display:flex;flex-wrap:wrap;gap:7px;margin-bottom:9px}
  .message-image{padding:0;border:1px solid var(--line-strong);border-radius:9px;overflow:hidden;background:var(--soft);width:132px;height:132px;display:block;line-height:0;transition:border-color .15s ease}
  .message-image img{width:100%;height:100%;object-fit:cover;display:block}
  .message-image:hover:not(:disabled){border-color:var(--yellow-deep)}
  .message-image.missing{width:auto;height:auto;padding:7px 10px;font-size:10.5px;color:var(--faint);line-height:1.4;cursor:default}
  .message.user .message-images{justify-content:flex-end}
  .lightbox{position:fixed;inset:0;z-index:60;background:var(--overlay);backdrop-filter:blur(6px);display:grid;place-items:center;padding:40px}
  .lightbox-backdrop{position:absolute;inset:0;padding:0;border:0;background:transparent;cursor:default}
  .lightbox img{position:relative;max-width:100%;max-height:100%;border-radius:12px;box-shadow:0 30px 80px rgba(0,0,0,.6)}
  .lightbox-close{position:absolute;top:18px;right:18px;width:32px;height:32px;padding:0;display:grid;place-items:center;font-size:17px;border-radius:50%;border:1px solid var(--line-strong);background:var(--paper);color:var(--text)}
  .composer:focus-within{border-color:#4b4b40}
  .composer textarea{display:block;border:0;resize:none;background:transparent;color:var(--text);width:100%;height:54px;min-height:54px;max-height:220px;font-size:15px;line-height:1.65;padding:2px 2px 8px;overflow-y:auto}
  .composer-note{font-size:11px;color:var(--error-text);margin:0 2px 9px;padding-bottom:9px;border-bottom:1px solid var(--line)}
  .composer-note button{background:transparent;border:0;color:var(--yellow);padding:0;margin-left:5px;text-decoration:underline;font-size:11px}
  .composer-foot{display:flex;align-items:center;justify-content:space-between;gap:14px;row-gap:8px;border-top:1px solid var(--line);padding-top:10px;flex-wrap:wrap}
  .model-picker{display:flex;align-items:center;gap:8px;min-width:0}
  .model-picker .picker-label{font-size:10px;color:var(--faint);flex-shrink:0}
  .picker{position:relative;min-width:0}
  .picker-trigger{display:inline-flex;align-items:center;gap:7px;max-width:300px;background:transparent;border:1px solid transparent;border-radius:7px;color:var(--muted);font-size:11px;padding:5px 9px;line-height:1.4}
  .picker-trigger:hover:not(:disabled){border-color:var(--line);color:var(--text)}
  .picker-trigger[aria-expanded="true"]{border-color:var(--line-strong);color:var(--text);background:var(--soft)}
  .picker-trigger:disabled{opacity:.45}
  .picker-value{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .picker-value.unset{color:var(--faint)}
  .picker-chevron{width:11px;height:11px;flex-shrink:0;transition:transform .15s ease}
  .picker-trigger[aria-expanded="true"] .picker-chevron{transform:rotate(180deg)}

  .picker-menu{position:absolute;left:0;bottom:calc(100% + 8px);z-index:40;min-width:290px;max-width:min(430px,90vw);display:flex;flex-direction:column;background:var(--paper);border:1px solid var(--line-strong);border-radius:12px;padding:6px;box-shadow:0 18px 48px rgba(0,0,0,.55)}
  .picker-search{display:block;width:100%;background:var(--soft);border:1px solid var(--line);border-radius:8px;padding:7px 10px;color:var(--text);font:inherit;font-size:12px;margin-bottom:4px}
  .picker-list{max-height:min(330px,48vh);overflow-y:auto;scrollbar-gutter:stable}
  .picker-empty{margin:2px;padding:8px 10px;font-size:12px;color:var(--faint)}
  .picker-group{font-size:9.5px;letter-spacing:.09em;text-transform:uppercase;color:var(--faint);padding:9px 10px 5px}
  .picker-option{display:flex;align-items:center;gap:9px;width:100%;text-align:left;background:transparent;border:0;border-radius:8px;padding:8px 10px;font-size:12.5px;color:var(--text);cursor:pointer;line-height:1.4}
  .picker-option.active{background:#24241e}
  .picker-option.disabled{opacity:.45;cursor:not-allowed}
  .picker-dot{width:6px;height:6px;border-radius:50%;border:1px solid var(--line-strong);flex-shrink:0}
  .picker-option.selected .picker-dot{background:var(--yellow);border-color:var(--yellow)}
  .picker-name{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;min-width:0}
  .picker-option.selected .picker-name{font-weight:600}
  .picker-tag{font-size:9px;color:var(--yellow-deep);border:1px solid #4e4324;border-radius:20px;padding:2px 7px;white-space:nowrap;flex-shrink:0}
  .picker-status{font-size:10px;color:var(--faint)}

  .thinking-wrap{position:relative;min-width:0}
  .thinking-pill{display:flex;align-items:center;gap:6px;border:1px solid var(--line-strong);background:var(--soft);color:var(--muted);border-radius:9px;padding:0 8px;height:32px;cursor:pointer;transition:border-color .15s ease}
  .thinking-pill:hover:not(:disabled){border-color:#4a4a41;color:var(--text)}
  .thinking-pill:focus-visible{border-color:#4b4b40}
  .thinking-pill:disabled{opacity:.45;cursor:not-allowed}
  .thinking-name{font-size:10px;color:var(--faint);flex-shrink:0}
  .thinking-value{font-size:11px;color:var(--text);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;min-width:0}
  .thinking-chevron{width:11px;height:11px;flex-shrink:0;transition:transform .15s ease}
  .thinking-pill[aria-expanded="true"] .thinking-chevron{transform:rotate(180deg)}
  .thinking-menu{position:absolute;right:0;bottom:calc(100% + 8px);z-index:40;min-width:140px;background:var(--paper);border:1px solid var(--line-strong);border-radius:12px;padding:6px;box-shadow:0 18px 48px rgba(0,0,0,.55)}
  .thinking-option{display:flex;align-items:center;gap:9px;padding:7px 10px;border-radius:8px;font-size:12px;color:var(--text);cursor:pointer;line-height:1.4}
  .thinking-option.active{background:#24241e}
  .thinking-option .picker-dot{width:6px;height:6px;border-radius:50%;border:1px solid var(--line-strong);flex-shrink:0}
  .thinking-option.selected .picker-dot{background:var(--yellow);border-color:var(--yellow)}
  .thinking-option.selected{font-weight:600}
  @media(prefers-reduced-motion:reduce){
    .typing i,.cursor,.send[aria-busy="true"] svg,.loading-dot{animation:none}.thinking-chevron{transition:none}}
  .send-area{display:flex;align-items:center;gap:11px;flex-wrap:wrap;justify-content:flex-end;min-width:0}
  .composer-hint{font-size:10px;color:var(--faint);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;min-width:0}
  .send{background:var(--yellow);color:var(--black);border:0;border-radius:9px;width:34px;height:32px;padding:0;display:grid;place-items:center;flex-shrink:0;line-height:0;transition:background .15s ease}
  .attach{background:transparent;border:1px solid transparent;border-radius:9px;width:30px;height:32px;padding:0;display:grid;place-items:center;color:var(--faint);flex-shrink:0;transition:color .15s ease,border-color .15s ease,background .15s ease}
  .attach svg{width:16px;height:16px}
  .attach:hover:not(:disabled){color:var(--text);border-color:var(--line-strong);background:var(--soft)}
  .send svg{width:16px;height:16px;display:block;transition:transform .18s ease}
  .send[aria-busy="true"] svg{animation:lift 1.1s ease-in-out infinite}
  .send:hover:not(:disabled){background:#ffdd7f}
  .send:hover:not(:disabled) svg{transform:translateY(-1.5px)}
  .send:active:not(:disabled) svg{transform:translateY(1px) scale(.9)}
  .stop{background:transparent;border:1px solid var(--line-strong);color:var(--text);border-radius:9px;padding:7px 12px;font-size:11px;line-height:1.3}
  .stop:not(:disabled){animation:stop-pulse 2s ease-in-out infinite}
  .stop.stopping{border-color:var(--yellow-deep)}
  .stop:hover:not(:disabled){border-color:var(--yellow-deep);color:var(--yellow)}
  .conversation-foot{margin-top:9px;font-size:9.5px;color:var(--faint);text-align:center}
  @keyframes pulse{0%,100%{opacity:1}50%{opacity:.35}}
  @keyframes bob{0%,100%{transform:translateY(0);opacity:.45}50%{transform:translateY(-3px);opacity:1}}
  @keyframes blink{0%,100%{opacity:1}50%{opacity:0}}
  @keyframes lift{0%,100%{opacity:.35;transform:translateY(1px)}50%{opacity:1;transform:translateY(-1.5px)}}
  @keyframes msg-in{from{opacity:0;transform:translateY(7px)}}
  @keyframes avatar-think{0%,100%{transform:rotate(-4deg) translateY(.5px)}50%{transform:rotate(4deg) translateY(-1.5px)}}
  @keyframes mascot-bob{0%,100%{transform:translateY(1px) rotate(-1.5deg)}50%{transform:translateY(-3px) rotate(1.5deg)}}
  @keyframes stop-pulse{50%{border-color:var(--yellow-deep);box-shadow:0 0 0 3px rgba(255,212,92,.08)}}
  @media(max-width:980px){
    .chat-layout{--rail-w:216px}
    .conversation{padding-left:12px}
    .rail-inner{padding:0 16px 8px 0}
  }
  @media(max-width:560px){.chat-workspace{flex-direction:column}.chat-workspace :global(.companion){order:-1}}
  @media(max-width:1120px){
    .composer-hint{display:none}
  }
  @media(max-width:700px){
    .suggestions{grid-template-columns:1fr;gap:6px}
    .picker-trigger{max-width:190px}
    .thinking-name{display:none}
    .message.user .message-body{max-width:88%}
    .welcome{padding:18px 4px}
  }
  @media(prefers-reduced-motion:reduce){
    .typing i,.cursor,.send[aria-busy="true"] svg,.loading-dot{animation:none}
    .picker-chevron,.rail-inner,.conversation{transition:none}
  }
  @media(prefers-reduced-motion:reduce){
    .avatar.busy,.welcome img,.stop,.browser-dot,.message{animation:none}
    .send svg{transition:none}
  }
</style>
