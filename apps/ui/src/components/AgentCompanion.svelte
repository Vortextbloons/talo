<script lang="ts">
  import mascot from '../../../../packages/design/mascot.svg';
  import BrowserPreview from './BrowserPreview.svelte';
  import { browser } from '../lib/browser.svelte';
  import { chat, selectionLabel } from '../lib/chat.svelte';

  let { name = 'Talo', onRename }: { name?: string; onRename: (name: string) => Promise<void> } = $props();
  let editing = $state(false);
  let draft = $state('');
  let saving = $state(false);
  let error = $state('');
  const working = $derived(chat.active?.state === 'generating' || chat.active?.state === 'cancelling');
  const approvals = $derived(browser.approvals.length);
  const activity = $derived(approvals ? 'Needs your approval' : browser.status.owner === 'human' ? 'You have browser control' : browser.status.busy ? 'Exploring the web' : chat.active?.state === 'cancelling' ? 'Stopping' : working ? 'Thinking it through' : chat.worker.state === 'failed' ? 'Needs a moment' : 'Here when you need me');
  const model = $derived(selectionLabel(chat.active?.conversation.selection ?? chat.draftSelection));
  async function rename() {
    if (saving) return;
    const value = draft.trim();
    if (!value || value.length > 80 || /[\u0000-\u001f\u007f]/u.test(value)) { error = 'Choose a name between 1 and 80 characters.'; return; }
    saving = true; error = '';
    try { await onRename(value); editing = false; }
    catch { error = 'The name could not be saved. Try again.'; }
    finally { saving = false; }
  }
</script>

