<script lang="ts">
  import { onDestroy } from 'svelte';
  import type { ConnectionInfo } from '../../../../packages/contracts/src/chat';
  import {
    chat,
    connectProvider,
    cancelChatGPT,
    dismissError,
    dismissNotice,
    discoverModels,
    disconnectProvider,
    modelsFor,
  } from '../lib/chat.svelte';
  import { PROVIDER_CHOICES, providerChoice, providerName } from '../lib/connection-providers';

  const initialProvider = chat.connecting && chat.connectingProvider
    ? chat.connectingProvider
    : (PROVIDER_CHOICES[0]?.id ?? 'openai');
  let providerId = $state(initialProvider);
  let displayName = $state(chat.connecting && chat.connectingName ? chat.connectingName : providerChoice(initialProvider).name);
  let apiKey = $state('');
  let modelId = $state('');
  let busy = $state(false);
  let elapsed = $state(0);
  let timer: ReturnType<typeof setInterval> | undefined;
  let localError = $state('');
  let savedAccount = $state('');

  onDestroy(() => {
    if (timer) clearInterval(timer);
  });

  const descriptor = $derived(providerChoice(providerId));
  const placeholder = $derived(descriptor.placeholder);
  const chatgpt = $derived(providerId === 'chatgpt');
  const savedAccounts = $derived(chat.connections.filter(connection => connection.providerId === 'chatgpt'));
  const pending = $derived(busy || chat.connecting);
  const connected = $derived(chat.connections.filter(connection => connection.connected));


  $effect(() => {
    if (pending) {
      if (!timer) {
        elapsed = 0;
        timer = setInterval(() => { elapsed += 1; }, 1000);
      }
    } else if (timer) {
      clearInterval(timer);
      timer = undefined;
      elapsed = 0;
    }
  });

  function selectProvider(next: string): void {
    providerId = next;
    const choice = providerChoice(next);
    displayName = choice.name;
    modelId = '';

    apiKey = '';
    localError = '';
    savedAccount = '';
  }

  async function connect(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (pending || chat.preview || placeholder) return;
    if (!chatgpt && !apiKey.trim()) {
      localError = 'Enter an API key for this provider.';
      return;
    }


    let key = apiKey;
    apiKey = '';
    localError = '';
    busy = true;
    try {
      const info = await connectProvider({
        providerId,
        name: displayName.trim() || descriptor.name,
        apiKey: key,
        modelId: modelId.trim() || undefined,
        connectionId: chatgpt ? savedAccount || undefined : undefined,
      });
      if (info) {
        displayName = info.name;
        modelId = '';
      } else {

        localError = chatgpt ? 'ChatGPT sign-in did not finish. Try again.' : 'Connection failed. Check the key and try again.';
      }
    } catch (value) {
      localError = value instanceof Error
        ? `The key check did not finish. ${value.message}`
        : 'The key check did not finish. Try again.';
    } finally {
      key = '';
      busy = false;
    }
  }

  async function refresh(connection: ConnectionInfo): Promise<void> {
    localError = '';
    await discoverModels(connection.id);
  }

  async function disconnect(connectionId: string): Promise<void> {
    localError = '';
    await disconnectProvider(connectionId);
  }

  function discoveredCount(connectionId: string): number {
    return chat.models[connectionId]?.length ?? 0;
  }

  function discoveredLabel(connectionId: string): string {
    const count = discoveredCount(connectionId);
    return count === 1 ? '1 model discovered' : `${count} models discovered`;
  }

  function discoveredNames(connection: ConnectionInfo): string {
    const models = modelsFor(connection);
    if (!models.length) return 'No models discovered yet';
    const names = models.slice(0, 4).map(model => model.name);
    return models.length > names.length ? `${names.join(', ')} and ${models.length - names.length} more` : names.join(', ');
  }
</script>

