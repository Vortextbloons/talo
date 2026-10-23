<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import type { GoogleStatus } from '../../../../packages/contracts/src/google';

  let status = $state<GoogleStatus>({
    configured: false, connected: false, scopes: [], missingPermissions: [],
    needsReconnect: false, authorizing: false,
  });
  let busy = $state(false);
  let loading = $state(true);
  let error = $state('');
  const desktop = isTauri();

  const label = $derived(
    status.connected
      ? status.needsReconnect ? 'Sign in again' : 'Connected'
      : status.authorizing ? 'Waiting for Google'
      : status.configured ? 'Not connected'
      : 'Not available'
  );
  const granted = $derived(status.scopes.filter(scope => scope.granted).length);
  const stateClass = $derived(status.connected && !status.needsReconnect ? 'on' : status.authorizing ? 'waiting' : '');

  async function refresh(): Promise<void> {
    if (!desktop) { loading = false; return; }
    try {
      status = await invoke<GoogleStatus>('google_action', { operation: 'state' });
    } catch (e) { error = String(e); }
    finally { loading = false; }
  }

  async function action(operation: string): Promise<void> {
    if (busy) return;
    busy = true;
    error = '';
    try {
      status = await invoke<GoogleStatus>('google_action', { operation, payload: null });
    } catch (e) { error = String(e); }
    finally { busy = false; }
  }

  onMount(() => {
    let active = true;
    let stop: (() => void) | undefined;
    void refresh();
    if (desktop) {
      void listen('google-changed', () => { if (active) void refresh(); }).then(dispose => {
        if (active) stop = dispose; else dispose();
      });
    }
    return () => { active = false; stop?.(); };
  });
</script>