<aside class="companion" aria-label="Your agent and browser">
  <section class="identity" class:working class:attention={approvals > 0}>
    <span class="eyebrow">Your companion</span>
    <div class="avatar-stage" aria-hidden="true"><span class="orbit"></span><img src={mascot} alt="" /><span class="shadow"></span></div>
    {#if editing}
      <form class="rename" onsubmit={event => { event.preventDefault(); void rename(); }}>
        <label for="agent-name">Agent name</label>
        <input id="agent-name" bind:value={draft} maxlength="80" autocomplete="off" disabled={saving} />
        <div><button type="button" disabled={saving} onclick={() => { editing = false; error = ''; }}>Cancel</button><button class="save" disabled={saving || !draft.trim()}>{saving ? 'Saving' : 'Save name'}</button></div>
        <small>Used in your agent's system prompt from the next reply.</small>
      </form>
    {:else}
      <button class="agent-name" onclick={() => { draft = name; editing = true; }} title="Rename your agent"><h2>{name}</h2><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true"><path d="m15 5 4 4M4 20l5-1L20 8a2.8 2.8 0 0 0-4-4L5 15l-1 5Z"/></svg></button>
    {/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <p class="activity" role="status"><span class="presence"></span>{activity}</p>
  </section>
  <div class="browser-slot"><BrowserPreview /></div>
  <div class="details"><span>In this conversation</span><strong>{model || 'Choose a model in chat'}</strong><p>{approvals ? 'A browser action is waiting for your decision.' : 'Watch the browser any time. Take over when you want to step in.'}</p></div>
</aside>

<style>
  .companion{width:clamp(260px,25vw,340px);flex-shrink:0;min-height:0;overflow-y:auto;border-left:1px solid var(--line);margin-left:28px;padding:10px 0 16px 28px;scrollbar-width:thin}
  .identity{text-align:center;padding:9px 0 26px}.eyebrow{font-size:9px;text-transform:uppercase;letter-spacing:.16em;color:var(--faint)}
  .avatar-stage{height:200px;position:relative;display:grid;place-items:center;isolation:isolate}.avatar-stage img{width:116px;z-index:1;animation:float 6s ease-in-out infinite;filter:drop-shadow(0 8px 20px #ffd45c18)}.orbit{position:absolute;width:164px;height:164px;border:1px solid var(--line);border-radius:50%;background:radial-gradient(circle,#ffd45c09,transparent 70%)}.shadow{position:absolute;bottom:29px;width:67px;height:8px;background:#0003;border-radius:50%;filter:blur(4px);animation:shadow 6s ease-in-out infinite}
  .working .avatar-stage img{animation:think 2.5s ease-in-out infinite}.working .orbit{border-style:dashed;animation:orbit 24s linear infinite}.attention .orbit{border-color:var(--yellow-deep)}
  .agent-name{display:inline-flex;align-items:center;gap:9px;max-width:100%;border:0;background:transparent;padding:2px 8px;color:var(--text)}h2{font-size:25px;font-weight:550;letter-spacing:-.8px;margin:0;overflow-wrap:anywhere}.agent-name svg{width:13px;height:13px;color:var(--faint);flex-shrink:0}.agent-name:hover svg{color:var(--yellow)}
  .activity{display:flex;align-items:center;justify-content:center;gap:7px;color:var(--muted);font-size:11px;margin:11px 0 0}.presence{width:5px;height:5px;background:var(--yellow);border-radius:50%;flex-shrink:0}.working .presence{animation:pulse 1.8s ease-in-out infinite}
  .rename{text-align:left;display:grid;gap:8px}.rename label{font-size:11px;color:var(--muted)}.rename input{width:100%;min-width:0;border:1px solid var(--line-strong);border-radius:8px;background:var(--paper);color:var(--text);padding:9px 10px;font-size:15px}.rename>div{display:flex;gap:6px;justify-content:flex-end}.rename button{border:1px solid var(--line);border-radius:7px;padding:6px 9px;background:transparent;color:var(--muted);font-size:11px}.rename .save{background:var(--yellow);color:#151515;border-color:var(--yellow)}.rename small{font-size:10px;color:var(--faint);line-height:1.6}.error{font-size:11px;color:var(--danger,#ef9b9b)}button:focus-visible,input:focus-visible{outline:2px solid var(--yellow);outline-offset:3px}
  .browser-slot{border-top:1px solid var(--line);padding-top:20px}.details{border-top:1px solid var(--line);margin-top:22px;padding-top:17px}.details>span{font-size:9px;text-transform:uppercase;letter-spacing:.1em;color:var(--faint)}.details strong{display:block;font-size:11px;font-weight:450;color:var(--muted);margin-top:8px;overflow-wrap:anywhere}.details p{font-size:11px;color:var(--faint);line-height:1.7;margin:12px 0 0}
  @keyframes float{0%,100%{transform:translateY(2px) rotate(-2deg)}50%{transform:translateY(-7px) rotate(2deg)}}@keyframes think{0%,100%{transform:translateY(0) rotate(-4deg)}50%{transform:translateY(-6px) rotate(4deg)}}@keyframes orbit{to{transform:rotate(360deg)}}@keyframes shadow{50%{transform:scale(.8);opacity:.6}}@keyframes pulse{50%{opacity:.3}}
  @media(max-width:1100px){.companion{width:240px;margin-left:18px;padding-left:18px}.avatar-stage{height:168px}.avatar-stage img{width:96px}.orbit{width:140px;height:140px}.shadow{bottom:22px}}
  @media(max-width:760px){.companion{width:190px;margin-left:12px;padding-left:12px}.avatar-stage{height:125px}.avatar-stage img{width:72px}.orbit{width:106px;height:106px}.shadow{bottom:14px}h2{font-size:20px}.details{display:none}.activity{font-size:10px}.identity{padding-bottom:18px}}
  @media(max-width:560px){.companion{width:auto;margin:0 0 16px;padding:0 0 12px;border-left:0;border-bottom:1px solid var(--line);display:grid;grid-template-columns:1fr 1fr;gap:16px;flex-shrink:0;max-height:270px}.identity{padding:0}.eyebrow{display:none}.avatar-stage{height:90px}.avatar-stage img{width:56px}.orbit{width:78px;height:78px}.browser-slot{border-top:0;padding-top:0}.details{display:none}}
  @media(prefers-reduced-motion:reduce){.avatar-stage img,.working .avatar-stage img,.working .orbit,.shadow,.working .presence{animation:none}}
</style>