<section class="card providers">
  <h2>Model providers</h2>

  {#if chat.error}
    <div class="settings-message error" role="alert">
      <span>{chat.error}</span>
      <button aria-label="Dismiss error" onclick={dismissError}>×</button>
    </div>
  {/if}
  {#if chat.notice}
    <div class="settings-message notice" role="status">
      <span>{chat.notice}</span>
      <button aria-label="Dismiss notice" onclick={dismissNotice}>×</button>
    </div>
  {/if}
  {#if connected.length}
    <div class="connected-list">
      <h3>Connected</h3>
      {#each connected as connection (connection.id)}
        <div class="connection">
          <div class="connection-main">
            <div class="connection-title">
              <strong>{connection.name}</strong>
              <span class="tag connected">Connected</span>
            </div>
            <small>{providerName(connection.providerId)} · {discoveredLabel(connection.id)}</small>
            <small class="model-summary">{discoveredNames(connection)}</small>
            {#if chat.discovery[connection.id]?.error}
              <small class="discovery-error">{chat.discovery[connection.id].error}</small>
            {/if}
          </div>
          <div class="connection-actions">
            <button
              disabled={pending || chat.discovery[connection.id]?.loading}
              onclick={() => { void refresh(connection); }}
            >{chat.discovery[connection.id]?.loading ? 'Refreshing…' : 'Refresh models'}</button>
            <button class="quiet-danger" disabled={pending} onclick={() => { void disconnect(connection.id); }}>Disconnect</button>
          </div>
        </div>
      {/each}
    </div>
  {:else}
    <p class="empty-line">No providers connected.</p>
  {/if}


  <details class="connect-details" open={connected.length === 0}>
    <summary>{connected.length ? 'Add another provider' : 'Add a provider'}</summary>
  <form class="connect-form" onsubmit={connect} aria-busy={pending}>
    {#if localError}
      <div class="settings-message error form-error" role="alert">{localError}</div>
    {/if}
    {#if pending}
      <div class="pending" role="status" aria-live="polite">
        <span class="spinner" aria-hidden="true"></span>
        <div>
          <strong>{chatgpt ? 'Finish signing in with ChatGPT in your browser…' : `Checking your key with ${descriptor.name}…`}</strong>
          <small>
            {#if chatgpt}
              Choose your account to continue.
            {:else if elapsed >= 20}
              Taking longer than usual. Keep this window open.
            {:else}
              Verifying your key.
            {/if}
          </small>
        </div>
        {#if elapsed >= 8}<span class="elapsed">{elapsed}s</span>{/if}
      </div>
    {/if}
    <div class="field-row">
      <label for="connection-provider">Provider
        <select id="connection-provider" value={providerId} disabled={pending} onchange={(event) => selectProvider((event.currentTarget as HTMLSelectElement).value)}>
          {#each PROVIDER_CHOICES as choice (choice.id)}
            <option value={choice.id}>{choice.name}{choice.placeholder ? ' — coming next' : ''}</option>
          {/each}
        </select>
      </label>
      {#if !chatgpt}<label for="connection-name">Name in Talo
        <input id="connection-name" bind:value={displayName} maxlength="80" placeholder={descriptor.name} disabled={placeholder || pending} />
      </label>{/if}
    </div>

    {#if placeholder}
      <div class="placeholder-note">
        <strong>Custom endpoints are not available yet.</strong>
      </div>
    {:else if chatgpt}
      {#if savedAccounts.length}
        <label for="chatgpt-account">Account
          <select id="chatgpt-account" bind:value={savedAccount} disabled={pending}>
            <option value="">Add a ChatGPT account</option>
            {#each savedAccounts as account (account.id)}
              <option value={account.id}>{account.name}</option>
            {/each}
          </select>
        </label>
      {/if}
      <div class="connect-actions">
        <button class="primary" type="submit" disabled={pending || chat.preview}>Continue with ChatGPT</button>
        {#if pending}<button type="button" onclick={cancelChatGPT}>Cancel sign-in</button>{/if}
      </div>
    {:else}
      {#if descriptor.keysUrl}<p class="provider-note">Get a key: <span class="link">{descriptor.keysUrl}</span></p>{/if}
      <label for="connection-key">API key
        <input
          id="connection-key"
          type="password"
          bind:value={apiKey}
          autocomplete="new-password"
          autocapitalize="off"
          spellcheck="false"
          maxlength="512"
          placeholder="Paste the key here"
          disabled={pending}
        />
      </label>
      <div class="field-row">
        <label for="connection-model">Starting model <span>(optional)</span>
          {#if descriptor.suggestedModels.length}
            <select id="connection-model" bind:value={modelId} disabled={pending}>
              <option value="">First available model</option>
              {#each descriptor.suggestedModels as model (model)}<option value={model}>{model}</option>{/each}
            </select>
          {:else}
            <input id="connection-model" bind:value={modelId} maxlength="200" placeholder="First available model" disabled={pending} />
          {/if}
        </label>
      </div>
      <div class="connect-actions">
        <button class="primary" type="submit" disabled={pending || chat.preview || !apiKey.trim()}>
          {pending ? 'Checking key…' : `Connect ${descriptor.name}`}
        </button>
      </div>
    {/if}
  </form>
  </details>

  <details class="catalog">
    <summary>What Talo supports</summary>
    <ul>
      {#each PROVIDER_CHOICES as choice (choice.id)}
        <li>
          <strong>{choice.name}</strong>
          {#if choice.keysUrl}<small class="link">{choice.keysUrl}</small>{/if}
        </li>
      {/each}
    </ul>
  </details>
</section>

<style>
  .providers{padding:24px 26px}
  .providers h2{font-size:17px;letter-spacing:-.4px;font-weight:550;margin:0 0 6px}
  .providers h3{font-size:13px}
  .form-error{margin:12px 0 2px}
  .pending{display:flex;align-items:flex-start;gap:11px;padding:12px 14px;margin:14px 0 4px;border:1px solid #4e4324;border-radius:9px;background:#211f16;color:#e8d59e}
  .pending strong{display:block;font-size:12px;font-weight:600;line-height:1.5}
  .pending small{display:block;font-size:11px;color:#c7b98a;line-height:1.55;margin-top:3px}
  .pending .elapsed{margin-left:auto;font-size:10.5px;color:#c7b98a;white-space:nowrap;padding-top:2px}
  .spinner{width:13px;height:13px;flex-shrink:0;margin-top:2px;border:2px solid #6b5a2c;border-top-color:var(--yellow);border-radius:50%;animation:spin .9s linear infinite}
  @keyframes spin{to{transform:rotate(360deg)}}
  .settings-message{display:flex;gap:12px;align-items:center;justify-content:space-between;padding:10px 13px;border-radius:9px;font-size:12px;margin:16px 0;line-height:1.5}
  .settings-message.error{background:var(--error-bg);border:1px solid var(--error-line);color:var(--error-text)}
  .settings-message.notice{background:#211f16;border:1px solid #4e4324;color:#e8d59e}
  .settings-message button{background:transparent;border:0;color:inherit;padding:0 4px;font-size:16px;line-height:1}
  .connected-list{margin-top:18px;display:grid;gap:10px}
  h3{font-size:13px;font-weight:600;margin:0}
  .connection{display:flex;align-items:center;justify-content:space-between;gap:18px;padding:13px 16px;border:1px solid var(--line);border-radius:12px;background:var(--soft)}
  .connection-main{min-width:0}
  .connection-title{display:flex;align-items:center;gap:9px;flex-wrap:wrap}
  .connection-title strong{font-size:13.5px;font-weight:550}
  .connection small{display:block;font-size:11px;color:var(--muted);margin-top:4px;line-height:1.5}
  .connection small.model-summary{color:var(--faint);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:520px}
  .connection small.discovery-error{color:var(--error-text)}
  .connection-actions{display:flex;gap:8px;flex-shrink:0}
  .connection-actions button{font-size:11px;padding:7px 11px}
  .quiet-danger{color:var(--muted)}
  .quiet-danger:hover:not(:disabled){color:var(--error-text);border-color:var(--error-line)}
  .tag.connected{font-size:9.5px;color:var(--yellow);border:1px solid #5c4b22;background:#282416;border-radius:20px;padding:3px 8px;white-space:nowrap}
  .empty-line{font-size:12px;color:var(--muted);margin:0;padding:13px 16px;border:1px solid var(--line);border-radius:12px;background:var(--soft)}
  .connect-details{border-top:1px solid var(--line);margin-top:18px;padding-top:14px}
  .connect-details>summary{cursor:pointer;font-size:12.5px;color:var(--muted);list-style:none;display:flex;align-items:center;gap:8px}
  .connect-details>summary::-webkit-details-marker{display:none}
  .connect-details>summary:hover{color:var(--text)}
  .connect-details>summary::before{content:"";width:0;height:0;border-left:4px solid currentColor;border-top:4px solid transparent;border-bottom:4px solid transparent;transition:transform .15s ease}
  .connect-details[open]>summary::before{transform:rotate(90deg)}
  .connect-form{margin-top:16px}
  .field-row{display:grid;grid-template-columns:1fr 1fr;gap:16px}
  .field-row label:only-child{grid-column:1 / -1;max-width:50%}
  label{display:block;font-size:12px;font-weight:550;margin:16px 0}
  label span{font-weight:400;color:var(--muted)}
  input,select{display:block;width:100%;padding:10px 11px;border:1px solid var(--line-strong);border-radius:8px;margin-top:8px;background:var(--soft);color:var(--text);font-size:12.5px}
  input:hover:not(:disabled),select:hover:not(:disabled){border-color:#4a4a41}
  input:disabled,select:disabled{opacity:.5}
  .provider-note{font-size:11.5px;color:var(--muted);line-height:1.6;margin:12px 0 0}
  .connect-actions{display:flex;align-items:center;gap:15px;margin-top:18px;flex-wrap:wrap}
  .placeholder-note{border:1px dashed var(--line-strong);border-radius:10px;padding:15px 17px;margin-top:16px}
  .placeholder-note strong{font-size:12.5px;font-weight:550}
  .catalog{border-top:1px solid var(--line);margin-top:24px;padding-top:17px}
  .catalog summary{cursor:pointer;font-size:12px;color:var(--muted);list-style-position:inside}
  .catalog summary:hover{color:var(--text)}
  .catalog ul{list-style:none;padding:0;margin:15px 0 0;display:grid;gap:12px}
  .catalog li{font-size:11.5px;display:grid;grid-template-columns:150px 1fr;gap:10px;align-items:baseline}
  .catalog li strong{font-weight:550;font-size:12px}
  .catalog li small{grid-column:2;color:var(--faint);word-break:break-all}
  .link{color:var(--text);opacity:.8;word-break:break-all}
  @media(prefers-reduced-motion:reduce){.spinner{animation:none}}
  @media(max-width:700px){
    .field-row{grid-template-columns:1fr;gap:0}
    .field-row label:only-child{max-width:none}
    .connection{flex-direction:column;gap:10px}
    .connection small.model-summary{white-space:normal;max-width:none}
    .catalog li{grid-template-columns:1fr;gap:3px}
    .catalog li small{grid-column:1}
  }
</style>