<section class="integration">
  <div class="heading">
    <div class="mark" aria-hidden="true">
      <span class="blue"></span><span class="red"></span><span class="yellow"></span><span class="green"></span>
    </div>
    <div><h2>Google Workspace</h2><p>Gmail, Calendar and Drive, only when you ask.</p></div>
    <span class="status" class:on={stateClass === 'on'} class:waiting={stateClass === 'waiting'}>{loading ? 'Loading' : label}</span>
  </div>

  <div class="body">
    <p>Talo talks to Google from this PC, not through another service. Mail, events and files are sent to your chosen model provider only when a chat asks about them. Sending email, changing calendar events and writing files ask for your approval every time.</p>

    {#if !desktop}
      <p class="notice">Open the desktop app to connect Google.</p>
    {:else if status.connected}
      <div class="account">
        <div>
          <strong>{status.account || 'Connected Google account'}</strong>
          <span class="meta">{granted} of {status.scopes.length} permissions granted</span>
        </div>
        <ul class="scopes">
          {#each status.scopes as scope (scope.id)}
            <li class:missed={!scope.granted}><span aria-hidden="true"></span>{scope.label}</li>
          {/each}
        </ul>
      </div>
      {#if status.missingPermissions.length}
        <p class="notice">Google did not grant {status.missingPermissions.join(', ')}. Those operations stay unavailable until you reconnect and allow them.</p>
      {/if}
      <div class="actions">
        <button class="primary" disabled={busy} onclick={() => void action('connect')}>{status.needsReconnect || status.missingPermissions.length ? 'Reconnect Google' : 'Sign in again'}</button>
        <button disabled={busy} onclick={() => void action('disconnect')}>Disconnect</button>
      </div>
    {:else if status.authorizing}
      <div class="waiting-card">
        <strong>Finish in your browser</strong>
        <p>Choose your Google account and allow the permissions, then return here. Talo finishes as soon as Google answers.</p>
        <button disabled={busy} onclick={() => void action('cancel')}>Cancel sign-in</button>
      </div>
    {:else if !status.configured}
      <div class="notice">
        <strong>Google isn't available in this build</strong>
        <p>This copy of Talo was built without the app's Google configuration, so sign-in can't start here. Everything else works as usual.</p>
      </div>
    {:else}
      <div class="connect">
        <button class="primary" disabled={busy} onclick={() => void action('connect')}>{busy ? 'Opening your browser…' : 'Connect Google'}</button>
        <p>Your default browser opens Google's own sign-in. Talo never sees your password: it waits once on this PC for Google's answer and keeps the result encrypted.</p>
        <ul class="scopes requested">
          {#each status.scopes as scope (scope.id)}
            <li><span aria-hidden="true"></span>{scope.label}</li>
          {/each}
        </ul>
      </div>
    {/if}

    {#if error || status.error}<p class="error" role="alert">{error || status.error}</p>{/if}

    <p class="boundary">Tokens stay in Windows-protected storage on this PC. Disconnecting removes them here and revokes the access at Google.</p>
  </div>
</section>

<style>
  .integration{max-width:760px;border:1px solid var(--line);border-radius:14px;background:var(--panel);overflow:hidden}
  .heading{display:flex;align-items:center;gap:13px;padding:23px 25px;border-bottom:1px solid var(--line)}
  .mark{width:42px;height:42px;display:grid;grid-template-columns:1fr 1fr;grid-template-rows:1fr 1fr;gap:3px;padding:9px;border-radius:12px;background:var(--soft);flex-shrink:0}
  .mark span{border-radius:2px;opacity:.9}
  .mark .blue{background:#6f9ae8}.mark .red{background:#e08a80}.mark .yellow{background:#e8cd77}.mark .green{background:#86c19a}
  h2{font-size:18px;margin:0 0 4px;font-weight:550}
  p{margin:0;color:var(--muted);font-size:12.5px;line-height:1.65}
  .heading p{font-size:12px}
  .status{margin-left:auto;font-size:10.5px;color:var(--muted);border:1px solid var(--line);border-radius:20px;padding:5px 9px;white-space:nowrap}
  .status.on{color:#b9d8b0;border-color:#40533c;background:#253021}
  .status.waiting{color:var(--accent-note);border-color:var(--accent-line);background:var(--accent-bg)}
  .body{display:grid;gap:20px;padding:24px 25px}
  .body>p{max-width:590px}
  strong{font-size:13px;font-weight:550}
  .account{display:grid;gap:13px;border:1px solid var(--line);border-radius:10px;padding:16px;background:var(--paper)}
  .account .meta{display:block;color:var(--muted);font-size:12px;margin-top:3px}
  .scopes{list-style:none;display:grid;grid-template-columns:repeat(auto-fit,minmax(220px,1fr));gap:7px 18px;margin:0;padding:0;max-width:640px}
  .scopes li{display:flex;align-items:center;gap:8px;font-size:12px;color:var(--muted)}
  .scopes li span{display:inline-block;width:6px;height:6px;border-radius:50%;background:#86c19a;flex-shrink:0}
  .scopes li.missed{color:var(--faint)}
  .scopes li.missed span{background:transparent;border:1px solid var(--faint)}
  .scopes.requested li span{background:var(--line-strong)}
  .actions{display:flex;gap:8px;flex-wrap:wrap}
  .connect{display:grid;gap:12px;justify-items:start}
  .connect p{max-width:560px}
  button{background:var(--soft);border:1px solid var(--line-strong);border-radius:8px;color:var(--text);padding:9px 13px;font-size:12px}
  button.primary{background:var(--yellow);border-color:var(--yellow);color:var(--black);font-weight:550;padding:11px 16px}
  button:hover:not(:disabled){filter:brightness(1.08)}
  .waiting-card{border:1px solid var(--accent-line);background:var(--accent-bg);border-radius:10px;padding:16px;display:grid;gap:10px;justify-items:start}
  .notice{border:1px solid var(--line);border-radius:10px;padding:16px;display:grid;gap:7px}
  .notice p{font-size:12px}
  .boundary{font-size:11.5px;color:var(--faint);border-top:1px solid var(--line);padding-top:16px}
  .error{color:var(--error-text)}
  @media(max-width:600px){.heading,.body{padding:18px}.heading{flex-wrap:wrap}.status{margin-left:55px}.scopes{grid-template-columns:1fr}}
  @media(prefers-reduced-motion:reduce){*{transition:none!important}}
</style>
