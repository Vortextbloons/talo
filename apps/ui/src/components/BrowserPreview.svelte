<script lang="ts">
  import { browser, openBrowser, closeBrowser, controlBrowser } from '../lib/browser.svelte';
  const page = $derived(browser.status.tabs.find(tab => tab.id === browser.status.activeTab));
  const status = $derived(browser.status.state === 'stopped' ? 'Browser resting' : browser.status.state === 'crashed' ? 'Connection lost' : browser.status.busy ? 'Browsing in the background' : browser.status.owner === 'human' ? 'You have control' : browser.status.owner === 'paused' ? 'Automation paused' : 'Ready when you are');
</script>

<aside class="preview" aria-label="Browser preview">
  <div class="preview-heading"><span><span class:active={browser.status.state === 'connected'} class="dot"></span> Browser</span>{#if browser.view === 'side'}<button class="dismiss" onclick={() => closeBrowser()} aria-label="Hide live browser preview">×</button>{/if}</div>
  <button class="preview-card" onclick={() => void openBrowser('large')} aria-label="Open larger browser view">
    <div class="preview-page">
      {#if browser.frame}<img src={'data:image/jpeg;base64,' + browser.frame.data} alt="Live browser preview" draggable="false" />
      {:else}<div class="page-placeholder"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.2" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="3"/><path d="M3 9h18M7 6.5h.01M10 6.5h.01M8 13h8M8 16h5"/></svg><span>{browser.status.state === 'starting' ? 'Getting the browser ready' : browser.view !== 'closed' && browser.status.state === 'connected' ? 'Connecting live preview' : 'Browser opens when needed'}</span></div>{/if}
      <span class="expand" aria-hidden="true">↗</span>
    </div>
    <div class="page-caption"><strong>{page?.title || 'A space for the web'}</strong><span>{page?.url && page.url !== 'about:blank' ? page.url : 'Your browser, inside Talo'}</span></div>
  </button>
  <p>{status}</p>
  {#if browser.status.state === 'connected'}
    <div class="quick-controls">
      {#if browser.status.owner === 'agent'}<button disabled={browser.busy} onclick={async () => { await openBrowser('large'); await controlBrowser('take_over'); }}>Take Over</button>
      {:else}<button disabled={browser.busy} onclick={() => void controlBrowser('return')}>Return Control</button>{/if}
      <button disabled={browser.busy} onclick={() => void controlBrowser('stop')}>Stop</button>
    </div>
  {/if}
  {#if browser.error || browser.status.error}<small class="view-error" role="alert">{browser.error || browser.status.error}</small>{/if}
  <small>Separate tabs for this chat. Shared logins.</small>
</aside>

<style>
  .preview{width:100%;min-width:0;padding:0;margin:0}.preview-heading{display:flex;align-items:center;justify-content:space-between;margin:0 0 12px;font-size:11px;color:var(--muted)}.preview-heading>span{display:flex;gap:7px;align-items:center}.dot{width:5px;height:5px;border-radius:50%;background:var(--faint)}.dot.active{background:var(--yellow)}.dismiss{padding:0 3px;border:0;background:transparent;color:var(--muted);font-size:18px}
  .preview-card{display:block;padding:0;width:100%;border:1px solid var(--line);border-radius:11px;background:var(--paper);text-align:left;overflow:hidden;color:var(--text);cursor:pointer;transition:border-color .15s}.preview-card:hover{border-color:var(--yellow-deep)}button:focus-visible{outline:2px solid var(--yellow);outline-offset:3px}.preview-page{position:relative;aspect-ratio:8/5;background:var(--soft);overflow:hidden}.preview-page img{display:block;width:100%;height:100%;object-fit:contain;pointer-events:none}.page-placeholder{height:100%;display:grid;align-content:center;justify-items:center;gap:10px;color:var(--faint);font-size:10px}.page-placeholder svg{width:30px;height:30px}.expand{position:absolute;bottom:8px;right:9px;background:var(--paper);padding:1px 5px;border-radius:5px;color:var(--muted);font-size:15px}.page-caption{padding:12px;display:grid;gap:5px}.page-caption strong{font-size:11px;font-weight:500;white-space:nowrap;text-overflow:ellipsis;overflow:hidden}.page-caption>span{font-size:10px;color:var(--faint);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}.preview>p{font-size:11px;color:var(--muted);margin:12px 0 6px}.preview>small{display:block;color:var(--faint);font-size:10px;line-height:1.6}
  .quick-controls{display:flex;gap:7px;margin:0 0 10px}.quick-controls button{border:1px solid var(--line);border-radius:6px;background:transparent;color:var(--muted);font-size:10px;padding:5px 8px}.quick-controls button:hover{color:var(--yellow);border-color:var(--yellow-deep)}.quick-controls button:disabled{opacity:.4}.preview .view-error{color:var(--error-text);margin:8px 0}
  @media(prefers-reduced-motion:reduce){.preview-card{transition:none}}
</style>
