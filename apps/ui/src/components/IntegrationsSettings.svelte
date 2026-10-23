<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import type { TelegramStatus } from '../../../../packages/contracts/src/telegram';
  import GoogleSettings from './GoogleSettings.svelte';

  let status = $state<TelegramStatus>({ connected: false, paired: false, online: false });
  let token = $state('');
  let busy = $state(false);
  let loading = $state(true);
  let error = $state('');
  const desktop = isTauri();
  const label = $derived(status.paired ? (status.online ? 'Connected' : 'Reconnecting') : status.connected ? 'Finish pairing' : 'Not connected');

  async function refresh(): Promise<void> {
    if (!desktop) { loading = false; return; }
    try { status = await invoke<TelegramStatus>('telegram_action', { operation: 'state' }); }
    catch (e) { error = String(e); }
    finally { loading = false; }
  }
  async function action(operation: string): Promise<void> {
    if (busy) return;
    busy = true;
    error = '';
    const payload = operation === 'connect' ? { token: token.trim() } : undefined;
    token = '';
    try { status = await invoke<TelegramStatus>('telegram_action', { operation, payload }); }
    catch (e) { error = String(e); }
    finally { busy = false; }
  }
  onMount(() => {
    let active = true;
    let stop: (() => void) | undefined;
    void refresh();
    if (desktop) void listen('telegram-changed', () => { if (active) void refresh(); }).then(dispose => { if (active) stop = dispose; else dispose(); });
    return () => { active = false; stop?.(); token = ''; };
  });
</script>

<div class="integrations">
  <GoogleSettings />

<section class="integration">
  <div class="heading">
    <div class="telegram-mark" aria-hidden="true"><svg viewBox="0 0 24 24" fill="none"><path d="m20 4-3 16-6-5-3 3 1-6 8-5-10 4-4-2 17-5Z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/></svg></div>
    <div><h2>Telegram</h2></div>
    <span class="status" class:online={status.paired && status.online}>{loading ? 'Loading' : label}</span>
  </div>
  <div class="body">
    {#if !desktop}
      <p class="notice">Open the desktop app to connect Telegram.</p>
    {:else if status.paired}
      <div class="connection"><strong>{status.accountName || 'Your Telegram account'}</strong><span>{status.botUsername ? `@${status.botUsername}` : 'Telegram bot'} · Primary chat</span></div>
      <button disabled={busy} onclick={() => void action('disconnect')}>Disconnect Telegram</button>
    {:else if status.connected}
      <div class="pairing">
        {#if status.pendingAccount}
          <strong>Is this your account?</strong>
          <p>{status.pendingAccount.name}. Confirm only if you opened the pairing link.</p>
          <button class="primary" disabled={busy} onclick={() => void action('confirm')}>Connect this account</button>
        {:else}
          <p>Tap Start in Telegram, then confirm your account here.</p>
          {#if status.pairingUrl}<button class="pair-link" disabled={busy} onclick={() => void action('open_pair')}>Open Telegram to pair <span aria-hidden="true">&nearr;</span></button>{/if}
        {/if}
        <div class="actions"><button disabled={busy} onclick={() => void action('pair')}>New pairing link</button><button disabled={busy} onclick={() => void action('disconnect')}>Cancel connection</button></div>
      </div>
    {:else}
      <form onsubmit={event => { event.preventDefault(); void action('connect'); }}>
        <p>Create a bot with <button class="text-link" type="button" disabled={busy} onclick={() => void action('open_botfather')}>@BotFather</button> using /newbot, then paste its token.</p>
        <label for="telegram-token">Bot token</label>
        <div class="token-row"><input id="telegram-token" type="password" bind:value={token} autocomplete="off" spellcheck="false" placeholder="Paste your bot token" disabled={busy}/><button class="primary" disabled={busy || !token.trim()}>{busy ? 'Connecting…' : 'Connect bot'}</button></div>
      </form>
    {/if}
    {#if error || status.error}<p class="error" role="alert">{error || status.error}</p>{/if}
    <p class="availability">Requires Talo running on an awake, online PC.</p>
  </div>
</section>
</div>

<style>
  .integrations{display:grid;gap:22px;max-width:760px}
  .integration{max-width:760px;border:1px solid var(--line);border-radius:14px;background:var(--panel);overflow:hidden}
  .heading{display:flex;align-items:center;gap:13px;padding:23px 25px;border-bottom:1px solid var(--line)}
  .telegram-mark{width:42px;height:42px;display:grid;place-items:center;border-radius:12px;background:#233844;color:#8dcee8;flex-shrink:0}.telegram-mark svg{width:27px;height:27px}
  h2{font-size:18px;margin:0 0 4px;font-weight:550}p{margin:0;color:var(--muted);font-size:12.5px;line-height:1.65}
  .status{margin-left:auto;font-size:10.5px;color:var(--muted);border:1px solid var(--line);border-radius:20px;padding:5px 9px;white-space:nowrap}.status.online{color:#b9d8b0;border-color:#40533c;background:#253021}
  .body{display:grid;gap:23px;padding:24px 25px}.body>p{max-width:590px}strong{font-size:13px;font-weight:550}form,.pairing{display:grid;gap:10px}label{font-size:11px;color:var(--muted);margin-top:5px}.token-row{display:flex;gap:8px}input{flex:1;min-width:0;background:var(--soft);border:1px solid var(--line-strong);border-radius:8px;color:var(--text);padding:11px 12px;font:inherit;font-size:12px}
  button{justify-self:start;background:var(--soft);border:1px solid var(--line-strong);border-radius:8px;color:var(--text);padding:9px 13px;font:inherit;font-size:12px;cursor:pointer}button.primary{background:var(--yellow);border-color:var(--yellow);color:#222014;font-weight:550}button:disabled{opacity:.45;cursor:not-allowed}button:hover:not(:disabled){filter:brightness(1.08)}.pair-link{justify-self:start;display:flex;gap:24px;padding:12px 14px;border:1px solid #3c5c69;border-radius:8px;text-decoration:none;background:#22313a;font-size:12px}.actions{display:flex;gap:8px;flex-wrap:wrap;margin-top:5px}.connection{display:grid;gap:5px}.connection span{color:var(--muted);font-size:12px}.availability{border-top:1px solid var(--line);padding-top:16px;font-size:11.5px}.error{color:var(--error-text)}.notice{border:1px solid var(--line);border-radius:8px;padding:12px}
  .text-link{padding:0;border:0;background:transparent;color:#9dcfe1;text-decoration:underline;text-underline-offset:3px;font:inherit}button:focus-visible{outline:2px solid var(--yellow);outline-offset:3px}
  @media(max-width:600px){.heading,.body{padding:18px}.heading{flex-wrap:wrap}.status{margin-left:55px}.token-row{flex-direction:column}.token-row button{align-self:flex-start}}
</style>
