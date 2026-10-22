<script lang="ts">
  import { tick } from 'svelte';
  import mascot from '../../../../packages/design/mascot.svg';
  import type { ChatMessage } from '../../../../packages/contracts/src/chat';
  import type { ProviderModelInfo } from '../../../../packages/contracts/src/index';
  import MessageBody from './MessageBody.svelte';
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
  } from '../lib/chat.svelte';

  let { onOpenSettings, name, tagline }: { onOpenSettings: () => void; name: string; tagline: string } = $props();

  let text = $state('');
  let logEl = $state<HTMLDivElement>();
  let composerEl = $state<HTMLTextAreaElement>();
  let pinned = true;
  let lastConversationId: string | null = null;

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
            : 'Enter to send · Shift + Enter for a new line',
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
    const signal = `${conversationId ?? ''}:${messages.length}:${last?.text.length ?? 0}:${last?.status ?? ''}:${chat.active?.state ?? ''}`;
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
    if (working || chat.sending || !text.trim()) return;
    const value = text;
    const accepted = await sendMessage(value);
    if (accepted) {
      text = '';
      pinned = true;
      await tick();
      resize();
      if (logEl) logEl.scrollTop = logEl.scrollHeight;
    }
  }

  function onComposerKeydown(event: KeyboardEvent): void {
    if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
      event.preventDefault();
      void submit();
    }
  }

  async function changeModel(event: Event): Promise<void> {
    const next = parseSelectionKey((event.currentTarget as HTMLSelectElement).value);
    if (next) await chooseModel(next);
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

  function modelLabel(model: ProviderModelInfo): string {
    return model.reasoning ? `${model.name} · reasoning` : model.name;
  }
</script>

<div class="chat-layout">
  <aside class="threads" aria-label="Conversations">
    <div class="rail-greeting">
      <strong>{greeting}</strong>
      <span>{tagline}</span>
    </div>
    <div class="threads-head">
      <span>Threads</span>
      <button
        class="new-thread"
        disabled={working || chat.creating || chat.preview}
        title={working ? 'Stop the current reply before starting another conversation' : 'Start a new conversation'}
        onclick={() => { void createConversation(); }}
      >New</button>
    </div>
    {#if chat.conversations.length}
      <ul class="thread-list">
        {#each chat.conversations as conversation (conversation.id)}
          <li>
            <button
              class:active={chat.active?.conversation.id === conversation.id}
              disabled={working || chat.creating}
              aria-current={chat.active?.conversation.id === conversation.id ? 'true' : undefined}
              title={conversation.title || 'New conversation'}
              onclick={() => { void openConversation(conversation.id); }}
            >
              <span class="thread-title">{conversation.title || 'New conversation'}</span>
              <span class="thread-meta">
                <span>{relativeDate(conversation.updatedAt)}</span>
                {#if conversation.selection}<span class="thread-model">{selectionLabel(conversation.selection)}</span>{/if}
              </span>
              {#if chat.openingId === conversation.id}<span class="thread-loading">Opening</span>{/if}
            </button>
          </li>
        {/each}
      </ul>
    {:else}
      <p class="threads-empty">Your first message starts a thread here.</p>
    {/if}
    {#if working}<p class="threads-note">Stop the reply before switching threads.</p>{/if}
  </aside>

  <section class="conversation" aria-label="Conversation">
    <div class="conversation-head">
      <div class="conversation-title">
        <strong>{active?.conversation.title || 'A new conversation'}</strong>
        <span>{selection ? selectionLabel(selection) : 'No model chosen yet'}</span>
      </div>
      <span class="worker {statusClass()}" title={chat.worker.error || 'Local Pi runtime'}>
        <span class="worker-dot" aria-hidden="true"></span>{workerLabel}
      </span>
    </div>

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
    <form class="composer" onsubmit={(event) => { event.preventDefault(); void submit(); }}>
      {#if active?.state === 'failed' && !chat.sending}
        <div class="composer-note">That reply did not finish. <button type="button" disabled={chat.preview} onclick={() => { void retryGeneration(); }}>Try again</button></div>
      {/if}
      <textarea
        bind:this={composerEl}
        bind:value={text}
        aria-label="Message to Talo"
        placeholder={placeholder}
        rows="1"
        disabled={!connected.length}
        oninput={resize}
        onkeydown={onComposerKeydown}
      ></textarea>
      <div class="composer-foot">
        <div class="model-picker">
          <label for="chat-model">Model</label>
          <select
            id="chat-model"
            value={selectedKey}
            disabled={working || chat.selectingModel || !connected.length}
            onchange={changeModel}
          >
            {#if !selection}<option value="">Choose a model</option>{/if}
            {#if selection && !selectionInGroups}
              <option value={selectedKey} disabled>{selectionLabel(selection)} (unavailable)</option>
            {/if}
            {#each modelGroups as group (group.connection.id)}
              {#if group.models.length}
                <optgroup label={group.connection.name}>
                  {#each group.models as model (model.id)}
                    <option value={`${group.connection.id}::${model.id}`} disabled={working}>{modelLabel(model)}</option>
                  {/each}
                </optgroup>
              {:else if chat.discovery[group.connection.id]?.loading}
                <optgroup label={group.connection.name}><option value={`${group.connection.id}::`} disabled>Discovering models…</option></optgroup>
              {:else}
                <optgroup label={group.connection.name}><option value={`${group.connection.id}::`} disabled>No models found — refresh in Settings</option></optgroup>
              {/if}
            {/each}
          </select>
          {#if chat.selectingModel}<span class="picker-status" role="status">Saving…</span>{/if}
        </div>
        <div class="send-area">
          <span class="composer-hint">{composerHint}</span>
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
              type="submit"
              class="send"
              disabled={!connected.length || !selection || !selectionAvailable || !text.trim() || chat.sending || chat.preview}
              aria-label={chat.sending ? 'Sending message' : 'Send message'}
              aria-busy={chat.sending}
            ><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><path d="M12 19.5V5.5" /><path d="m5.5 11.5 6.5-6.5 6.5 6.5" /></svg></button>
          {/if}
        </div>
      </div>
    </form>
    {#if !generating && !cancelling && messages.length > 0 && active?.state !== 'failed'}
      <p class="conversation-foot">Talo can make mistakes. Check anything important.</p>
    {/if}
    {/if}
  </section>
</div>

<style>
  /* One full-height surface: the rail holds threads, the column holds the conversation. */
  .chat-layout{flex:1;min-height:0;display:grid;grid-template-columns:clamp(210px,17vw,246px) minmax(0,1fr)}
  .chat-layout>*{min-width:0}
  .threads{border-right:1px solid var(--line);padding:2px 20px 8px 0;min-height:0;overflow-y:auto;display:flex;flex-direction:column;scrollbar-gutter:stable}
  .rail-greeting{padding:6px 2px 20px}
  .rail-greeting strong{display:block;font-size:21px;letter-spacing:-.8px;font-weight:560}
  .rail-greeting span{display:block;margin-top:6px;font-size:12px;color:var(--muted);line-height:1.55}
  .threads-head{display:flex;align-items:center;justify-content:space-between;margin-bottom:12px}
  .threads-head>span{font-size:10.5px;color:var(--faint);letter-spacing:.1em;text-transform:uppercase}
  .new-thread{background:transparent;border:1px solid var(--line);color:var(--text);padding:5px 10px;font-size:11px;border-radius:7px}
  .new-thread:hover:not(:disabled){border-color:var(--yellow-deep);color:var(--yellow)}
  .thread-list{list-style:none;padding:0;margin:0;display:grid;gap:3px;min-width:0}
  .thread-list button{position:relative;width:100%;text-align:left;background:transparent;border:1px solid transparent;color:var(--muted);padding:10px 10px;border-radius:9px;display:block;line-height:1.4}
  .thread-list button:hover:not(:disabled){background:#1d1d19;color:var(--text)}
  .thread-list button.active{background:#26261f;color:var(--text);border-color:#34342c}
  .thread-list button.active::before{content:'';position:absolute;left:-1px;top:12px;bottom:12px;width:2px;border-radius:2px;background:var(--yellow)}
  .thread-title{display:block;font-size:12.5px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
  .thread-meta{display:flex;gap:7px;margin-top:4px;font-size:10px;color:var(--faint);white-space:nowrap;overflow:hidden}
  .thread-model{overflow:hidden;text-overflow:ellipsis;max-width:115px}
  .thread-loading{display:block;margin-top:4px;font-size:10px;color:var(--yellow-deep)}
  .threads-empty{font-size:11.5px;line-height:1.6;color:var(--faint);margin:10px 2px}
  .threads-note{font-size:10.5px;line-height:1.5;color:var(--faint);margin:14px 2px 0;border-top:1px solid var(--line);padding-top:12px}
  .conversation{width:100%;max-width:940px;margin:0 auto;min-height:0;display:flex;flex-direction:column;padding-left:26px}
  .conversation-head{display:flex;align-items:flex-start;justify-content:space-between;gap:14px;padding-bottom:13px;border-bottom:1px solid var(--line);margin-bottom:14px}
  .conversation-title{min-width:0}
  .conversation-title strong{display:block;font-size:14px;font-weight:550;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
  .conversation-title span{display:block;font-size:10.5px;color:var(--muted);margin-top:4px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
  .worker{display:inline-flex;align-items:center;gap:7px;font-size:10.5px;color:var(--muted);white-space:nowrap;border:1px solid var(--line);background:var(--soft);border-radius:20px;padding:5px 11px}
  .worker-dot{width:6px;height:6px;border-radius:50%;background:var(--faint)}
  .worker.ready .worker-dot{background:var(--yellow-deep)}
  .worker.starting .worker-dot{background:var(--yellow);animation:pulse 1.4s ease-in-out infinite}
  .worker.failed .worker-dot{background:#d0844c}
  .chat-message{display:flex;gap:12px;align-items:center;justify-content:space-between;padding:10px 13px;border-radius:9px;font-size:12px;margin-bottom:12px;line-height:1.5}
  .chat-message.error{background:var(--error-bg);border:1px solid var(--error-line);color:var(--error-text)}
  .chat-message.notice{background:#211f16;border:1px solid #4e4324;color:#e8d59e}
  .chat-message button{background:transparent;border:0;color:inherit;padding:0 4px;font-size:16px;line-height:1}
  /* Empty moments center in whatever height is left, with the mascot lit from within. */
  .welcome{flex:1;min-height:0;overflow-y:auto;display:flex;flex-direction:column;align-items:center;justify-content:center;text-align:center;padding:24px 12px;width:100%;max-width:600px;align-self:center}
  .welcome img{width:84px;margin-bottom:22px;filter:drop-shadow(0 0 36px rgba(255,212,92,.22)) drop-shadow(0 0 90px rgba(255,212,92,.1))}
  .welcome h2{font-size:23px;letter-spacing:-.6px;font-weight:550;margin:0 0 12px}
  .welcome p{color:var(--muted);font-size:13.5px;line-height:1.7;margin:0 0 21px}
  .welcome small{display:block;margin-top:18px;color:var(--faint);font-size:11px;line-height:1.6}
  .secondary{background:transparent;margin-left:8px}
  .loading-dot{display:block;width:7px;height:7px;border-radius:50%;background:var(--yellow);margin:0 auto 18px;animation:pulse 1.4s ease-in-out infinite}
  .suggestions{display:grid;grid-template-columns:repeat(3,1fr);gap:16px;margin-top:26px;text-align:left;width:100%}
  .suggestions button{background:transparent;border:0;border-top:1px solid var(--line);padding:14px 2px 0;color:var(--text);border-radius:0;display:block}
  .suggestions button:hover:not(:disabled){background:transparent;border-top-color:var(--yellow-deep)}
  .suggestions button:hover:not(:disabled) span{color:var(--yellow)}
  .suggestions span{display:block;font-size:12.5px;font-weight:600;transition:color .15s ease}
  .suggestions small{display:block;margin-top:6px;font-size:11px;color:var(--muted);line-height:1.5}
  .log{flex:1;min-height:0;overflow-y:auto;padding:6px 16px 18px 2px;scrollbar-gutter:stable}
  .message{display:flex;gap:12px;padding:14px 0 20px;border-bottom:1px solid var(--line)}
  .message:last-child{border-bottom:0}
  .message.user{justify-content:flex-end}
  .avatar{width:27px;height:27px;margin-top:2px;flex-shrink:0}
  .message-body{min-width:0;max-width:92%}
  .message.user .message-body{max-width:82%;background:#23231e;border:1px solid #33332a;border-radius:13px;padding:10px 14px}
  .message-role{display:flex;align-items:center;gap:8px;font-size:10.5px;color:var(--muted);margin-bottom:6px;letter-spacing:.01em}
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
  .composer{flex-shrink:0;border:1px solid var(--line-strong);border-radius:15px;padding:14px 15px 12px;background:var(--paper);box-shadow:0 12px 32px var(--shadow);margin-top:10px}
  .composer:focus-within{border-color:#4b4b40}
  .composer textarea{display:block;border:0;resize:none;background:transparent;color:var(--text);width:100%;height:48px;min-height:48px;max-height:190px;font-size:13.5px;line-height:1.6;padding:2px 2px 8px;overflow-y:auto}
  .composer-note{font-size:11px;color:var(--error-text);margin:0 2px 9px;padding-bottom:9px;border-bottom:1px solid var(--line)}
  .composer-note button{background:transparent;border:0;color:var(--yellow);padding:0;margin-left:5px;text-decoration:underline;font-size:11px}
  .composer-foot{display:flex;align-items:center;justify-content:space-between;gap:14px;border-top:1px solid var(--line);padding-top:10px}
  .model-picker{display:flex;align-items:center;gap:8px;min-width:0;position:relative}
  .model-picker label{font-size:10px;color:var(--faint);flex-shrink:0}
  .model-picker select{appearance:none;background:transparent;border:1px solid transparent;border-radius:6px;color:var(--muted);font-size:10.5px;padding:4px 21px 4px 6px;max-width:300px;min-width:0;text-overflow:ellipsis;margin:0;width:auto;background-image:linear-gradient(45deg,transparent 50%,var(--faint) 50%),linear-gradient(135deg,var(--faint) 50%,transparent 50%);background-position:calc(100% - 11px) 50%,calc(100% - 7px) 50%;background-size:4px 4px,4px 4px;background-repeat:no-repeat}
  .model-picker select:hover:not(:disabled){border-color:var(--line);color:var(--text)}
  .picker-status{font-size:10px;color:var(--faint)}
  .send-area{display:flex;align-items:center;gap:11px;flex-shrink:0}
  .composer-hint{font-size:10px;color:var(--faint);white-space:nowrap}
  .send{background:var(--yellow);color:var(--black);border:0;border-radius:9px;width:34px;height:32px;padding:0;display:grid;place-items:center;flex-shrink:0;line-height:0;transition:background .15s ease}
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
    .threads{border-right:0;border-bottom:1px solid var(--line);padding:0 0 14px;overflow:visible}
    .rail-greeting{padding:0 2px 12px}
    .thread-list{display:flex;overflow-x:auto;gap:5px;padding-bottom:2px}
    .thread-list li{flex:0 0 auto;max-width:190px}
    .thread-list button{min-width:150px;border-color:var(--line)}
    .thread-list button.active::before{display:none}
    .threads-empty{margin:4px 2px}
    .threads-note{margin-top:8px;border-top:0;padding-top:2px}
    .conversation{max-width:none;padding-left:0}
  }
  @media(max-width:700px){
    .suggestions{grid-template-columns:1fr;gap:6px}
    .composer-hint{display:none}
    .model-picker select{max-width:190px}
    .message.user .message-body{max-width:88%}
    .welcome{padding:18px 4px}
  }
  @media(prefers-reduced-motion:reduce){
    .worker.starting .worker-dot,.typing i,.cursor,.send[aria-busy="true"] svg,.loading-dot{animation:none}
  }
</style>