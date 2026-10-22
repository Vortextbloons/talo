<script lang="ts">
  import BrowserPanel from './components/BrowserPanel.svelte';
  import { initBrowser, closeBrowser } from './lib/browser.svelte';
  import { onMount } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import mascot from '../../../packages/design/mascot.svg';
  import NavIcon from './NavIcon.svelte';
  import { library, refreshLibrary } from './lib/library.svelte';
  import LibraryPanel from './components/LibraryPanel.svelte';
  import ChatView from './components/ChatView.svelte';
  import ProviderSettings from './components/ProviderSettings.svelte';
  import ToolsSettings from './components/ToolsSettings.svelte';
  import ToolApprovals from './components/ToolApprovals.svelte';
  import TasksPanel from './components/TasksPanel.svelte';
  import { chat, disposeChat, initChat, selectionLabel } from './lib/chat.svelte';
  import { CUSTOM_BASE_URL_EXAMPLE, DEFAULT_PREFERENCE, PROVIDERS, parseProviderPreference, providerDescriptor } from '../../../packages/contracts/src/index';
  import type { Onboarding, RuntimeStatus, ServiceId } from '../../../packages/contracts/src/index';
  const pages = ['Chat', 'Workspaces', 'Tasks', 'Browser', 'Memory', 'Settings'] as const;
  type Page = typeof pages[number];
  let page = $state<Page>('Chat');
  let taskFocus = $state('');
  function navigatePage(next: Page) { if (page !== next) closeBrowser(); page = next; }
  const NAV_KEY = 'talo.nav.collapsed';
  function initialNavCollapsed(): boolean {
    try { return localStorage.getItem(NAV_KEY) === '1'; } catch { return false; }
  }
  let navCollapsed = $state(initialNavCollapsed());
  function toggleNav(): void {
    navCollapsed = !navCollapsed;
    try { localStorage.setItem(NAV_KEY, navCollapsed ? '1' : '0'); } catch { /* storage can be unavailable */ }
  }
  let settings = $state<Onboarding>({name:'',personality:'Thoughtful and direct',...DEFAULT_PREFERENCE,memory:'workspace',showReasoning:false,complete:false});
  let provider = $derived(providerDescriptor(settings.provider));
  let connectedCount = $derived(chat.connections.filter(connection=>connection.connected).length);
  let chatSelection = $derived(selectionLabel(chat.active?.conversation.selection ?? chat.draftSelection));
  const aiWorking = $derived(chat.active?.state === 'generating' || chat.active?.state === 'cancelling');
  let loaded = $state(false);
  let editing = $state(false);
  let step = $state(0);
  let status = $state<RuntimeStatus | null>(null);
  let error = $state('');
  let busy = $state(false);
  let pending = $state<{id:string; action:string; resource:string; run:(id:string)=>Promise<void>} | null>(null);
  const descriptions:Record<Page,string> = {
    Chat:'A little space to think things through.',Workspaces:'Keep each project in its own place.',Tasks:'A home for the things you want to come back to.',Browser:'Your browser, with a space of its own.',Memory:'The useful things, remembered on your terms.',Settings:'Make yourself at home.'
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
    const stopBrowser = initBrowser();
    (async()=>{try { if(isTauri()){const saved=await invoke<Onboarding|null>('load_settings');if(saved && active)settings={...settings,...saved,...parseProviderPreference(saved)};await refresh();} }catch(e){error=String(e);}finally{if(active)loaded=true;}})();
    // The host pushes service transitions; polling would keep the hidden webview awake.
    if(isTauri())listen<RuntimeStatus>('runtime-status',event=>{if(active)status=event.payload;}).then(stop=>{if(active)unlisten=stop;else stop();});
    const wake=()=>{if(active)refresh().catch(()=>{});};
    window.addEventListener('focus',wake);
    return ()=>{active=false;unlisten?.();stopAutomation?.();window.removeEventListener('focus',wake);disposeChat();stopBrowser();};
  });
  async function save() { try {busy=true;if(isTauri())await invoke('save_settings',{settings:{...settings,complete:true}});settings.complete=true;editing=false;void refreshLibrary();}catch(e){error=String(e);}finally{busy=false;} }
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
  async function renameAgent(name: string) {
    if (!isTauri()) throw new Error('Open the desktop app to save your agent name.');
    const next = {...settings, agentName: name};
    await invoke('save_settings', {settings: next});
    settings = next;
  }
  async function saveReasoning(){
    const value = settings.showReasoning;
    if(!isTauri())return;
    try{await invoke('save_settings',{settings:{...settings,complete:true}});}
    catch(e){error=String(e);settings.showReasoning=!value;}
  }
