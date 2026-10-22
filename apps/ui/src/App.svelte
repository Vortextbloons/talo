<script lang="ts">
  import BrowserPanel from './components/BrowserPanel.svelte';
  import { initBrowser, closeBrowser } from './lib/browser.svelte';
  import { onMount } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import mascot from '../../../packages/design/mascot.svg';
  import NavIcon from './NavIcon.svelte';
  import { library, refreshLibrary, mutateLibrary } from './lib/library.svelte';
  import LibraryPanel from './components/LibraryPanel.svelte';
  import ChatView from './components/ChatView.svelte';
  import SettingsPanel from './components/SettingsPanel.svelte';
  import ToolApprovals from './components/ToolApprovals.svelte';
  import TasksPanel from './components/TasksPanel.svelte';
  import InboxPanel from './components/InboxPanel.svelte';
  import SkillsPanel from './components/SkillsPanel.svelte';
  import { agent, initAgent } from './lib/agent.svelte';
  import { chat, disposeChat, initChat, createConversation, routing } from './lib/chat.svelte';
  import { checkForUpdates } from './lib/updates.svelte';
  import { CUSTOM_BASE_URL_EXAMPLE, DEFAULT_PREFERENCE, PROVIDERS, parseProviderPreference, providerDescriptor } from '../../../packages/contracts/src/index';
  import type { Onboarding, RuntimeStatus, ServiceId } from '../../../packages/contracts/src/index';
  const pages = ['Chat', 'Inbox', 'Workspaces', 'Skills', 'Tasks', 'Settings'] as const;
  type Page = typeof pages[number];
  let page = $state<Page>('Chat');
  /** The rail shows a count only when something genuinely waits on a person. */
  const inboxCount = $derived(agent.asks.length + agent.assignments.filter(item=>item.status==='open').length + agent.drafts.length + agent.skillProposals.length);
  let taskFocus = $state('');
  function navigatePage(next: Page) { if (page !== next) closeBrowser(); page = next; }
  /* The rail keeps its icons on screen and opens over the page on hover, so the layout never jumps. */
  const PIN_KEY = 'talo.nav.pinned';
  function initialPinned(): boolean {
    try { return localStorage.getItem(PIN_KEY) === '1'; } catch { return false; }
  }
  let railPinned = $state(initialPinned());
  function toggleRail(): void {
    railPinned = !railPinned;
    try { localStorage.setItem(PIN_KEY, railPinned ? '1' : '0'); } catch { /* storage can be unavailable */ }
  }
  let settings = $state<Onboarding>({name:'',personality:'Thoughtful and direct',...DEFAULT_PREFERENCE,memory:'workspace',showReasoning:false,complete:false});
  let provider = $derived(providerDescriptor(settings.provider));
  let loaded = $state(false);
  let editing = $state(false);
  let step = $state(0);
  let status = $state<RuntimeStatus | null>(null);
  let error = $state('');
  let busy = $state(false);
  let pending = $state<{id:string; action:string; resource:string; run:(id:string)=>Promise<void>} | null>(null);
  const descriptions:Record<Page,string> = {
    Chat:'A little space to think things through.',Inbox:'What Talo has asked, proposed or left for you.',Workspaces:'One Talo, with conversations and context organized by project.',Skills:'Ways of working Talo saved once they worked.',Tasks:'A home for the things you want to come back to.',Settings:'Make yourself at home.'
  };
  async function refresh() { if(isTauri()) status = await invoke<RuntimeStatus>('runtime_status'); }
  onMount(() => {
    let active=true;
    let unlisten:(()=>void)|undefined;
    let stopAutomation:(()=>void)|undefined;
    const notification=async()=>{if(!isTauri())return;const run=await invoke<string>('automation_action',{operation:'notification'});if(run&&active){taskFocus=run;navigatePage('Tasks');}};
    if(isTauri()){
      void notification().catch(()=>{});
      void listen('automation-open',()=>void notification().catch(()=>{})).then(stop=>{if(active)stopAutomation=stop;else stop();});
    }
    void initChat();
    const stopAgent = initAgent();
    const stopBrowser = initBrowser();
    (async()=>{try { if(isTauri()){const saved=await invoke<Onboarding|null>('load_settings');if(saved && active){settings={...settings,...saved,...parseProviderPreference(saved)};routing.mode=saved.routing??'auto';routing.tier=saved.performance??'recommended';}await refresh();} }catch(e){error=String(e);}finally{if(active)loaded=true;}})();
    // The host pushes service transitions; polling would keep the hidden webview awake.
    if(isTauri())listen<RuntimeStatus>('runtime-status',event=>{if(active)status=event.payload;}).then(stop=>{if(active)unlisten=stop;else stop();});
    // Ask the release feed once the window has settled. A failed check stays silent; the
    // Settings row shows the detail when someone looks.
    const updateCheck = setTimeout(() => { if (active) void checkForUpdates({ quiet: true }); }, 4000);
    const wake=()=>{if(active)refresh().catch(()=>{});};
    window.addEventListener('focus',wake);
    return ()=>{active=false;unlisten?.();stopAutomation?.();window.removeEventListener('focus',wake);disposeChat();stopAgent();stopBrowser();};
  });
  async function saveSettings(next: Onboarding): Promise<void> {
    if(isTauri()) await invoke('save_settings', { settings: { ...next, complete: true } });
    settings = { ...next, complete: true };
    routing.mode = next.routing ?? 'auto';
    routing.tier = next.performance ?? 'recommended';
  }
  async function save() { try {busy=true;await saveSettings(settings);editing=false;void refreshLibrary();}catch(e){error=String(e);}finally{busy=false;} }
  /** One setting changes at a time; the row that changed reports its own failure. */
  async function persist(patch: Partial<Onboarding>): Promise<void> {
    const next = { ...settings, ...patch };
    try { await saveSettings(next); }
    catch(e) { error = String(e); }
  }
  async function permission(action:string,resource:string,run:(id:string)=>Promise<void>) {
    if(!isTauri()){error='Open the desktop app to manage local services.';return;}
    try{const id=await invoke<string>('request_permission',{action,resource});pending={id,action,resource,run};}catch(e){error=String(e);}
  }
  async function resolve(allowed:boolean){const p=pending;if(!p)return;pending=null;try{busy=true;await invoke('resolve_permission',{id:p.id,allowed});if(allowed)await p.run(p.id);}catch(e){error=String(e);}finally{busy=false;}}
  async function control(service:ServiceId,start:boolean,grant?:string){
    if(!isTauri()){error='Open the desktop app to manage local services.';return;}
    try{busy=true;status=await invoke<RuntimeStatus>('service_control',{service,start,grant:grant??null});}catch(e){error=String(e);}finally{busy=false;}
  }
  function browserStart(){return control('browser',true);}
  async function setAutostart(enabled:boolean): Promise<void> {
    await permission('autostart.change', enabled ? 'enable' : 'disable', async grant => { await invoke('set_autostart',{enabled,grant}); });
  }
  async function setMemory(enabled:boolean): Promise<void> {
    if(!isTauri()){error='Open the desktop app to change memory.';return;}
    await mutateLibrary('memory_enable',{enabled});
  }
  async function renameAgent(name: string) {
    if (!isTauri()) throw new Error('Open the desktop app to save your agent name.');
    await persist({ agentName: name } as Partial<Onboarding>);
  }
  /* Setup may be skipped entirely; saved preferences stay untouched and onboarding never blocks first use. */
  async function skipSetup(){
    if(busy)return;
    try{await saveSettings(settings);}catch(e){error=String(e);return;}
    editing=false;void refreshLibrary();
  }
  /* Keyboard map: Ctrl+1..5 switch pages, Ctrl+N starts a conversation, Ctrl+B keeps the rail open,
     Ctrl+, opens Settings. */
  function onKeydown(event:KeyboardEvent):void{
    if(!loaded||!settings.complete||editing)return;
    if(!event.ctrlKey||event.altKey||event.shiftKey||event.metaKey)return;
    if(event.key>='1'&&event.key<='6'){
      const item=pages[Number(event.key)-1];
      if(item){event.preventDefault();navigatePage(item);}
      return;
    }
    if(event.key.toLowerCase()==='n'){
      event.preventDefault();
      if(page==='Chat'&&!busy&&!chat.creating&&!chat.sending&&!chat.preview&&chat.active?.state!=='generating'&&chat.active?.state!=='cancelling')void createConversation();
      return;
    }
    if(event.key.toLowerCase()==='b'){event.preventDefault();toggleRail();return;}
    if(event.key===','){event.preventDefault();navigatePage('Settings');}
  }
