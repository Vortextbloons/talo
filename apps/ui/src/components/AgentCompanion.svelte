<script lang="ts">
  import { onMount, untrack } from 'svelte';
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
  const activity = $derived(browser.status.state === 'crashed' ? 'Browser needs reconnecting' : approvals ? 'Waiting on you' : chat.active?.state === 'cancelling' ? 'Stopping' : chat.worker.state === 'starting' ? 'Waking up' : browser.status.busy ? 'Exploring the web' : working ? 'Thinking it through' : chat.worker.state === 'failed' ? 'Connection needs attention' : browser.status.owner === 'paused' ? 'Browser paused' : 'Here when you need me');
  const mood = $derived.by(() => {
    if (browser.status.state === 'crashed') return 'crashed';
    if (approvals) return 'attention';
    if (chat.active?.state === 'cancelling') return 'stopping';
    if (chat.worker.state === 'starting') return 'waking';
    if (browser.status.busy) return 'browsing';
    if (chat.active?.state === 'generating') return 'thinking';
    if (chat.worker.state === 'failed') return 'failed';
    if (browser.status.owner === 'paused') return 'paused';
    return 'idle';
  });
  const runtime = $derived(chat.preview ? 'Interface preview' : chat.worker.state === 'starting' ? 'Starting the local agent' : working ? 'Working on your request' : chat.worker.state === 'failed' ? 'Check your connection in Settings' : 'Ready for your next message');
  const model = $derived(selectionLabel(chat.active?.conversation.selection ?? chat.draftSelection));

  /* --- Life-like touches: blinking, gaze, celebrations --- */
  let stageEl = $state<HTMLDivElement>();
  let blinking = $state(false);
  let gazeX = $state(0);
  let gazeY = $state(0);
  let happy = $state(false);
  let poking = $state(false);
  let happyTimer = 0;
  let pokeTimer = 0;
  const WANDER: [number, number][] = [[3.1, 0], [-3.1, 0], [0, -2.3], [2.2, -1.4], [-2.4, 1.3], [0, 0]];
  const clamp = (value: number, low: number, high: number) => Math.max(low, Math.min(high, value));

  function celebrate() {
    happy = true;
    window.clearTimeout(happyTimer);
    happyTimer = window.setTimeout(() => { happy = false; }, 1900);
  }
  function poke() {
    if (poking) return;
    poking = true;
    window.clearTimeout(pokeTimer);
    pokeTimer = window.setTimeout(() => { poking = false; }, 850);
  }

  // A finished reply deserves a little smile before the next one starts.
  let prevStatus: string | null = null;
  $effect(() => {
    const status = (chat.active?.messages ?? []).filter(message => message.role === 'assistant').at(-1)?.status ?? null;
    const previous = prevStatus;
    prevStatus = status;
    if (previous === 'generating' && status === 'completed') untrack(() => celebrate());
  });

  onMount(() => {
    if (typeof window === 'undefined' || window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    let blinkTimer = 0;
    let doubleTimer = 0;
    const doBlink = () => {
      blinking = true;
      window.setTimeout(() => {
        blinking = false;
        // Every so often a second blink sneaks in, the way eyes do.
        if (Math.random() < 0.24) doubleTimer = window.setTimeout(() => { blinking = true; window.setTimeout(() => { blinking = false; }, 150); }, 230);
      }, 170);
    };
    const scheduleBlink = () => {
      blinkTimer = window.setTimeout(() => { doBlink(); scheduleBlink(); }, 2600 + Math.random() * 3600);
    };
    scheduleBlink();
    let lastPointer = 0;
    const onPointerMove = (event: PointerEvent) => {
      const stage = stageEl;
      if (!stage) return;
      const rect = stage.getBoundingClientRect();
      lastPointer = performance.now();
      gazeX = Math.round(clamp((event.clientX - (rect.left + rect.width / 2)) / 110, -3.3, 3.3) * 10) / 10;
      gazeY = Math.round(clamp((event.clientY - (rect.top + rect.height * 0.45)) / 110, -2.5, 2.5) * 10) / 10;
    };
    window.addEventListener('pointermove', onPointerMove, { passive: true });
    const wander = window.setInterval(() => {
      if (performance.now() - lastPointer > 3500) {
        const [x, y] = WANDER[Math.floor(Math.random() * WANDER.length)];
        gazeX = x; gazeY = y;
      }
    }, 3200);
    return () => {
      window.clearTimeout(blinkTimer); window.clearTimeout(doubleTimer); window.clearTimeout(pokeTimer); window.clearTimeout(happyTimer);
      clearInterval(wander);
      window.removeEventListener('pointermove', onPointerMove);
    };
  });

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

<aside class="companion mood-{mood}" class:happy class:poking aria-label="Your agent and browser">
  <section class="identity">
    <span class="eyebrow">Your companion</span>
    <div class="avatar-wrap">
      <div class="avatar-stage" class:blinking bind:this={stageEl} style="--gx:{gazeX}px;--gy:{gazeY}px" aria-hidden="true">
        <span class="aura"></span>
        <span class="orbit"></span>
        <span class="comet"><i></i></span>
        <span class="spark s1"></span><span class="spark s2"></span><span class="spark s3"></span>
        <svg class="agent-logo" viewBox="0 0 96 80" aria-hidden="true">
          <rect x="2" y="2" width="92" height="76" rx="32" fill="#ffd45c"/>
          <g class="gaze">
            <g class="eyes" fill="#151515">
              <rect x="28" y="25" width="10" height="28" rx="5"/>
              <rect x="58" y="25" width="10" height="28" rx="5"/>
              <circle cx="31" cy="31" r="1.6" fill="#fff"/>
              <circle cx="61" cy="31" r="1.6" fill="#fff"/>
            </g>
          </g>
          <path class="mouth" d="M41 55q7 6 14 0" fill="none" stroke="#151515" stroke-width="2.6" stroke-linecap="round"/>
        </svg>
        <span class="shadow"></span>
        {#each [0, 60, 120, 180, 240, 300] as angle (angle)}<span class="burst" style="--a:{angle}deg"></span>{/each}
      </div>
      <button type="button" class="poke" aria-label={`Poke ${name}`} onclick={poke}></button>
    </div>
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
    <p class="activity" role="status"><span class="presence"></span>{#key activity}<span class="activity-in">{activity}</span>{/key}</p><span class="runtime">{runtime}</span>
  </section>
  <div class="browser-slot"><BrowserPreview /></div>
  <div class="details" class:waiting={approvals > 0}><span>In this conversation</span><strong>{model || 'Choose a model in chat'}</strong><p>{approvals ? 'An action is waiting for your decision.' : 'Tabs stay with this chat. Website logins are shared across chats.'}</p></div>
</aside>

<style>
  .companion{width:clamp(260px,25vw,340px);flex-shrink:0;min-height:0;overflow-y:auto;border-left:1px solid var(--line);margin-left:28px;padding:10px 0 16px 28px;scrollbar-width:thin}
  .identity{text-align:center;padding:9px 0 26px}.eyebrow{font-size:9px;text-transform:uppercase;letter-spacing:.16em;color:var(--faint)}
  .avatar-wrap{position:relative;height:200px;display:grid;place-items:center}
  .avatar-stage{position:absolute;inset:0;display:grid;place-items:center;isolation:isolate}
  .agent-logo{width:116px;z-index:1;animation:float 6s ease-in-out infinite;filter:drop-shadow(0 8px 20px #ffd45c18);transform-origin:50% 88%;transition:filter .3s ease}
  .avatar-wrap:hover .agent-logo{animation:float-squish 1.9s ease-in-out infinite}
  .mood-waking .agent-logo{animation:doze 4.2s ease-in-out infinite}
  .mood-browsing .agent-logo{animation:lean 2.4s ease-in-out infinite;filter:drop-shadow(0 10px 24px #ffd45c2e)}
  .mood-thinking .agent-logo{animation:think 2.5s ease-in-out infinite;filter:drop-shadow(0 10px 24px #ffd45c2e)}
  .mood-stopping .agent-logo{animation:shake .55s ease-in-out 3}
  .mood-attention .agent-logo{animation:perk .8s cubic-bezier(.34,1.56,.64,1) 1}
  .mood-crashed .agent-logo,.mood-failed .agent-logo{animation:droop .7s ease forwards}
  .happy .agent-logo{animation:joy .75s ease-in-out 2}
  .poking .agent-logo{animation:poke .8s cubic-bezier(.34,1.56,.64,1) 1}
  .orbit{position:absolute;width:164px;height:164px;border:1px solid var(--line);border-radius:50%;background:radial-gradient(circle,#ffd45c09,transparent 70%)}
  .mood-thinking .orbit,.mood-browsing .orbit{border-style:dashed;animation:orbit 14s linear infinite}
  .mood-attention .orbit{border-color:var(--yellow-deep);animation:ring-pulse 1.6s ease-in-out infinite}
  .aura{position:absolute;width:190px;height:190px;border-radius:50%;background:radial-gradient(circle,#ffd45c14,transparent 65%);opacity:.35;transition:opacity .4s ease}
  .mood-thinking .aura,.mood-browsing .aura,.mood-attention .aura{opacity:.9;animation:aura-pulse 2.4s ease-in-out infinite}
  .comet{position:absolute;width:164px;height:164px;animation:orbit 46s linear infinite;opacity:.5;pointer-events:none}
  .comet i{position:absolute;top:-3px;left:50%;margin-left:-3px;width:6px;height:6px;border-radius:50%;background:var(--yellow);box-shadow:0 0 9px 2px #ffd45c59}
  .mood-thinking .comet,.mood-browsing .comet,.mood-attention .comet{animation-duration:8s;opacity:1}
  .spark{position:absolute;width:5px;height:5px;border-radius:50%;background:var(--yellow);box-shadow:0 0 8px 2px #ffd45c40;opacity:0;pointer-events:none}
  .spark.s1{top:16%;left:15%}.spark.s2{top:26%;right:11%}.spark.s3{bottom:24%;left:20%}
  .mood-thinking .spark,.mood-browsing .spark{animation:twinkle 1.7s ease-in-out infinite}
  .spark.s2{animation-delay:.5s}.spark.s3{animation-delay:1s}
  .burst{position:absolute;left:50%;top:44%;width:5px;height:5px;margin:-2.5px;border-radius:50%;background:var(--yellow);box-shadow:0 0 7px 1px #ffd45c4d;opacity:0;pointer-events:none;z-index:2}
  .poking .burst{animation:burst .8s cubic-bezier(.2,.6,.35,1) 1}
  .shadow{position:absolute;bottom:29px;width:67px;height:8px;background:#0003;border-radius:50%;filter:blur(4px);animation:shadow 6s ease-in-out infinite}
  .poke{position:absolute;left:50%;top:50%;width:150px;height:150px;transform:translate(-50%,-50%);z-index:2;background:transparent;border:0;padding:0;border-radius:50%;cursor:pointer}
  .poke:focus-visible{outline:2px solid var(--yellow);outline-offset:4px}
  .gaze{transform:translate(var(--gx,0px),var(--gy,0px));transition:transform .45s cubic-bezier(.25,.7,.3,1)}
  .eyes{transform-origin:48px 39px;transition:transform .3s ease}
  .mood-thinking .eyes{animation:look 2.2s ease-in-out infinite}
  .mood-browsing .eyes{animation:dart 1.1s ease-in-out infinite}
  .mood-waking .eyes{transform:scaleY(.42)}
  .mood-stopping .eyes,.happy .eyes{transform:scaleY(.5)}
  .mood-attention .eyes{transform:scaleY(1.1) scaleX(.92)}
  .mood-crashed .eyes,.mood-failed .eyes{transform:scaleY(.3) translateY(2px)}
  .blinking .eyes{animation:blink-fast .18s ease-in-out 1 !important}
  .mouth{opacity:0;transform:scale(.6);transform-origin:48px 58px;transition:opacity .18s ease,transform .22s cubic-bezier(.34,1.56,.64,1)}
  .happy .mouth,.poking .mouth{opacity:1;transform:scale(1)}
  .agent-name{display:inline-flex;align-items:center;gap:9px;max-width:100%;border:0;background:transparent;padding:2px 8px;color:var(--text)}h2{font-size:25px;font-weight:550;letter-spacing:-.8px;margin:0;overflow-wrap:anywhere}.agent-name svg{width:13px;height:13px;color:var(--faint);flex-shrink:0}.agent-name:hover svg{color:var(--yellow)}
  .runtime{display:block;font-size:10px;color:var(--faint);margin-top:7px;line-height:1.6}
  .activity{display:flex;align-items:center;justify-content:center;gap:7px;color:var(--muted);font-size:11px;margin:11px 0 0}.presence{width:5px;height:5px;background:var(--yellow);border-radius:50%;flex-shrink:0}
  .mood-thinking .presence,.mood-browsing .presence{animation:pulse 1.8s ease-in-out infinite}
  .mood-attention .presence{animation:pulse 1.1s ease-in-out infinite}
  .mood-crashed .presence,.mood-failed .presence{background:#ef9b9b}
  .activity-in{display:inline-block;animation:rise .32s cubic-bezier(.2,.7,.3,1)}
  .rename{text-align:left;display:grid;gap:8px}.rename label{font-size:11px;color:var(--muted)}.rename input{width:100%;min-width:0;border:1px solid var(--line-strong);border-radius:8px;background:var(--paper);color:var(--text);padding:9px 10px;font-size:15px}.rename>div{display:flex;gap:6px;justify-content:flex-end}.rename button{border:1px solid var(--line);border-radius:7px;padding:6px 9px;background:transparent;color:var(--muted);font-size:11px}.rename .save{background:var(--yellow);color:#151515;border-color:var(--yellow)}.rename small{font-size:10px;color:var(--faint);line-height:1.6}.error{font-size:11px;color:var(--danger,#ef9b9b)}button:focus-visible,input:focus-visible{outline:2px solid var(--yellow);outline-offset:3px}
  .browser-slot{border-top:1px solid var(--line);padding-top:20px}.details{border-top:1px solid var(--line);margin-top:22px;padding-top:17px}.details>span{font-size:9px;text-transform:uppercase;letter-spacing:.1em;color:var(--faint)}.details strong{display:block;font-size:11px;font-weight:450;color:var(--muted);margin-top:8px;overflow-wrap:anywhere}.details p{font-size:11px;color:var(--faint);line-height:1.7;margin:12px 0 0}
  .details.waiting{animation:details-glow 1.8s ease-in-out infinite}
  @keyframes float{0%,100%{transform:translateY(2px) rotate(-2deg)}50%{transform:translateY(-7px) rotate(2deg)}}
  @keyframes float-squish{0%,100%{transform:translateY(1px) scale(1,1)}25%{transform:translateY(-6px) scale(.97,1.04)}55%{transform:translateY(-2px) scale(1.03,.97)}}
  @keyframes think{0%,100%{transform:translateY(0) rotate(-4deg)}50%{transform:translateY(-6px) rotate(4deg)}}
  @keyframes lean{0%,100%{transform:translateY(1px) rotate(-6deg) scale(1.01)}50%{transform:translateY(-6px) rotate(5deg)}}
  @keyframes doze{0%,100%{transform:translateY(3px) rotate(-1deg)}50%{transform:translateY(-3px) rotate(1deg)}}
  @keyframes shake{0%,100%{transform:translateX(0)}20%{transform:translateX(-3px) rotate(-2deg)}60%{transform:translateX(3px) rotate(2deg)}}
  @keyframes perk{0%,100%{transform:translateY(0) scale(1)}35%{transform:translateY(-9px) scale(.98,1.05)}70%{transform:translateY(0) scale(1.04,.96)}}
  @keyframes joy{0%,100%{transform:rotate(0)}25%{transform:rotate(-7deg) translateY(-3px)}75%{transform:rotate(7deg) translateY(-3px)}}
  @keyframes poke{0%{transform:scale(1,1)}18%{transform:scale(1.14,.8) translateY(4px)}45%{transform:scale(.92,1.12) translateY(-10px)}70%{transform:scale(1.05,.94) translateY(-2px)}100%{transform:scale(1,1)}}
  @keyframes droop{from{transform:rotate(0)}to{transform:rotate(-5deg) translateY(3px)}}
  @keyframes orbit{to{transform:rotate(360deg)}}
  @keyframes ring-pulse{50%{transform:scale(1.05)}}
  @keyframes aura-pulse{50%{transform:scale(1.08);opacity:.55}}
  @keyframes twinkle{0%,100%{opacity:0;transform:scale(.4) rotate(0)}50%{opacity:1;transform:scale(1) rotate(45deg)}}
  @keyframes burst{0%{opacity:0;transform:rotate(var(--a)) translateY(0) scale(.4)}15%{opacity:1}100%{opacity:0;transform:rotate(var(--a)) translateY(-62px) scale(1)}}
  @keyframes shadow{50%{transform:scale(.8);opacity:.6}}
  @keyframes pulse{50%{opacity:.3}}
  @keyframes rise{from{opacity:0;transform:translateY(5px)}}
  @keyframes blink-fast{0%,100%{transform:scaleY(1)}50%{transform:scaleY(.08)}}
  @keyframes look{0%,100%{transform:translateX(-3px)}50%{transform:translateX(3px)}}
  @keyframes dart{0%,100%{transform:translateX(-4.5px)}50%{transform:translateX(4.5px)}}
  @keyframes details-glow{50%{border-color:#ffd45c33}}
  @media(max-width:1100px){.companion{width:240px;margin-left:18px;padding-left:18px}.avatar-wrap{height:168px}.agent-logo{width:96px}.orbit,.comet{width:140px;height:140px}.aura{width:165px;height:165px}.shadow{bottom:22px}.poke{width:130px;height:130px}}
  @media(max-width:760px){.companion{width:190px;margin-left:12px;padding-left:12px}.avatar-wrap{height:125px}.agent-logo{width:72px}.orbit,.comet{width:106px;height:106px}.aura{width:125px;height:125px}.shadow{bottom:14px}.poke{width:100px;height:100px}h2{font-size:20px}.details{display:none}.activity{font-size:10px}.identity{padding-bottom:18px}}
  @media(max-width:560px){.companion{width:auto;margin:0 0 16px;padding:0 0 12px;border-left:0;border-bottom:1px solid var(--line);display:grid;grid-template-columns:1fr 1fr;gap:16px;flex-shrink:0;max-height:270px}.identity{padding:0}.eyebrow{display:none}.avatar-wrap{height:90px}.agent-logo{width:56px}.orbit,.comet{width:78px;height:78px}.aura{width:92px;height:92px}.poke{width:76px;height:76px}.browser-slot{border-top:0;padding-top:0}.details{display:none}}
  @media(prefers-reduced-motion:reduce){
    .agent-logo,.avatar-wrap:hover .agent-logo,.eyes,.gaze,.orbit,.comet,.aura,.spark,.burst,.shadow,.presence,.activity-in,.details,.happy .mouth,.poking .mouth{animation:none !important;transition:none !important}
    .mood-crashed .agent-logo,.mood-failed .agent-logo{animation:none !important;transform:none !important}
  }
</style>