</script>

<div class="shell" class:nav-collapsed={navCollapsed} class:ai-working={aiWorking}>
  <aside inert={navCollapsed} aria-hidden={navCollapsed}>
    <div class="nav-inner">
      <div class="aside-head">
        <a class="brand" href="#chat" onclick={()=>navigatePage('Chat')}><img src={mascot} alt=""/><span>talo<span class="dot">.</span></span></a>
        <button class="rail-toggle" onclick={toggleNav} aria-expanded={!navCollapsed} aria-label="Collapse navigation" title="Collapse navigation"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><rect x="3.5" y="4.5" width="17" height="15" rx="3.5"/><path d="M9.5 4.5v15"/></svg></button>
      </div>
      <div class="nav-caption">Your everyday companion</div>
      <nav aria-label="Main navigation">{#each pages as item (item)}<button class={{selected:page===item}} class:thinking={item==='Chat' && aiWorking} onclick={()=>navigatePage(item)} title={item}><span class="nav-icon"><NavIcon name={item} working={item==='Chat' && aiWorking} /></span><span class="nav-label">{item}</span>{#if page===item}<span class="nav-dot"></span>{/if}</button>{/each}</nav>
      <div class="sidebar-bottom"><span class="presence"></span><div>Talo is here<small>{isTauri()?'Running quietly in your tray':'Interface preview'}</small></div></div>
    </div>
  </aside>
  <main>
    <header>
      {#if navCollapsed}
        <button class="header-toggle" onclick={toggleNav} aria-expanded={!navCollapsed} aria-label="Show navigation" title="Show navigation"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><rect x="3.5" y="4.5" width="17" height="15" rx="3.5"/><path d="M9.5 4.5v15"/></svg></button>
      {/if}
      <span class="crumbs">Personal space <span class="slash">/</span> {page}</span>
    </header>
    {#if error}<div class="error" role="alert">{error}<button aria-label="Dismiss error" onclick={()=>error=''}>×</button></div>{/if}
    <div class="content" class:chat-mode={page==='Chat'}>
      {#if page!=='Chat'}<div class="page-heading"><h1>{page}</h1><p>{descriptions[page]}</p></div>{/if}
      <BrowserPanel pageView={page === 'Browser'} />
      {#if page==='Chat'}
        <ChatView onOpenSettings={()=>{navigatePage('Settings');}} name={settings.name} tagline={descriptions.Chat} showReasoning={settings.showReasoning === true} agentName={settings.agentName || 'Talo'} onRenameAgent={renameAgent} />
      {:else if page==='Memory' || page==='Workspaces'}
        <LibraryPanel mode={page} />
      {:else if page==='Settings'}
        <div class="settings-grid">
          <div class="settings-col main">
            <ProviderSettings />
            <ToolsSettings />
          </div>
          <div class="settings-col side">
            <section class="card card--prefs"><h2>Your preferences</h2><dl><dt>Name</dt><dd>{settings.name||'Not set'}</dd><dt>Personality</dt><dd>{settings.personality}</dd><dt>Chat model</dt><dd>{connectedCount ? chatSelection : 'No provider connected yet'}</dd>{#if settings.provider==='custom'}<dt>Custom endpoint</dt><dd>{settings.baseUrl||'Not set'}</dd>{/if}<dt>Memory</dt><dd>{library.memoryEnabled ? 'Enabled' : 'Off'}</dd></dl><label class="check preference-toggle"><input type="checkbox" bind:checked={settings.showReasoning} onchange={saveReasoning}/> <span>Show model reasoning in Chat</span></label><button class="primary" onclick={()=>{editing=true;step=0;}}>Edit setup</button></section>
            <section class="card card--services"><h2>Local services</h2><p>Start or stop the bundled runtimes. Pi starts when you send a message and rests after two idle minutes.</p>{#each ['pi','browser'] as service (service)}<div class="service"><div><strong>{service==='pi'?'Pi worker':'Chromium'}</strong><small>{status?.services[service as ServiceId]??'Desktop app required'}</small></div><button disabled={busy||!isTauri()} onclick={()=>status?.services[service as ServiceId]==='running'?control(service as ServiceId,false):service==='browser'?browserStart():control('pi',true)}>{status?.services[service as ServiceId]==='running'?'Stop':'Start'}</button></div>{/each}<p class="detail">SQLite: {status?.database??'Desktop app required'}</p>{#if status}<div class="dependencies">{#each Object.entries(status.dependencies) as [name,ready] (name)}<span>{ready?'✓':'○'} {name}</span>{/each}</div>{/if}</section>
            <section class="card card--windows"><h2>Windows</h2><p>Closing the window keeps Talo in the tray. Use Quit Talo to exit.</p><button disabled={busy||!isTauri()} onclick={()=>permission('autostart.change','enable',async grant=>{await invoke('set_autostart',{enabled:true,grant});})}>Enable launch at sign-in</button> <button disabled={busy||!isTauri()} onclick={()=>permission('autostart.change','disable',async grant=>{await invoke('set_autostart',{enabled:false,grant});})}>Disable launch at sign-in</button></section>
          </div>
        </div>
      {:else if page==='Browser'}
        <p class="browser-note">Browser profiles and downloads are stored separately from chat transcripts and memory.</p>
      {:else if page==='Tasks'}
        <TasksPanel focusRun={taskFocus} onOpenChat={()=>navigatePage('Chat')} />
      {:else}
        <section class="empty"><span class="empty-badge"><span class="empty-icon">&#9745;</span></span><h2>A clear slate.</h2><p>Your scheduled tasks will live here. Automatic execution is not enabled.</p><span class="coming">Foundation in place</span></section>
      {/if}
      {#if page!=='Chat'}<footer><span>Made for your everyday.</span><span>Talo / 0.1.0</span></footer>{/if}
    </div>
  </main>
</div>
{#if loaded && (!settings.complete || editing)}
  <div class="overlay"><dialog open class="onboarding" aria-modal="true" aria-labelledby="setup-title"><img src={mascot} alt="Talo"/><div class="steps">Getting acquainted · {step+1} of 3</div><h2 id="setup-title">{step===0?'A little introduction.':step===1?'Choose your intelligence.':'Remember what matters.'}</h2>
    {#if step===0}<label>Your name <span>(optional)</span><input bind:value={settings.name} maxlength="80" placeholder="What should Talo call you?"/></label><label>Assistant personality<select bind:value={settings.personality}><option>Thoughtful and direct</option><option>Warm and encouraging</option><option>Brief and practical</option></select></label>
    {:else if step===1}<label>Starting provider<select bind:value={settings.provider}>{#each PROVIDERS as item (item)}<option value={item.id}>{item.name}</option>{/each}</select></label>{#if settings.provider==='custom'}<label>Base URL<input bind:value={settings.baseUrl} maxlength="2048" placeholder={CUSTOM_BASE_URL_EXAMPLE}/></label><label>Model ID<input bind:value={settings.model} maxlength="200" placeholder="llama3.3"/></label><label class="check"><input type="checkbox" bind:checked={settings.reasoning}/> <span>This endpoint supports reasoning</span></label><p class="setup-note">Talo writes this endpoint to Pi's models.json. Reasoning adds Pi's thinking levels to the model. Local servers need no key. Secure key connection for hosted custom endpoints is coming next.</p>{:else}<p class="setup-note">{provider.tagline} Choose a starting provider now. You will connect its key and choose a chat model in Settings before your first message.</p>{/if}
    {:else}<label>Memory preference<select bind:value={settings.memory}><option value="workspace">Keep memory within each workspace</option><option value="global">Allow memory across workspaces</option><option value="off">Keep memory off</option></select></label><p class="setup-note">Memories stay on this device. Automatic capture is not enabled yet. You can change this preference in Settings.</p>{/if}
    <div class="setup-actions">{#if step>0}<button onclick={()=>step--}>Back</button>{:else}<span></span>{/if}{#if step<2}<button class="primary" onclick={()=>step++}>Continue</button>{:else}<button class="primary" disabled={busy} onclick={save}>Make yourself at home</button>{/if}</div>
  </dialog></div>
{/if}
{#if pending}<div class="overlay"><dialog open class="onboarding" aria-modal="true" aria-labelledby="permission-title"><h2 id="permission-title">Allow this action?</h2><p>{pending.action==='browser.start'?'Open Chromium with Talo’s dedicated persistent profile?':'Change whether Talo starts when you sign in to Windows?'}</p><p class="detail">{pending.resource} · This action only</p><div class="setup-actions"><button onclick={()=>resolve(false)}>Deny</button><button class="primary" onclick={()=>resolve(true)}>Allow once</button></div></dialog></div>{/if}

<ToolApprovals />
<style>
  /* The window is the page: a fixed app frame, each region scrolls itself. */
  .shell{display:flex;height:100vh;height:100dvh;overflow:hidden;background:var(--bg);--nav-w:226px}
  aside{width:var(--nav-w);flex-shrink:0;background:var(--sidebar);color:var(--text);border-right:1px solid var(--line);overflow:hidden;transition:width .24s cubic-bezier(.2,.7,.3,1),border-color .2s ease}
  .nav-inner{width:var(--nav-w);height:100%;padding:26px 16px 20px;display:flex;flex-direction:column;min-height:0;transition:opacity .16s ease}
  .shell.nav-collapsed aside{width:0;border-right-color:transparent}
  .shell.nav-collapsed .nav-inner{opacity:0;visibility:hidden}
  .aside-head{display:flex;align-items:center;justify-content:space-between;gap:8px;padding-left:12px;min-width:0}
  .brand{display:flex;align-items:center;gap:11px;padding:0;color:var(--text);text-decoration:none;min-width:0}
  .brand img{width:34px;flex-shrink:0;filter:drop-shadow(0 0 18px rgba(255,212,92,.18))}
  .brand>span{font-size:28px;font-weight:650;letter-spacing:-1.4px}
  .dot{color:var(--yellow)}
  .rail-toggle,.header-toggle{display:grid;place-items:center;width:28px;height:28px;padding:0;flex-shrink:0;background:transparent;border:1px solid transparent;border-radius:7px;color:var(--faint);transition:color .15s ease,border-color .15s ease,background .15s ease}
  .rail-toggle svg,.header-toggle svg{width:15px;height:15px}
  .rail-toggle:hover,.header-toggle:hover{color:var(--text);border-color:var(--line-strong);background:var(--soft)}
  .header-toggle{align-self:center;margin-right:-4px}
  .nav-caption{font-size:10.5px;color:var(--faint);margin:16px 12px 24px;letter-spacing:.06em;text-transform:uppercase}
  nav{display:grid;gap:4px}
  nav button{background:transparent;color:var(--muted);border:0;padding:11px 13px;text-align:left;display:flex;align-items:center;gap:12px;border-radius:9px;font-size:13.5px;white-space:nowrap}
  .nav-label{min-width:0;overflow:hidden;text-overflow:ellipsis}
  nav button:hover{background:#1d1d19;color:var(--text)}
  nav button.selected{background:#26261f;color:var(--yellow)}
  /* While Talo works, the chat entry glows so the answer feels alive from the rail too. */
  nav button.thinking{color:var(--yellow)}
  nav button.thinking .nav-icon{--icon-opacity:1}
  .shell.ai-working .brand img{animation:brand-bob 1.7s ease-in-out infinite;filter:drop-shadow(0 0 22px rgba(255,212,92,.34))}
  .nav-icon{--icon-opacity:.6;width:22px;height:22px;display:grid;place-items:center;flex-shrink:0}
  nav button:hover .nav-icon{--icon-opacity:.85}
  nav button.selected .nav-icon{--icon-opacity:1}
  .nav-dot{width:5px;height:5px;background:var(--yellow);border-radius:50%;margin-left:auto}
  .sidebar-bottom{margin-top:auto;display:flex;gap:10px;align-items:center;padding:16px 12px 0;border-top:1px solid var(--line);font-size:12px}
  .sidebar-bottom small{display:block;color:var(--faint);font-size:10px;margin-top:4px}
  .presence{display:inline-block;width:6px;height:6px;background:var(--yellow-deep);border-radius:50%;flex-shrink:0}
  main{flex:1;min-width:0;display:flex;flex-direction:column}
  header{height:52px;flex-shrink:0;border-bottom:1px solid var(--line);padding:0 28px;display:flex;align-items:center;justify-content:space-between;gap:14px}
  .crumbs{font-size:10.5px;letter-spacing:.12em;text-transform:uppercase;color:var(--muted);margin-right:auto}
  .slash{margin:0 10px;color:var(--faint)}
  .error{margin:12px 28px 0;padding:12px;background:var(--error-bg);border:1px solid var(--error-line);color:var(--error-text);border-radius:10px;font-size:12px;display:flex;justify-content:space-between;align-items:center}
  .error button{padding:2px 8px;background:transparent;border:0}
  /* Scrolling pages flow; chat keeps everything on one surface. */
  .content{flex:1;min-height:0;display:flex;flex-direction:column;padding:26px 28px 22px;overflow-y:auto;scrollbar-gutter:stable}
  .content.chat-mode{overflow:hidden;padding:14px 24px 20px 18px}
  .page-heading h1{font-size:30px;letter-spacing:-1.2px;font-weight:560;margin:0 0 8px}
  .page-heading p,.empty p,.card>p{color:var(--muted);font-size:13.5px;line-height:1.7;margin:0}
  .page-heading p{max-width:62ch}
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
  /* Settings fills the canvas: providers anchor the wide column, small cards stack beside it. */
  .settings-grid{display:grid;grid-template-columns:minmax(0,7fr) minmax(0,5fr);gap:22px;align-items:start}
  .settings-col{display:grid;gap:22px;min-width:0}
  .card h2{font-size:17px;letter-spacing:-.4px;font-weight:550;margin:0 0 4px}
  .card>p{margin:6px 0 0}
  .preference-toggle{margin:0 0 18px;font-size:12.5px}
  dl{display:grid;grid-template-columns:140px 1fr;gap:13px 18px;font-size:13px;margin:20px 0}
  dt{color:var(--muted)}
  dd{margin:0}
  .service{display:flex;align-items:center;justify-content:space-between;gap:14px;padding:15px 0;border-bottom:1px solid var(--line)}
  .service:first-of-type{margin-top:16px}
  .service strong{font-size:13px;font-weight:550}
  .service small{display:block;font-size:11px;color:var(--muted);margin-top:4px}
  .detail{font-size:11px;margin-top:14px}
  .dependencies{display:flex;gap:18px;font-size:11px;margin-top:12px;color:var(--muted)}
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
  @media(max-width:1180px){
    /* One column, in the order a new user needs: preferences, providers, then the rest. */
    .settings-grid{display:flex;flex-direction:column;gap:18px}
    .settings-col{display:contents}
    .settings-grid .card--prefs{order:1}
    .settings-grid :global(.providers){order:2}
    .settings-grid .card--services{order:3}
    .settings-grid .card--windows{order:4}
  }
  @media(max-width:900px){
    .shell{--nav-w:188px}
    .nav-inner{padding:22px 12px 18px}
    header{padding:0 28px}
    .error{margin:12px 28px 0}
    .content{padding:22px 22px 18px}
    .content.chat-mode{padding:12px 20px 16px}
  }
  @media(max-width:640px){
    .shell{--nav-w:70px}
    .nav-inner{padding:18px 8px}
    .aside-head{flex-direction:column;gap:12px;padding-left:0}
    .brand{padding:0;justify-content:center}
    .brand>span,.nav-caption,.sidebar-bottom{display:none}
    .brand img{width:40px}
    nav{margin-top:24px}
    nav button{font-size:0;padding:12px;justify-content:center}
    .nav-icon{--icon-size:21px;--icon-opacity:.85}
    .nav-dot{display:none}
    header{padding:0 20px}
    .error{margin:10px 20px 0}
    .content{padding:18px 20px 16px}
    .content.chat-mode{padding:10px 16px 14px}
    .page-heading h1{font-size:26px}
    dl{grid-template-columns:1fr;gap:7px}
    dd{margin-bottom:10px}
    .dependencies{flex-wrap:wrap}
  }
  @media(prefers-reduced-motion:reduce){
    aside,.nav-inner,.rail-toggle,.header-toggle{transition:none}
    .shell.ai-working .brand img{animation:none}
  }
  @keyframes brand-bob{0%,100%{transform:translateY(0) rotate(0)}50%{transform:translateY(-2.5px) rotate(-5deg)}}
</style>
