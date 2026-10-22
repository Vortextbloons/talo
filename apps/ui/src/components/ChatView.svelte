<script lang="ts">
  import LibrarySidebar from './LibrarySidebar.svelte';
  import { library, mutateLibrary } from '../lib/library.svelte';
  import { tick } from 'svelte';
  import mascot from '../../../../packages/design/mascot.svg';
  import type { ChatMessage } from '../../../../packages/contracts/src/chat';
  import type { ThinkingLevelName } from '../../../../packages/contracts/src/index';
  import MessageBody from './MessageBody.svelte';
  import Reasoning from './Reasoning.svelte';
  import WebResearch from './WebResearch.svelte';
  import {
    chat,
    chooseModel,
    createConversation,
    dismissError,
    dismissNotice,
    modelsFor,
    openConversation,
    parseSelectionKey,
    isSelectionAvailable,
    retryGeneration,
    retryChat,
    selectionKey,
    selectionLabel,
    sendMessage,
    stopGeneration,
    setThinkingLevel,
  } from '../lib/chat.svelte';

  let { onOpenSettings, name, tagline, showReasoning }: { onOpenSettings: () => void; name: string; tagline: string; showReasoning?: boolean } = $props();

  const THREADS_KEY = 'talo.threads.open';
  function initialThreadsOpen(): boolean {
    try { return localStorage.getItem(THREADS_KEY) !== '0'; } catch { return true; }
  }
  let showArchived = $state(false);
  let confirmingDelete = $state<string | null>(null);
  let text = $state('');
  /* Images waiting in the composer. `data` is base64 for the host, `url` is the preview. */
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
  let threadsOpen = $state(initialThreadsOpen());
  function toggleThreads(): void {
    threadsOpen = !threadsOpen;
    try { localStorage.setItem(THREADS_KEY, threadsOpen ? '1' : '0'); } catch { /* storage can be unavailable */ }
  }
  let logEl = $state<HTMLDivElement>();
  let composerEl = $state<HTMLTextAreaElement>();
  let pinned = true;
  let lastConversationId: string | null = null;

  interface PickerOption { key: string; group: string; label: string; reasoning: boolean; disabled: boolean; selected: boolean }
  interface PickerGroup { id: string; name: string; options: PickerOption[] }

  let pickerOpen = $state(false);
  let pickerActive = $state('');
  let pickerEl = $state<HTMLDivElement>();
  let menuEl = $state<HTMLDivElement>();
  let triggerEl = $state<HTMLButtonElement>();
  let thinkingOpen = $state(false);
  let thinkingActive = $state('');
  let thinkingWrapEl = $state<HTMLDivElement>();
  let thinkingMenuEl = $state<HTMLDivElement>();
  let thinkingTriggerEl = $state<HTMLButtonElement>();

  const active = $derived(chat.active);
  const messages = $derived(chat.active?.messages ?? []);
  const working = $derived(chat.active?.state === 'generating' || chat.active?.state === 'cancelling');
  const cancelling = $derived(chat.active?.state === 'cancelling');
  const generating = $derived(chat.active?.state === 'generating');
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
      const options: PickerOption[] = group.models.map(model => ({
        key: `${group.connection.id}::${model.id}`,
        group: group.connection.name,
        label: model.name,
        reasoning: model.reasoning,
        disabled: working,
        selected: selectedKey === `${group.connection.id}::${model.id}`,
      }));
      if (!options.length) {
        const note = chat.discovery[group.connection.id]?.loading ? 'Discovering models…' : 'No models found — refresh in Settings';
        options.push({ key: `${group.connection.id}::`, group: group.connection.name, label: note, reasoning: false, disabled: true, selected: false });
      }
      groups.push({ id: group.connection.id, name: group.connection.name, options });
    }
    return groups;
  });
  const pickerOptions = $derived(pickerGroups.flatMap(group => group.options));
  /* Pi reports image input per model; without it the worker hands over the file path. */
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
      return { key: selectedKey, group: '', label: selectionLabel(selection), reasoning: false, disabled: true, selected: true };
    }
    return null;
  });
  const pickerValue = $derived(pickerSelected ? pickerSelected.label : 'Choose a model');
  const selectedModel = $derived.by(() => {
    if (!selection) return null;
    const group = modelGroups.find(item => item.connection.id === selection!.connectionId);
    return group?.models.find(model => model.id === selection!.modelId) ?? null;
  });
  // Thinking levels come from Pi's model registry; only reasoning-capable models list them.
  const thinkingOptions = $derived(selectedModel?.thinkingLevels ?? []);
  const thinkingValue = $derived.by(() => {
    if (chat.thinkingLevel && thinkingOptions.includes(chat.thinkingLevel)) return chat.thinkingLevel;
    return thinkingOptions.includes('medium') ? 'medium' : thinkingOptions[0] ?? '';
  });
  const workerLabel = $derived(
    chat.preview ? 'Interface preview'
      : chat.worker.state === 'ready' ? 'Pi ready'
        : chat.worker.state === 'starting' ? 'Waking Pi'
          : chat.worker.state === 'failed' ? 'Pi needs attention'
            : 'Pi resting',
  );
  const composerHint = $derived(
    !connected.length ? 'Connect a provider to begin'
      : !selection ? 'Choose a model below'
        : !selectionAvailable ? 'Choose a connected model'
          : working ? 'Talo is replying'
            : '',
  );
  const placeholder = $derived(
    !connected.length ? 'Connect a provider to start'
      : !selection ? 'Choose a model below'
        : !selectionAvailable ? 'Choose a connected model below'
          : 'What is on your mind?',
  );

  $effect(() => {
    const conversationId = chat.active?.conversation.id ?? null;
    const last = messages[messages.length - 1];
    // Track the pieces that should bring a reader back to the newest text.
    const signal = `${conversationId ?? ''}:${messages.length}:${last?.text.length ?? 0}:${last?.reasoning?.length ?? 0}:${last?.status ?? ''}:${chat.active?.state ?? ''}`;
    if (conversationId !== lastConversationId) {
      lastConversationId = conversationId;
      pinned = true;
    }
    if (signal && pinned) {
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

  async function submit(): Promise<void> {
    const images = draftImages.map(image => ({ name: image.name, mimeType: image.mimeType, data: image.data }));
    if (working || chat.sending || (!text.trim() && !images.length)) return;
    const value = text;
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

  /**
   * Turns clipboard or dropped files into composer thumbnails. The bytes stay in memory until
   * send; the host writes them into Pi's workspace so the model can also open the file.
   */
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

  /* A page that copies its own rendering pastes HTML with a data: image instead of a file. */
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

  /* Custom model dropdown: the native select popup is drawn by the OS and stays light. */
  function openPicker(): void {
    const enabled = pickerOptions.filter(option => !option.disabled);
    const selectedUsable = pickerSelected && !pickerSelected.disabled && pickerOptions.some(option => option.key === pickerSelected.key);
    pickerActive = selectedUsable ? pickerSelected!.key : enabled[0]?.key ?? '';
    pickerOpen = true;
    // Roving focus: the active option takes the keyboard, arrows move it.
    tick().then(() => {
      menuEl?.querySelector<HTMLElement>(`[data-key="${CSS.escape(pickerActive)}"]`)?.focus();
    });
  }

  function closePicker(refocus = false): void {
    pickerOpen = false;
    if (refocus) triggerEl?.focus();
  }

  function movePicker(delta: number, jump?: number): void {
    const enabled = pickerOptions.filter(option => !option.disabled);
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
      const option = pickerOptions.find(item => item.key === pickerActive);
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

  /* Deleting is confirmed inline in the rail; the original history file stays on disk. */
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

  /* Custom thinking dropdown: the native select popup is OS-drawn and stays light. */
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
    if (!logEl) return;
    pinned = logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight < 90;
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
        : 'Talo';
  }

  function canRetry(index: number, message: ChatMessage): boolean {
    return message.role === 'assistant'
      && index === lastAssistantIndex
      && (message.status === 'failed' || message.status === 'cancelled')
      && !working;
  }

  function statusClass(): string {
    return chat.worker.state === 'ready' ? 'ready'
      : chat.worker.state === 'failed' ? 'failed'
        : chat.worker.state === 'starting' ? 'starting' : 'resting';
  }
</script>

<svelte:window onpointerdown={onWindowPointerDown} onkeydown={(event) => { if (event.key === 'Escape' && (confirmingDelete || viewingImage)) { confirmingDelete = null; viewingImage = null; } }} />

<div class="chat-layout" class:rail-collapsed={!threadsOpen}>
  <aside class="threads" aria-label="Conversations" inert={!threadsOpen} aria-hidden={!threadsOpen}>
    <div class="rail-inner">
      <div class="rail-greeting">
        <strong>{greeting}</strong>
        <span>{tagline}</span>
      </div>
      <LibrarySidebar bind:showArchived />
      <div class="threads-head">
        <span>Threads</span>
        <div class="threads-actions">
          <button
            class="new-thread"
            disabled={working || chat.creating || chat.preview}
            title={working ? 'Stop the current reply before starting another conversation' : 'Start a new conversation'}
            onclick={() => { void createConversation(); }}
          >New</button>
          <button class="rail-toggle" title="Hide threads" aria-label="Hide thread list" aria-expanded={threadsOpen} onclick={toggleThreads}><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><rect x="3.5" y="4.5" width="17" height="15" rx="3.5"/><path d="M9.5 4.5v15"/></svg></button>
        </div>
      </div>
      {#if chat.conversations.length}
        <ul class="thread-list">
          {#each chat.conversations.filter(c => (c.workspaceId ?? 'general') === library.activeWorkspace && Boolean(c.archived) === showArchived) as conversation (conversation.id)}
            <li class:active={chat.active?.conversation.id === conversation.id}>
              <button
                class="thread-open"
                disabled={working || chat.creating}
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
                  disabled={working || library.busy}
                  onclick={() => { confirmingDelete = conversation.id; }}
                ><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><path d="M4 7h16" /><path d="M9.5 4.5h5" /><path d="M6.5 7l.9 12a1.8 1.8 0 0 0 1.8 1.6h5.6a1.8 1.8 0 0 0 1.8-1.6l.9-12" /><path d="M10.5 11v6" /><path d="M13.5 11v6" /></svg></button>
              {/if}
            </li>
          {/each}
        </ul>
      {:else}
        <p class="threads-empty">Your first message starts a thread here.</p>
      {/if}
      {#if working}<p class="threads-note">Stop the reply before switching threads.</p>{/if}
    </div>
  </aside>

  <section class="conversation" aria-label="Conversation">
    <div class="conversation-head">
      <div class="conversation-lead">
        {#if !threadsOpen}
          <button class="threads-reopen" title="Show threads" aria-label="Show thread list" aria-expanded={threadsOpen} onclick={toggleThreads}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><rect x="3.5" y="4.5" width="17" height="15" rx="3.5"/><path d="M9.5 4.5v15"/></svg>
            <span>Threads</span>
          </button>
        {/if}
        <div class="conversation-title">
          <strong>{active?.conversation.title || 'A new conversation'}</strong>
          <span>{selection ? selectionLabel(selection) : 'No model chosen yet'}</span>
        </div>
      </div>
      <span class="worker {statusClass()}" title={chat.worker.error || 'Local Pi runtime'}>
        <span class="worker-dot" aria-hidden="true"></span>{workerLabel}
      </span>
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
        {#each messages as message, index (message.id)}
          <article class="message {message.role}" class:streaming={message.status === 'generating'}>
            {#if message.role === 'assistant'}
              <img class="avatar" src={mascot} alt="" aria-hidden="true" />
            {/if}
            <div class="message-body">
              <div class="message-role">
                <span>{message.role === 'user' ? 'You' : modelName(message)}</span>
                {#if message.status === 'cancelled'}<span class="status-tag">Stopped</span>{/if}
                {#if message.status === 'failed'}<span class="status-tag failed">Failed</span>{/if}
              </div>
              {#if message.generationId && chat.research[message.generationId]?.length}
                <WebResearch items={chat.research[message.generationId]} working={message.status === 'generating'} onCancel={() => void stopGeneration()} />
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
        {/each}
      </div>
    {/if}

    {#if chat.phase === 'ready'}
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
              <div class="picker-menu" id="chat-model-menu" role="listbox" tabindex="-1" aria-labelledby="chat-model-label" bind:this={menuEl}>
                {#each pickerOptions as option, index (option.key)}
                  {#if index === 0 || pickerOptions[index - 1].group !== option.group}
                    <div class="picker-group">{option.group}</div>
                  {/if}
                  <div
                    role="option"
                    id={`chat-model-opt-${index}`}
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
              </div>
            {/if}
          </div>
          {#if chat.selectingModel}<span class="picker-status" role="status">Saving…</span>{/if}
        </div>
        <div class="send-area">
          {#if composerHint}<span class="composer-hint">{composerHint}</span>{/if}
          {#if thinkingOptions.length}
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
              disabled={cancelling || chat.sending}
              onclick={() => { void stopGeneration(); }}
              aria-label={cancelling ? 'Stopping reply' : 'Stop reply'}
            >{cancelling ? 'Stopping…' : 'Stop'}</button>
          {:else}
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
              aria-label={chat.sending ? 'Sending message' : 'Send message'}
              aria-busy={chat.sending}
            ><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><path d="M12 19.5V5.5" /><path d="m5.5 11.5 6.5-6.5 6.5 6.5" /></svg></button>
          {/if}
        </div>
      </div>
    </form>
    {/if}
  </section>
</div>

{#if viewingImage}
  <div class="lightbox" role="dialog" aria-modal="true" aria-label="Image preview">
    <button type="button" class="lightbox-backdrop" aria-label="Close preview" onclick={() => { viewingImage = null; }}></button>
    <img src={viewingImage} alt={viewingName ?? 'Attachment'} />
    <button type="button" class="lightbox-close" aria-label="Close preview" onclick={() => { viewingImage = null; }}>×</button>
  </div>
{/if}

<style>
  /* One full-height surface: the rail holds threads, the column holds the conversation. */
  .chat-layout{--rail-w:clamp(210px,17vw,246px);flex:1;min-height:0;display:grid;grid-template-columns:var(--rail-w) minmax(0,1fr);transition:grid-template-columns .24s cubic-bezier(.2,.7,.3,1)}
  .chat-layout>*{min-width:0}
  .chat-layout.rail-collapsed{grid-template-columns:0 minmax(0,1fr)}
  .threads{border-right:1px solid var(--line);min-height:0;overflow:hidden;transition:border-color .2s ease}
  .rail-collapsed .threads{border-right-color:transparent}
  .rail-inner{width:var(--rail-w);height:100%;padding:2px 20px 8px 0;overflow-y:auto;display:flex;flex-direction:column;scrollbar-gutter:stable;transition:opacity .16s ease}
  .rail-collapsed .rail-inner{opacity:0;visibility:hidden}
  .rail-greeting{padding:6px 2px 20px}
  .rail-greeting strong{display:block;font-size:21px;letter-spacing:-.8px;font-weight:560}
  .rail-greeting span{display:block;margin-top:6px;font-size:12px;color:var(--muted);line-height:1.55}
  .rail-toggle,.threads-reopen{display:inline-flex;align-items:center;justify-content:center;gap:7px;flex-shrink:0;background:transparent;border:1px solid transparent;border-radius:7px;color:var(--faint);padding:5px;line-height:1;transition:color .15s ease,border-color .15s ease,background .15s ease}
  .rail-toggle svg{width:15px;height:15px}
  .rail-toggle:hover,.threads-reopen:hover{color:var(--text);border-color:var(--line-strong);background:var(--soft)}
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
  /* Delete stays out of the way until a thread is hovered, focused, or current. */
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
  .conversation{width:100%;max-width:1180px;margin:0 auto;min-height:0;display:flex;flex-direction:column;padding-left:18px;transition:padding-left .24s cubic-bezier(.2,.7,.3,1)}
  .rail-collapsed .conversation{padding-left:0}
  .conversation-head{display:flex;align-items:flex-start;justify-content:space-between;gap:14px;padding-bottom:13px;border-bottom:1px solid var(--line);margin-bottom:14px}
  .conversation-lead{display:flex;align-items:flex-start;gap:12px;min-width:0}
  .threads-reopen{align-self:center;padding:6px 10px;font-size:11px;color:var(--muted);gap:8px}
  .threads-reopen svg{width:14px;height:14px;opacity:.75}
  .conversation-title{min-width:0}
  .conversation-title strong{display:block;font-size:15px;font-weight:550;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
  .conversation-title span{display:block;font-size:11px;color:var(--muted);margin-top:4px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
  .worker{display:inline-flex;align-items:center;gap:7px;font-size:10.5px;color:var(--muted);white-space:nowrap;border:1px solid var(--line);background:var(--soft);border-radius:20px;padding:5px 11px}
  .worker-dot{width:6px;height:6px;border-radius:50%;background:var(--faint)}
  .worker.ready .worker-dot{background:var(--yellow-deep)}
  .worker.starting .worker-dot{background:var(--yellow);animation:pulse 1.4s ease-in-out infinite}
  .worker.failed .worker-dot{background:#d0844c}
  .chat-message{display:flex;gap:12px;align-items:center;justify-content:space-between;padding:10px 13px;border-radius:9px;font-size:12.5px;margin-bottom:12px;line-height:1.5}
  .chat-message.error{background:var(--error-bg);border:1px solid var(--error-line);color:var(--error-text)}
  .chat-message.notice{background:#211f16;border:1px solid #4e4324;color:#e8d59e}
  .chat-message button{background:transparent;border:0;color:inherit;padding:0 4px;font-size:16px;line-height:1}
  /* Empty moments center in whatever height is left, with the mascot lit from within. */
  .welcome{flex:1;min-height:0;overflow-y:auto;display:flex;flex-direction:column;align-items:center;justify-content:center;text-align:center;padding:24px 12px;width:100%;max-width:640px;align-self:center}
  .welcome img{width:84px;margin-bottom:22px;filter:drop-shadow(0 0 36px rgba(255,212,92,.22)) drop-shadow(0 0 90px rgba(255,212,92,.1))}
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
  .log{flex:1;min-height:0;overflow-y:auto;padding:8px 10px 20px 2px;scrollbar-gutter:stable}
  .message{display:flex;gap:12px;padding:14px 0 20px;border-bottom:1px solid var(--line)}
  .message:last-child{border-bottom:0}
  .message.user{justify-content:flex-end}
  .avatar{width:27px;height:27px;margin-top:2px;flex-shrink:0}
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
  .composer{flex-shrink:0;border:1px solid var(--line-strong);border-radius:15px;padding:15px 16px 13px;background:var(--paper);box-shadow:0 12px 32px var(--shadow);margin-top:10px;transition:border-color .15s ease,background .15s ease}
  /* A drop anywhere on the composer takes the file instead of the browser opening it. */
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
  .composer-foot{display:flex;align-items:center;justify-content:space-between;gap:14px;border-top:1px solid var(--line);padding-top:10px}
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
  /* Opens upward from the composer; fully themed, unlike the OS-drawn native popup. */
  .picker-menu{position:absolute;left:0;bottom:calc(100% + 8px);z-index:40;min-width:290px;max-width:min(430px,90vw);max-height:min(370px,52vh);overflow-y:auto;background:var(--paper);border:1px solid var(--line-strong);border-radius:12px;padding:6px;box-shadow:0 18px 48px rgba(0,0,0,.55);scrollbar-gutter:stable}
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
  /* Thinking level pill + custom menu: the native select popup is OS-drawn and stays light. */
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
  @media(prefers-reduced-motion:reduce){.thinking-chevron{transition:none}}
  .send-area{display:flex;align-items:center;gap:11px;flex-shrink:0}
  .composer-hint{font-size:10px;color:var(--faint);white-space:nowrap}
  .send{background:var(--yellow);color:var(--black);border:0;border-radius:9px;width:34px;height:32px;padding:0;display:grid;place-items:center;flex-shrink:0;line-height:0;transition:background .15s ease}
  .attach{background:transparent;border:1px solid transparent;border-radius:9px;width:30px;height:32px;padding:0;display:grid;place-items:center;color:var(--faint);flex-shrink:0;transition:color .15s ease,border-color .15s ease,background .15s ease}
  .attach svg{width:16px;height:16px}
  .attach:hover:not(:disabled){color:var(--text);border-color:var(--line-strong);background:var(--soft)}
  .send svg{width:16px;height:16px;display:block}
  .send[aria-busy="true"] svg{animation:lift 1.1s ease-in-out infinite}
  .send:hover:not(:disabled){background:#ffdd7f}
  .stop{background:transparent;border:1px solid var(--line-strong);color:var(--text);border-radius:9px;padding:7px 12px;font-size:11px;line-height:1.3}
  .stop:hover:not(:disabled){border-color:var(--yellow-deep);color:var(--yellow)}
  .conversation-foot{margin-top:9px;font-size:9.5px;color:var(--faint);text-align:center}
  @keyframes pulse{0%,100%{opacity:1}50%{opacity:.35}}
  @keyframes bob{0%,100%{transform:translateY(0);opacity:.45}50%{transform:translateY(-3px);opacity:1}}
  @keyframes blink{0%,100%{opacity:1}50%{opacity:0}}
  @keyframes lift{0%,100%{opacity:.35;transform:translateY(1px)}50%{opacity:1;transform:translateY(-1.5px)}}
  @media(max-width:980px){
    .chat-layout{grid-template-columns:1fr;grid-template-rows:auto minmax(0,1fr)}
    .chat-layout.rail-collapsed{grid-template-columns:1fr;grid-template-rows:0 minmax(0,1fr)}
    .threads{border-right:0;border-bottom:1px solid var(--line)}
    .rail-collapsed .threads{border-bottom-color:transparent}
    .rail-inner{width:auto;padding:0 0 14px;overflow:visible}
    .rail-greeting{padding:0 2px 12px}
    .thread-list{display:flex;overflow-x:auto;gap:5px;padding-bottom:2px}
    .thread-list li{flex:0 0 auto;max-width:190px}
    .thread-open{min-width:150px;border-color:var(--line)}
    .thread-list li.active .thread-open::before{display:none}
    .threads-empty{margin:4px 2px}
    .threads-note{margin-top:8px;border-top:0;padding-top:2px}
    .conversation{max-width:none;padding-left:0}
  }
  @media(max-width:700px){
    .suggestions{grid-template-columns:1fr;gap:6px}
    .composer-hint{display:none}
    .picker-trigger{max-width:190px}
    .thinking-name{display:none}
    .message.user .message-body{max-width:88%}
    .welcome{padding:18px 4px}
  }
  @media(prefers-reduced-motion:reduce){
    .worker.starting .worker-dot,.typing i,.cursor,.send[aria-busy="true"] svg,.loading-dot{animation:none}
    .picker-chevron,.chat-layout,.threads,.rail-inner,.conversation{transition:none}
  }
</style>