</script>

<div class="shell" class:rail-pinned={railPinned} class:ai-working={chat.active?.state==='generating'||chat.active?.state==='cancelling'}>
  <aside class="rail" aria-label="Main navigation">
    <div class="rail-inner">
      <div class="rail-top">
        <button class="brand" onclick={()=>navigatePage('Chat')} title="Chat" aria-label="Talo — go to Chat">
          <img src={mascot} alt=""/>
          <span class="wordmark">talo<span class="dot">.</span></span>
        </button>
        <button class="pin" onclick={toggleRail} aria-pressed={railPinned} title={railPinned?'Let the rail close again':'Keep the rail open'} aria-label={railPinned?'Let the navigation rail close again':'Keep the navigation rail open'}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><path d="M12 17v4"/><path d="M9 3h6l-1 6 3 3H7l3-3-1-6Z"/></svg>
        </button>
      </div>
      <nav>{#each pages as item (item)}
        <button class:selected={page===item} class:thinking={item==='Chat' && (chat.active?.state==='generating'||chat.active?.state==='cancelling')} onclick={()=>navigatePage(item)} title={item === 'Workspaces' ? 'Projects' : item} aria-current={page===item?'page':undefined}>
          <span class="nav-icon"><NavIcon name={item} working={item==='Chat' && (chat.active?.state==='generating'||chat.active?.state==='cancelling')} /></span>
          <span class="nav-label">{item === 'Workspaces' ? 'Projects' : item}</span>
          {#if item==='Inbox' && inboxCount}<span class="nav-badge" aria-label={`${inboxCount} waiting`}>{inboxCount}</span>{/if}
        </button>
      {/each}</nav>
      <div class="rail-foot"><span class="presence"></span><span class="foot-label">Talo is here</span></div>
    </div>
  </aside>
  <main>
    <header><span class="crumbs">{page === 'Workspaces' ? 'Projects' : page}</span></header>
    {#if error}<div class="error" role="alert">{error}<button aria-label="Dismiss error" onclick={()=>error=''}>×</button></div>{/if}
    <div class="content" class:chat-mode={page==='Chat'}>
      {#if page!=='Chat'}<div class="page-heading"><h1>{page === 'Workspaces' ? 'Projects' : page}</h1><p>{descriptions[page]}</p></div>{/if}
      <BrowserPanel />
      {#if page==='Inbox'}
        <InboxPanel onOpenChat={()=>navigatePage('Chat')} />
      {:else if page==='Chat'}
        <ChatView onOpenSettings={()=>{navigatePage('Settings');}} name={settings.name} tagline={descriptions.Chat} showReasoning={settings.showReasoning === true} agentName={settings.agentName || 'Talo'} onRenameAgent={renameAgent} />
      {:else if page==='Workspaces'}
        <LibraryPanel mode={page} />
      {:else if page==='Settings'}
        <SettingsPanel
          {settings}
          {status}
          {busy}
          onPersist={persist}
          onService={control}
          onBrowserStart={browserStart}
          onAutostart={setAutostart}
          onMemory={setMemory}
          onOpenChat={()=>navigatePage('Chat')}
          onRerunSetup={()=>{editing=true;step=0;}}
        />
      {:else if page==='Skills'}
        <SkillsPanel />
      {:else if page==='Tasks'}
        <TasksPanel focusRun={taskFocus} onOpenChat={()=>navigatePage('Chat')} />
      {:else}
        <section class="empty"><span class="empty-badge"><span class="empty-icon">&#9745;</span></span><h2>A clear slate.</h2><p>Your scheduled tasks will live here. Automatic execution is not enabled.</p><span class="coming">Foundation in place</span></section>
      {/if}
      {#if page!=='Chat'}<footer><span>Made for your everyday.</span><span>Talo / 0.1.0</span></footer>{/if}
    </div>
  </main>
</div>
<svelte:window onkeydown={onKeydown} />
{#if loaded && (!settings.complete || editing)}
  <div class="overlay"><dialog open class="onboarding" aria-modal="true" aria-labelledby="setup-title"><img src={mascot} alt="Talo"/><div class="steps">Getting acquainted · {step+1} of 2</div><h2 id="setup-title">{step===0?'A little introduction.':'Choose your intelligence.'}</h2>
    {#if step===0}<label>Your name <span>(optional)</span><input bind:value={settings.name} maxlength="80" placeholder="What should Talo call you?"/></label><label>How it should sound<select bind:value={settings.personality}><option>Thoughtful and direct</option><option>Warm and encouraging</option><option>Brief and practical</option></select></label>
    {:else}<label>Starting provider<select bind:value={settings.provider}>{#each PROVIDERS as item (item)}<option value={item.id}>{item.name}</option>{/each}</select></label>{#if settings.provider==='custom'}<label>Base URL<input bind:value={settings.baseUrl} maxlength="2048" placeholder={CUSTOM_BASE_URL_EXAMPLE}/></label><label>Model ID<input bind:value={settings.model} maxlength="200" placeholder="llama3.3"/></label><label class="check"><input type="checkbox" bind:checked={settings.reasoning}/> <span>This endpoint supports reasoning</span></label><p class="setup-note">Talo writes this endpoint to Pi's models.json. Reasoning adds Pi's thinking levels to the model. Local servers need no key.</p>{:else}<p class="setup-note">{provider.tagline} Pick a starting provider now. You will connect its key in Settings → Models before your first message.</p>{/if}{/if}
    <div class="setup-actions">{#if step>0}<button onclick={()=>step--}>Back</button>{:else}<button onclick={()=>{void skipSetup();}}>Skip for now</button>{/if}{#if step<1}<button class="primary" onclick={()=>step++}>Continue</button>{:else}<button class="primary" disabled={busy} onclick={save}>Make yourself at home</button>{/if}</div>
  </dialog></div>
{/if}
{#if pending}<div class="overlay"><dialog open class="onboarding" aria-modal="true" aria-labelledby="permission-title"><h2 id="permission-title">Allow this action?</h2><p>{pending.action==='browser.start'?'Open Chromium with Talo’s dedicated persistent profile?':'Change whether Talo starts when you sign in to Windows?'}</p><p class="detail">{pending.resource} · This action only</p><div class="setup-actions"><button onclick={()=>resolve(false)}>Deny</button><button class="primary" onclick={()=>resolve(true)}>Allow once</button></div></dialog></div>{/if}

<ToolApprovals />
<style>
  /* The window is the page: a fixed app frame, each region scrolls itself. The rail floats
     above it, so opening it never reflows the conversation underneath. */
  .shell{position:relative;display:flex;height:100vh;height:100dvh;overflow:hidden;background:var(--bg);--rail:64px;--rail-open:232px}
  .shell.rail-pinned{--rail:var(--rail-open)}
  .rail{position:absolute;inset:0 auto 0 0;z-index:4;width:var(--rail);background:var(--sidebar);border-right:1px solid var(--line);overflow:hidden;
    transition:width .18s cubic-bezier(.2,.7,.3,1),box-shadow .18s ease}
  .rail:hover,.rail:focus-within{width:var(--rail-open);box-shadow:22px 0 46px rgba(0,0,0,.5)}
  .rail-inner{width:var(--rail-open);height:100%;padding:18px 0 14px;display:flex;flex-direction:column;min-height:0}
  .rail-top{display:flex;align-items:center;height:34px;padding:0 18px}
  .brand{display:flex;align-items:center;gap:11px;background:transparent;border:0;padding:0;color:var(--text);min-width:0;cursor:pointer}
  .brand img{width:27px;flex-shrink:0;filter:drop-shadow(0 0 16px rgba(255,212,92,.16))}
  .wordmark{font-size:21px;font-weight:640;letter-spacing:-1px;white-space:nowrap;opacity:0;transform:translateX(-4px);
    transition:opacity .14s ease .03s,transform .14s ease .03s}
  .dot{color:var(--yellow)}
  .rail:hover .wordmark,.rail:focus-within .wordmark,.rail-pinned .wordmark{opacity:1;transform:none}
  .pin{margin-left:auto;display:grid;place-items:center;width:26px;height:26px;flex-shrink:0;padding:0;border:1px solid transparent;border-radius:7px;
    background:transparent;color:var(--faint);opacity:0;transition:opacity .14s ease,color .15s ease,border-color .15s ease,background .15s ease}
  .pin svg{width:14px;height:14px}
  .rail:hover .pin,.rail:focus-within .pin,.rail-pinned .pin{opacity:1}
  .pin:hover{color:var(--text);border-color:var(--line-strong);background:var(--soft)}
  .pin[aria-pressed="true"]{color:var(--yellow)}
  nav{display:grid;gap:2px;margin-top:20px}
  nav button{position:relative;display:flex;align-items:center;gap:14px;height:40px;padding:0 18px;border:0;background:transparent;
    color:var(--muted);font-size:13.5px;text-align:left;white-space:nowrap;transition:color .13s ease,background .13s ease}
  nav button:hover{background:#151513;color:var(--text)}
  nav button.selected{background:#191916;color:var(--text)}
  /* The active page is a lit edge, not a heavy pill. */
  nav button.selected::before{content:"";position:absolute;left:0;top:9px;bottom:9px;width:2px;border-radius:0 2px 2px 0;background:var(--yellow)}
  nav button.thinking{color:var(--yellow)}
  .nav-icon{--icon-opacity:.7;width:20px;height:20px;display:grid;place-items:center;flex-shrink:0}
  nav button:hover .nav-icon{--icon-opacity:.9}
  nav button.selected .nav-icon{--icon-opacity:1}
  .nav-label{opacity:0;transition:opacity .13s ease}
  .nav-badge{position:absolute;right:12px;min-width:16px;height:16px;padding:0 4px;border-radius:999px;background:var(--yellow);color:var(--black);
    font-size:10px;font-weight:650;display:grid;place-items:center;line-height:1}
  .rail:not(:hover):not(:focus-within):not(.rail-pinned) .nav-badge{right:6px}
  .rail:hover .nav-label,.rail:focus-within .nav-label,.rail-pinned .nav-label{opacity:1}
  .rail-foot{margin-top:auto;display:flex;align-items:center;gap:9px;padding:0 18px;height:30px;white-space:nowrap}
  .presence{display:inline-block;width:6px;height:6px;background:var(--yellow-deep);border-radius:50%;flex-shrink:0}
  .foot-label{font-size:11.5px;color:var(--faint);opacity:0;transition:opacity .13s ease}
  .rail:hover .foot-label,.rail:focus-within .foot-label,.rail-pinned .foot-label{opacity:1}
  .ai-working .brand img{animation:brand-bob 1.7s ease-in-out infinite;filter:drop-shadow(0 0 22px rgba(255,212,92,.34))}
  main{flex:1;min-width:0;display:flex;flex-direction:column;margin-left:var(--rail);transition:margin-left .18s cubic-bezier(.2,.7,.3,1)}
  header{height:50px;flex-shrink:0;border-bottom:1px solid var(--line);padding:0 44px;display:flex;align-items:center}
  .crumbs{font-size:10.5px;letter-spacing:.12em;text-transform:uppercase;color:var(--muted)}
  .error{margin:12px 28px 0;padding:12px;background:var(--error-bg);border:1px solid var(--error-line);color:var(--error-text);border-radius:10px;font-size:12px;display:flex;justify-content:space-between;align-items:center}
  .error button{padding:2px 8px;background:transparent;border:0}
  .content{flex:1;min-height:0;display:flex;flex-direction:column;padding:30px 44px 26px;overflow-y:auto;scrollbar-gutter:stable}
  .content.chat-mode{overflow:hidden;padding:14px 24px 20px 18px}
  .page-heading h1{font-size:34px;letter-spacing:-1.3px;font-weight:560;margin:0 0 9px}
  .page-heading p,.empty p{color:var(--muted);font-size:14px;line-height:1.7;margin:0}
  .page-heading p{max-width:76ch}
  .empty{margin:auto;padding:40px 20px;text-align:center;max-width:520px}
  .empty p{margin:0 0 24px}
  .empty-badge{display:grid;place-items:center;width:76px;height:76px;margin:0 auto 28px;border:1px solid var(--line-strong);border-radius:50%;background:var(--paper);box-shadow:0 0 64px 6px rgba(255,212,92,.09),inset 0 0 22px rgba(255,212,92,.05)}
  .empty-icon{font-size:28px;color:var(--yellow-deep)}
  .empty h2{font-size:23px;letter-spacing:-.7px;font-weight:550;margin:0 0 12px}
  .coming{display:inline-block;font-size:11px;color:var(--muted);background:var(--soft);border:1px solid var(--line);padding:7px 12px;border-radius:20px}
  footer{margin-top:auto;padding-top:28px;display:flex;justify-content:space-between;font-size:10px;color:var(--faint)}
  button{border:1px solid var(--line-strong);background:var(--soft);color:var(--text);padding:9px 14px;border-radius:8px;font-size:12px}
  button:hover:not(:disabled){background:#2c2c27;border-color:#4a4a41}
  button.primary{background:var(--yellow);border-color:var(--yellow);color:var(--black);font-weight:600;padding:11px 18px}
  button.primary:hover:not(:disabled){background:#ffdd7f;border-color:#ffdd7f}
  .overlay{position:fixed;inset:0;background:var(--overlay);backdrop-filter:blur(5px);display:grid;place-items:center;padding:24px;z-index:5}
  .onboarding{position:relative;margin:auto;border:1px solid var(--line);color:var(--text);background:var(--paper);border-radius:24px;padding:36px;width:440px;max-width:100%;box-shadow:0 30px 90px rgba(0,0,0,.65)}
  .onboarding>img{width:55px;margin-bottom:20px;filter:drop-shadow(0 0 26px rgba(255,212,92,.2))}
  .steps{font-size:11px;color:var(--muted);margin-bottom:14px}
  .onboarding h2{font-size:26px;letter-spacing:-.7px;margin-bottom:24px}
  label{display:block;font-size:13px;font-weight:550;margin:18px 0}
  label span{font-weight:400;color:var(--muted)}
  input,select{display:block;width:100%;padding:12px;border:1px solid var(--line-strong);border-radius:8px;margin-top:9px;background:var(--soft);color:var(--text)}
  input:hover,select:hover{border-color:#4a4a41}
  .setup-note{font-size:12px;margin-top:20px}
  .setup-actions{display:flex;justify-content:space-between;margin-top:30px;gap:12px}
  label.check{display:flex;align-items:center;gap:10px;font-weight:400}
  label.check input{width:auto;margin:0;accent-color:var(--yellow)}
  ::-webkit-scrollbar{width:10px;height:10px}
  ::-webkit-scrollbar-track{background:transparent}
  ::-webkit-scrollbar-thumb{background:var(--line-strong);border-radius:8px;border:2px solid var(--bg)}
  ::-webkit-scrollbar-thumb:hover{background:#4f4f46}
  @media(max-width:900px){
    .shell{--rail-open:212px}
    header{padding:0 24px}
    .error{margin:12px 22px 0}
    .content{padding:24px 24px 20px}
    .content.chat-mode{padding:12px 20px 16px}
  }
  @media(max-width:640px){
    .shell{--rail:56px;--rail-open:208px}
    nav button{padding:0 16px}
    .rail-top{padding:0 15px}
    header{padding:0 18px}
    .error{margin:10px 18px 0}
    .content{padding:18px 18px 16px}
    .content.chat-mode{padding:10px 14px 14px}
    .page-heading h1{font-size:26px}
  }
  @media(prefers-reduced-motion:reduce){
    .rail,main,.wordmark,.nav-label,.foot-label,.pin{transition:none}
    .ai-working .brand img{animation:none}
  }
  @keyframes brand-bob{0%,100%{transform:translateY(0) rotate(0)}50%{transform:translateY(-2.5px) rotate(-5deg)}}
</style>
