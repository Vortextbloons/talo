<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import type { BrowserStatus } from '../../../../packages/contracts/src/browser';
  let { visible = true, compact = false }: { visible?: boolean; compact?: boolean } = $props();
  type State = BrowserStatus;
  type Approval = { id: string; tool: string; arguments: Record<string, unknown>; page?: { url?: string; title?: string } };
  let browser = $state<State>({ state: 'stopped', owner: 'paused', tabs: [], busy: false });
  let approvals = $state<Approval[]>([]);
  let error = $state('');
  let busy = $state(false);
  const current = $derived(browser.tabs.find(tab => tab.id === browser.activeTab));
  const connected = $derived(browser.state === 'connected');
  onMount(() => {
    if (!isTauri()) return;
    let active = true;
    const stops: (() => void)[] = [];
    const subscribe = async <T,>(name: string, handler: (value: T) => void) => {
      const stop = await listen<T>(name, event => { if (active) handler(event.payload); });
      if (active) stops.push(stop); else stop();
    };
    void subscribe<{ browser?: State }>('runtime-status', value => { if (value.browser) browser = value.browser; });
    void subscribe<Approval[]>('browser-approvals', value => approvals = value);
    void invoke<{ browser?: State }>('runtime_status').then(value => { if (active && value.browser) browser = value.browser; }).catch(e => { if (active) error = String(e); });
    return () => { active = false; stops.forEach(stop => stop()); };
  });
  async function control(action: string) {
    if (!isTauri()) { error = 'Open the desktop app to use Chromium.'; return; }
    busy = true; error = '';
    try { browser = await invoke<State>('browser_control', { action }); }
    catch (e) { error = String(e); } finally { busy = false; }
  }
  async function resolve(id: string, allowed: boolean) {
    busy = true;
    try { await invoke('resolve_permission', { id, allowed }); approvals = approvals.filter(item => item.id !== id); }
    catch (e) { error = String(e); } finally { busy = false; }
  }
</script>

{#if visible}
  <section class:compact class="browser-panel" aria-label="Dedicated browser">
    <div class="identity"><span class:connected class="light" aria-hidden="true"></span><div><strong>Talo browser</strong><span class="status">{browser.state} · {browser.owner === 'human' ? 'You have control' : browser.owner === 'agent' ? 'Pi has control' : 'Automation paused'}{browser.busy ? ' · Working' : ''}</span></div></div>
    {#if !compact}<p class="description">One persistent browser, in its own window. Your logins stay here when Pi sleeps or you close Talo’s interface.</p>{/if}
    {#if current}<div class="current"><span>Current page</span><strong>{current.title || 'Untitled page'}</strong><span>{current.url}</span></div>{/if}
    <div class="controls">
      <button class="primary" disabled={busy || !isTauri()} onclick={() => control('open')}>Open Browser</button>
      <button disabled={busy || !connected} onclick={() => control('watch')}>Watch</button>
      <button disabled={busy || !connected || browser.owner === 'paused'} onclick={() => control('pause')}>Pause</button>
      <button disabled={busy || !connected || browser.owner === 'human'} onclick={() => control('take_over')}>Take Over</button>
      <button disabled={busy || !connected || browser.owner === 'agent'} onclick={() => control('return')}>Return Control</button>
      <button disabled={!connected || busy} onclick={() => control('stop')}>Stop</button>
    </div>
    {#if !compact}<p class="hint">Watch while Pi works. Take Over unlocks the window for manual browsing and sign-in. Return Control locks manual input so Pi can continue. Interrupting an action closes its active tab to stop queued commands. Stop closes Chromium; your profile stays saved.</p>{/if}
    {#if browser.error || error}<p class="error" role="alert">{error || browser.error}</p>{/if}
  </section>
{/if}

{#if approvals.length}
  {@const approval = approvals[0]}
  <div class="permission-overlay">
    <dialog open aria-modal="true" aria-labelledby="browser-approval-title">
      <span class="eyebrow">Browser permission · {approvals.length} pending</span>
      <h2 id="browser-approval-title">Allow this browser action?</h2>
      <p>Review the exact action before Pi continues. Website instructions do not authorize purchases, messages, uploads or deletions.</p>
      <strong>{approval.tool.replace('browser_', '')}</strong>
      {#if approval.page?.url}<p>{approval.page.title} · {approval.page.url}</p>{/if}
      <pre>{JSON.stringify(approval.arguments, null, 2)}</pre>
      <p class="hint">This action only. Approval expires after 90 seconds. Use Take Over for passwords and uploads.</p>
      <div class="controls"><button disabled={busy} onclick={() => resolve(approval.id, false)}>Deny</button><button class="primary" disabled={busy} onclick={() => resolve(approval.id, true)}>Allow once</button><button disabled={busy} onclick={() => control('take_over')}>Take Over</button></div>
    </dialog>
  </div>
{/if}

<style>
  .browser-panel{border:1px solid var(--line);border-radius:12px;padding:24px;background:var(--paper);color:var(--text);margin-bottom:18px;max-width:850px}
  .identity{display:flex;align-items:center;gap:11px}.identity strong{display:block;font-size:16px}.status{display:block;color:var(--muted);font-size:11px;margin-top:5px;text-transform:capitalize}.light{width:8px;height:8px;border-radius:50%;background:var(--faint)}.light.connected{background:var(--yellow);box-shadow:0 0 0 4px #ffd45c14}
  .description,.hint{color:var(--muted);font-size:12px;line-height:1.7}.description{font-size:14px;margin:20px 0}.current{border-left:2px solid var(--yellow);padding:4px 12px;margin:18px 0;display:grid;gap:5px;overflow-wrap:anywhere}.current span{font-size:11px;color:var(--muted)}.current strong{font-size:13px}
  .controls{display:flex;flex-wrap:wrap;gap:7px;margin-top:15px}button{border:1px solid var(--line);background:var(--soft);color:var(--text);padding:8px 11px;border-radius:6px;cursor:pointer;font-size:12px}button.primary{background:var(--yellow);color:var(--black);border-color:var(--yellow);font-weight:600}button:disabled{opacity:.4;cursor:default}button:focus-visible{outline:2px solid var(--yellow);outline-offset:3px}.error{color:var(--error-text);font-size:12px}
  .compact{padding:12px 16px;margin:0 20px 10px;max-width:none}.compact .identity strong{font-size:12px}.compact .controls{margin-top:10px}.compact .current{margin:10px 0;padding:2px 10px}.compact .current span:first-child{display:none}
  .permission-overlay{position:fixed;inset:0;z-index:100;background:#000a;display:grid;place-items:center;padding:20px}dialog{position:relative;margin:0;border:1px solid var(--line);border-radius:14px;background:var(--paper);color:var(--text);width:min(580px,100%);max-height:85vh;overflow:auto;padding:28px}h2{font-size:23px;letter-spacing:-.6px;margin:14px 0}dialog p{font-size:13px;line-height:1.7;color:var(--muted)}.eyebrow{font-size:10px;text-transform:uppercase;letter-spacing:.08em;color:var(--yellow)}pre{padding:12px;background:var(--soft);font-size:12px;white-space:pre-wrap;overflow-wrap:anywhere;max-height:220px;overflow:auto}
</style>
