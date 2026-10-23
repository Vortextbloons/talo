<script lang="ts">
  import { browser, sendBrowserInput } from '../lib/browser.svelte';
  import type { BrowserInput } from '../../../../packages/contracts/src/browser';
  const manual = $derived(browser.status.owner === 'human' && !browser.busy);
  let movedAt = 0;
  const modifiers = (event: MouseEvent | KeyboardEvent) => (event.altKey ? 1 : 0) | (event.ctrlKey ? 2 : 0) | (event.metaKey ? 4 : 0) | (event.shiftKey ? 8 : 0);
  function pointer(event: PointerEvent | WheelEvent, type: BrowserInput['type']) {
    if (!manual || !browser.frame) return;
    if (type === 'mouseMoved' && Date.now() - movedAt < 40) return;
    movedAt = Date.now();
    const element = event.currentTarget as HTMLElement;
    const rect = element.getBoundingClientRect();
    const scale = Math.min(rect.width / 1280, rect.height / 800);
    const x = (event.clientX - rect.left - (rect.width - 1280 * scale) / 2) / scale;
    const y = (event.clientY - rect.top - (rect.height - 800 * scale) / 2) / scale;
    if (x < 0 || y < 0 || x > 1280 || y > 800) return;
    event.preventDefault();
    if (type === 'mousePressed') { element.focus(); element.setPointerCapture((event as PointerEvent).pointerId); }
    void sendBrowserInput({ type, x, y, modifiers: modifiers(event),
      button: event.button === 2 ? 'right' : event.button === 1 ? 'middle' : 'left', buttons: event.buttons,
      clickCount: type === 'mousePressed' || type === 'mouseReleased' ? Math.max(1, event.detail) : 0,
      ...(type === 'mouseWheel' ? { deltaX: (event as WheelEvent).deltaX, deltaY: (event as WheelEvent).deltaY } : {}),
    });
  }
  function keyboard(event: KeyboardEvent, type: 'keyDown' | 'keyUp') {
    if (!manual || event.isComposing || ['Dead','Process','Unidentified'].includes(event.key)) return;
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'v') return;
    event.preventDefault(); event.stopPropagation();
    void sendBrowserInput({ type, key: event.key, code: event.code, keyCode: event.keyCode, modifiers: modifiers(event) });
  }
  function paste(event: ClipboardEvent) {
    if (!manual) return;
    event.preventDefault();
    const text = event.clipboardData?.getData('text/plain');
    if (text) void sendBrowserInput({ type: 'text', text: text.slice(0,16000) });
  }
</script>



<div class:manual class="surface" role="application" tabindex={manual ? 0 : -1}
  aria-label={manual ? 'Interactive browser page. Type or click to browse. Paste supports text.' : 'Live browser page, watch only. Choose Take Over to interact.'}
  onpointerdown={event => pointer(event, 'mousePressed')} onpointerup={event => pointer(event, 'mouseReleased')}
  onpointermove={event => pointer(event, 'mouseMoved')} onwheel={event => pointer(event, 'mouseWheel')}
  onkeydown={event => keyboard(event, 'keyDown')} onkeyup={event => keyboard(event, 'keyUp')}
  onpaste={paste} oncompositionend={event => { if (manual && event.data) void sendBrowserInput({ type: 'text', text: event.data }); }}
  oncontextmenu={event => event.preventDefault()}>
  {#if browser.frame}<img src={'data:image/jpeg;base64,' + browser.frame.data} alt="Current browser page" draggable="false" />
  {:else}<div class="empty-page"><span class="page-mark" aria-hidden="true">↗</span><strong>{browser.busy || browser.status.state === 'starting' ? 'Opening your browser…' : browser.status.state === 'crashed' ? 'The browser disconnected' : browser.status.state === 'connected' ? 'Connecting to the page…' : 'Your browser lives here'}</strong><span>{browser.status.state === 'stopped' ? 'Open it when you want to browse or watch Talo.' : 'The page stays separate from your conversation.'}</span></div>{/if}
</div>

<style>
  .surface{position:relative;flex:1;min-height:160px;overflow:hidden;background:var(--black);display:grid;place-items:center;touch-action:none;outline:none}.surface.manual{cursor:default}.surface.manual:focus-visible{box-shadow:inset 0 0 0 2px var(--yellow)}img{display:block;width:100%;height:100%;min-height:0;max-height:100%;object-fit:contain;position:absolute;inset:0;user-select:none;pointer-events:none}.empty-page{padding:32px;text-align:center;color:var(--faint);display:grid;justify-items:center;gap:12px}.empty-page strong{font-size:16px;color:var(--muted);font-weight:500}.empty-page>span:last-child{font-size:12px;line-height:1.6}.page-mark{font-size:35px;color:var(--yellow-deep);margin-bottom:4px}
</style>
