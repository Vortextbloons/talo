<script lang="ts">
  import { isTauri } from '@tauri-apps/api/core';
  import { browser, openBrowser, closeBrowser, controlBrowser, sendBrowserInput, resolveBrowserApproval } from '../lib/browser.svelte';
  import BrowserSurface from './BrowserSurface.svelte';
  let { pageView = false }: { pageView?: boolean } = $props();
  let address = $state('');
  const current = $derived(browser.status.tabs.find(tab => tab.id === browser.status.activeTab));
  const manual = $derived(browser.status.owner === 'human');
  const connected = $derived(browser.status.state === 'connected');
  async function navigate(event: SubmitEvent) {
    event.preventDefault();
    if (!manual || !address.trim()) return;
    await sendBrowserInput({ type: 'navigate', url: /^https?:\/\//iu.test(address.trim()) ? address.trim() : 'https://' + address.trim() });
    address = '';
  }
</script>

{#snippet viewer()}
  <div class="browser-head">
    <div class="browser-title"><span class:connected class="connection-dot"></span><strong>Browser</strong><span class="owner">{manual ? 'You have control' : browser.status.owner === 'paused' ? 'Paused' : 'Watching Pi'}</span></div>
    <div class="browser-actions">
      {#if !connected || browser.view === 'closed'}<button class="primary" disabled={browser.busy || !isTauri()} onclick={() => void openBrowser('large')}>Open Browser</button>
      {:else if manual}<button class="primary" disabled={browser.busy} onclick={() => void controlBrowser('return')}>Return Control</button>
      {:else}<button class="primary" disabled={browser.busy} onclick={() => void controlBrowser('take_over')}>Take Over</button>{#if browser.status.owner === 'paused'}<button disabled={browser.busy} onclick={() => void controlBrowser('return')}>Return Control</button>{:else}<button disabled={browser.busy} onclick={() => void controlBrowser('pause')}>Pause</button>{/if}{/if}
      {#if connected}<button disabled={browser.busy} onclick={() => void controlBrowser('stop')}>Stop</button>{/if}
      {#if !pageView}<button class="close" onclick={closeBrowser} aria-label="Close browser view" title="Close view. Pi can keep browsing.">&times;</button>{/if}
    </div>
  </div>
  <div class="page-bar">
    <button disabled={!manual || browser.busy} onclick={() => void sendBrowserInput({ type: 'back' })} aria-label="Go back">&#8592;</button>
    <button disabled={!manual || browser.busy} onclick={() => void sendBrowserInput({ type: 'forward' })} aria-label="Go forward">&#8594;</button>
    <button disabled={!manual || browser.busy} onclick={() => void sendBrowserInput({ type: 'reload' })} aria-label="Reload page">&#8635;</button>
    <form onsubmit={navigate}><input aria-label="Website address" readonly={!manual} bind:value={address} placeholder={current?.url && current.url !== 'about:blank' ? current.url : manual ? 'Enter a website address' : 'Pi will open a page here'} autocomplete="off" spellcheck="false" /></form>
  </div>
  {#if pageView && browser.view === 'closed'}<div class="landing"><strong>A window into what Pi is doing.</strong><p>Open the browser to watch. Take Over when you want to interact.</p><button class="primary" disabled={browser.busy || !isTauri()} onclick={() => void openBrowser('large')}>Open Browser</button></div>
  {:else}<BrowserSurface />{/if}
  <div class="browser-foot"><span>{current?.title || 'Talo browser'}</span><span>{manual ? 'Click the page to type or browse' : 'Watch only - Pi keeps working'}</span></div>
  {#if browser.status.error || browser.error}<p class="error" role="alert">{browser.error || browser.status.error}</p>{/if}
{/snippet}

{#if pageView}
  <section class="viewer inline" aria-label="Browser">{@render viewer()}</section>
{:else if browser.view === 'large'}
  <dialog class="browser-dialog" aria-label="Browser" oncancel={event => { event.preventDefault(); closeBrowser(); }} {@attach element => { element.showModal(); return () => element.close(); }}>
    <div class="viewer">{@render viewer()}</div>
  </dialog>
{/if}

{#if browser.approvals.length}
  {@const approval = browser.approvals[0]}
  <dialog class="approval-dialog" aria-labelledby="browser-approval-title" oncancel={event => { event.preventDefault(); void resolveBrowserApproval(approval.id, false); }} {@attach element => { element.showModal(); return () => element.close(); }}>
    <span class="eyebrow">Browser permission &middot; {browser.approvals.length} pending</span>
    <h2 id="browser-approval-title">Allow this browser action?</h2>
    <p>Review the action before Pi continues.</p>
    <strong>{approval.tool.replace('browser_', '').replaceAll('_', ' ')}</strong>
    {#if approval.page?.url}<p>{approval.page.title} &middot; {approval.page.url}</p>{/if}
    <pre>{JSON.stringify(approval.arguments, null, 2)}</pre>
    <p class="hint">This action only. Approval expires after 90 seconds.</p>
    <div class="browser-actions"><button disabled={browser.busy} onclick={() => void resolveBrowserApproval(approval.id, false)}>Deny</button><button class="primary" disabled={browser.busy} onclick={() => void resolveBrowserApproval(approval.id, true)}>Allow once</button><button disabled={browser.busy} onclick={async () => { await resolveBrowserApproval(approval.id, false); await openBrowser('large'); await controlBrowser('take_over'); }}>Take Over</button></div>
  </dialog>
{/if}

<style>
  .viewer{display:flex;flex-direction:column;min-height:0;height:100%;overflow:hidden;background:var(--paper);color:var(--text)}.inline{flex:1;min-height:380px;border:1px solid var(--line);border-radius:14px;overflow:hidden;margin-top:20px}.browser-dialog{padding:0;width:calc(100vw - 48px);height:calc(100dvh - 48px);max-width:1500px;max-height:none;border:1px solid var(--line-strong);border-radius:14px;background:var(--paper);color:var(--text);overflow:hidden;box-shadow:0 24px 100px #0008}.browser-dialog::backdrop,.approval-dialog::backdrop{background:#080808b8;backdrop-filter:blur(5px)}
  .browser-head{display:flex;align-items:center;justify-content:space-between;gap:16px;padding:14px 18px;border-bottom:1px solid var(--line)}.browser-title{display:flex;align-items:center;gap:9px;min-width:0}.browser-title strong{font-size:13px;font-weight:550}.connection-dot{width:6px;height:6px;background:var(--faint);border-radius:50%;flex-shrink:0}.connection-dot.connected{background:var(--yellow)}.owner{color:var(--muted);font-size:11px;margin-left:7px}.browser-actions{display:flex;gap:8px;align-items:center;flex-wrap:wrap}button{border:1px solid var(--line);background:var(--soft);color:var(--text);padding:7px 11px;border-radius:7px;cursor:pointer;font-size:11px}.primary{background:var(--yellow);border-color:var(--yellow);color:var(--black);font-weight:600}button:disabled{opacity:.4;cursor:default}button:focus-visible,input:focus-visible{outline:2px solid var(--yellow);outline-offset:2px}button.close{font-size:21px;padding:0 4px;line-height:1;border-color:transparent;background:transparent;margin-left:7px;color:var(--muted)}
  .page-bar{display:flex;align-items:center;padding:10px 16px;gap:5px;border-bottom:1px solid var(--line)}.page-bar>button{background:transparent;border-color:transparent;padding:4px 6px;font-size:16px}.page-bar form{flex:1;min-width:0;margin-left:5px}.page-bar input{width:100%;border:1px solid var(--line);border-radius:7px;padding:8px 11px;background:var(--bg);color:var(--muted);font-size:11px}.page-bar input[readonly]{cursor:default}.browser-foot{display:flex;justify-content:space-between;gap:16px;padding:10px 17px;font-size:10px;color:var(--faint);border-top:1px solid var(--line)}.browser-foot span:first-child{overflow:hidden;white-space:nowrap;text-overflow:ellipsis}.browser-foot span:last-child{white-space:nowrap}.error{font-size:12px;color:var(--error-text);padding:10px 17px;margin:0;border-top:1px solid var(--line)}.landing{flex:1;display:grid;justify-items:center;align-content:center;padding:48px 24px;text-align:center}.landing strong{font-size:20px;font-weight:500;letter-spacing:-.5px}.landing p{color:var(--muted);font-size:12px;line-height:1.7;margin:12px 0 24px}
  .approval-dialog{border:1px solid var(--line-strong);border-radius:14px;background:var(--paper);color:var(--text);width:min(540px,calc(100vw - 32px));max-height:85vh;overflow:auto;padding:28px}h2{font-size:22px;letter-spacing:-.6px;margin:14px 0}.approval-dialog p{font-size:12px;line-height:1.7;color:var(--muted)}.eyebrow{font-size:10px;text-transform:uppercase;letter-spacing:.08em;color:var(--yellow)}pre{padding:12px;background:var(--soft);font-size:12px;white-space:pre-wrap;overflow-wrap:anywhere;max-height:220px;overflow:auto}
  @media(max-width:650px){.browser-dialog{width:calc(100vw - 16px);height:calc(100dvh - 16px);border-radius:10px}.browser-head{padding:11px}.owner{margin-left:0;font-size:10px}.browser-foot span:first-child{display:none}.page-bar{padding:8px}.browser-title{gap:6px}}
</style>
