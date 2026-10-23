<script lang="ts">
  import { PRIMARY_CONVERSATION_ID } from '../../../packages/contracts/src/telegram';
  import { browser, initBrowser, closeBrowser, applyBrowserApprovals, applyRuntimeStatus, browserApprovalRevision } from './lib/browser.svelte';
  import { onMount } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import mascot from '../../../packages/design/mascot.svg';
  import NavIcon from './NavIcon.svelte';
  import { library, refreshLibrary, mutateLibrary, applyLibraryBootstrap } from './lib/library.svelte';
  import ChatView from './components/ChatView.svelte';
  import ToolApprovals from './components/ToolApprovals.svelte';
  import { agent, initAgent, applyAgentState, agentStateRevision, refreshAgent } from './lib/agent.svelte';
  import { chat, disposeChat, initChat, createConversation, openConversation, routing } from './lib/chat.svelte';
  import { checkForUpdates } from './lib/updates.svelte';
  import { CUSTOM_BASE_URL_EXAMPLE, DEFAULT_PREFERENCE, PROVIDERS, parseProviderPreference, providerDescriptor } from '../../../packages/contracts/src/index';
  import { tools } from './lib/tools.svelte';
  import type { Onboarding, RuntimeStatus, ServiceId, StartupSnapshot } from '../../../packages/contracts/src/index';
  const pages = ['Chat', 'Inbox', 'Workspaces', 'Memory', 'Skills', 'Tasks', 'Settings'] as const;
  type Page = typeof pages[number];
  const labels: Record<Page, string> = { Chat: 'Chat', Inbox: 'Inbox', Workspaces: 'Projects', Memory: 'Memory', Skills: 'Skills', Tasks: 'Tasks', Settings: 'Settings' };
  const primaryPages: readonly Page[] = ['Chat', 'Inbox', 'Workspaces', 'Memory'];
  const advancedPages: readonly Page[] = ['Skills', 'Tasks'];
  let page = $state<Page>('Chat');
  let advancedOpen = $state(false);
  $effect(() => { if (advancedPages.includes(page)) advancedOpen = true; });

  const inboxCount = $derived(agent.asks.length + agent.drafts.length + agent.skillProposals.length);
  let taskFocus = $state('');
  let settingsTab = $state('');
  function openPrimary(): void { navigatePage('Chat'); void openConversation(PRIMARY_CONVERSATION_ID); }
  function openSettings(tab?: string): void { settingsTab = tab ?? ''; navigatePage('Settings'); }
  function navigatePage(next: Page) { if (page !== next) { closeBrowser(); if (next !== 'Settings') settingsTab = ''; } page = next; }

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
  let startupReady = $state(false);
  let startup = $state.raw<StartupSnapshot | null>(null);
  let editing = $state(false);
  let step = $state(0);
  let preferChatGPT = $state(false);
  let status = $state<RuntimeStatus | null>(null);
  let error = $state('');
  let busy = $state(false);
  let pending = $state<{id:string; action:string; resource:string; run:(id:string)=>Promise<void>} | null>(null);
  const descriptions:Record<Page,string> = {
    Chat:'One continuing chat with Talo. Ask in plain words; it handles the rest.',Inbox:'What Talo has asked, proposed or left for you.',Workspaces:'One Talo, with conversations and context organized by project.',Memory:'What Talo remembers about each project, ready to correct or remove.',Skills:'Ways of working Talo saved once they worked.',Tasks:'A home for the things you want to come back to.',Settings:'Make yourself at home.'
  };
  async function refresh() { if(isTauri()) { status = await invoke<RuntimeStatus>('runtime_status'); applyRuntimeStatus(status); } }
  onMount(() => {
    let active=true;
    let unlisten:(()=>void)|undefined;
    let stopAutomation:(()=>void)|undefined;
    let runtimeRevision=0;
    const notification=async()=>{if(!isTauri())return;const run=await invoke<string>('automation_action',{operation:'notification'});if(run&&active){taskFocus=run;navigatePage('Tasks');}};
    if(isTauri()){
      void listen('automation-open',()=>void notification().catch(()=>{})).then(stop=>{if(active)stopAutomation=stop;else stop();});
    }
    const stopAgent = initAgent(null);
    const stopBrowser = initBrowser(null);
    if(isTauri())listen<RuntimeStatus>('runtime-status',event=>{if(active){runtimeRevision+=1;status=event.payload;applyRuntimeStatus(event.payload);}}).then(stop=>{if(active)unlisten=stop;else stop();});
    void (async()=>{
      try {
        const initialRuntimeRevision=runtimeRevision;
        const initialAgentRevision=agentStateRevision();
        const initialBrowserApprovalRevision=browserApprovalRevision();
        startup = await initChat();
        if (!active) return;
        if (startup) {
          if (startup.errors?.length) error = startup.errors.join('\n');
          if(agentStateRevision()===initialAgentRevision)applyAgentState(startup.agent);
          if (startup.library) applyLibraryBootstrap(startup.library);
          tools.state.pending = startup.approvals.tools;
          if(browserApprovalRevision()===initialBrowserApprovalRevision)applyBrowserApprovals(startup.approvals.browser);
          if(runtimeRevision===initialRuntimeRevision){status = startup.runtime;applyRuntimeStatus(startup.runtime);}
          if (startup.settings) {
            settings = {...settings,...startup.settings,...parseProviderPreference(startup.settings)};
            routing.mode = startup.settings.routing ?? 'auto';
            routing.tier = startup.settings.performance ?? 'recommended';
          }
        } else if (isTauri()) {
          const [saved] = await Promise.all([
            invoke<Onboarding|null>('load_settings').catch(e=>{if(active)error=String(e);return null;}),
            refreshAgent(),refreshLibrary(),refresh().catch(e=>{if(active)error=String(e);}),
          ]);
          if(saved&&active){settings={...settings,...saved,...parseProviderPreference(saved)};routing.mode=saved.routing??'auto';routing.tier=saved.performance??'recommended';}
        }
      } catch(e) {
        if (active) error = String(e);
      } finally {
        if (active) {
          startupReady = true;
          loaded = true;
          if (isTauri()) requestAnimationFrame(() => requestAnimationFrame(() => { if (active) void notification().catch(()=>{}); }));
        }
      }
    })();

    const updateCheck = setTimeout(() => { if (active) void checkForUpdates({ quiet: true }); }, 4000);
    const wake=()=>{if(active&&startupReady)requestAnimationFrame(()=>{if(active)refresh().catch(()=>{});});};
    window.addEventListener('focus',wake);
    return ()=>{active=false;unlisten?.();stopAutomation?.();window.removeEventListener('focus',wake);disposeChat();stopAgent();stopBrowser();};
  });
  async function saveSettings(next: Onboarding): Promise<void> {
    if(isTauri()) await invoke('save_settings', { settings: { ...next, complete: true } });
    settings = { ...next, complete: true };
    routing.mode = next.routing ?? 'auto';
    routing.tier = next.performance ?? 'recommended';
  }

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

  async function skipSetup(){
    if(busy)return;
    try{await saveSettings(settings);}catch(e){error=String(e);return;}
    editing=false;void refreshLibrary();
  }

  async function connectKeyNow(){
    if(busy)return;
    try{await saveSettings(settings);}catch(e){error=String(e);return;}
    editing=false;void refreshLibrary();
    openSettings('Models');
  }

  const PERMISSION_COPY: Record<string,{title:string;body:string}> = {
    'browser.start':{title:'Open the browser?',body:'Talo opens a separate browser window that it controls. Your own browser and its sign-ins are not used.'},
    'autostart.change':{title:'Start Talo with Windows?',body:'Windows will launch Talo when you sign in, so it is already running when you need it.'},
  };
  const permissionCopy = $derived(PERMISSION_COPY[pending?.action ?? ''] ?? {title:'Allow this action?',body:'Talo needs your permission for this.'});


  function onKeydown(event:KeyboardEvent):void{
    if(!loaded||!settings.complete||editing)return;
    if(!event.ctrlKey||event.altKey||event.shiftKey||event.metaKey)return;
    if(event.key>='1'&&event.key<='9'){
      const index=Number(event.key);
      const item=index<=pages.length?pages[index-1]:undefined;
      if(item){event.preventDefault();if(item==='Chat')openPrimary();else navigatePage(item);}
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
        <button class="brand" onclick={openPrimary} title="Chat" aria-label="Talo — go to Chat">
          <img src={mascot} alt=""/>
          <span class="wordmark">talo<span class="dot">.</span></span>
        </button>
        <button class="pin" onclick={toggleRail} aria-pressed={railPinned} title={railPinned?'Let the rail close again':'Keep the rail open'} aria-label={railPinned?'Let the navigation rail close again':'Keep the navigation rail open'}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><path d="M12 17v4"/><path d="M9 3h6l-1 6 3 3H7l3-3-1-6Z"/></svg>
        </button>
      </div>
      {#snippet navButton(item: Page)}
        <button class:selected={page===item} class:thinking={item==='Chat' && (chat.active?.state==='generating'||chat.active?.state==='cancelling')} onclick={()=>navigatePage(item)} title={item === 'Chat' ? (settings.agentName || 'Talo') : labels[item]} aria-current={page===item?'page':undefined}>
          <span class="nav-icon"><NavIcon name={item} working={item==='Chat' && (chat.active?.state==='generating'||chat.active?.state==='cancelling')} /></span>
          <span class="nav-label">{item === 'Chat' ? (settings.agentName || 'Talo') : labels[item]}</span>
          {#if item==='Inbox' && inboxCount}<span class="nav-badge" aria-label={`${inboxCount} waiting`}>{inboxCount}</span>{/if}
        </button>
      {/snippet}
      <nav>{#each primaryPages as item (item)}{@render navButton(item)}{/each}<button class="more-toggle" class:open={advancedOpen} class:selected={advancedPages.includes(page)} aria-expanded={advancedOpen} aria-controls="rail-more" title={advancedOpen ? 'Hide Skills and Tasks' : 'Skills and Tasks'} onclick={()=>advancedOpen=!advancedOpen}>
        <span class="nav-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true" focusable="false"><circle cx="5.6" cy="12" r="1.3"/><circle cx="12" cy="12" r="1.3"/><circle cx="18.4" cy="12" r="1.3"/></svg></span>
        <span class="nav-label">More</span>
        <svg class="more-chevron" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><path d="m6 9.5 6 6 6-6"/></svg>
      </button>{#if advancedOpen}<div class="rail-more" id="rail-more">{#each advancedPages as item (item)}{@render navButton(item)}{/each}</div>{/if}{@render navButton('Settings')}
      </nav>
      <div class="rail-foot"><span class="presence"></span><span class="foot-label">Talo is here</span></div>
    </div>
  </aside>
  <main>
    <header><span class="crumbs">{labels[page]}</span></header>
    {#if error}<div class="error" role="alert">{error}<button aria-label="Dismiss error" onclick={()=>error=''}>×</button></div>{/if}
    <div class="content" class:chat-mode={page==='Chat'} class:settings-mode={page==='Settings'}>
      {#if page!=='Chat' && page!=='Settings'}<div class="page-heading"><h1>{labels[page]}</h1><p>{descriptions[page]}</p></div>{/if}
      {#if browser.view === 'large' || browser.approvals.length}
        {#await import('./components/BrowserPanel.svelte') then { default: BrowserPanel }}
          <BrowserPanel />
        {:catch}
          <p class="error" role="alert">The browser view could not be loaded. Close and reopen Talo to try again.</p>
        {/await}
      {/if}
      {#if page==='Inbox'}
        {#await import('./components/InboxPanel.svelte')}
          <p role="status">Loading...</p>
        {:then { default: InboxPanel }}
          <InboxPanel onOpenChat={()=>navigatePage('Chat')} />
        {:catch}
          <p class="error" role="alert">This page could not be loaded. Close and reopen Talo to try again.</p>
        {/await}
      {:else if page==='Chat'}
        <ChatView onOpenSettings={(tab)=>openSettings(tab)} onOpenMemory={()=>navigatePage('Memory')} name={settings.name} tagline={descriptions.Chat} showReasoning={settings.showReasoning === true} agentName={settings.agentName || 'Talo'} onRenameAgent={renameAgent} />
      {:else if page==='Workspaces' || page==='Memory'}
        {#await import('./components/LibraryPanel.svelte')}
          <p role="status">Loading...</p>
        {:then { default: LibraryPanel }}
          <LibraryPanel mode={page} />
        {:catch}
          <p class="error" role="alert">This page could not be loaded. Close and reopen Talo to try again.</p>
        {/await}
      {:else if page==='Settings'}
        {#await import('./components/SettingsPanel.svelte')}
          <p role="status">Loading...</p>
        {:then { default: SettingsPanel }}
          <SettingsPanel
            {settings}
            {status}
            {busy}
            openTab={settingsTab}
            onPersist={persist}
            onService={control}
            onBrowserStart={browserStart}
            onAutostart={setAutostart}
            onMemory={setMemory}
            onOpenChat={()=>navigatePage('Chat')}
            onOpenMemory={()=>navigatePage('Memory')}
            onRerunSetup={()=>{editing=true;step=0;}}
          />
        {:catch}
          <p class="error" role="alert">This page could not be loaded. Close and reopen Talo to try again.</p>
        {/await}
      {:else if page==='Skills'}
        {#await import('./components/SkillsPanel.svelte')}
          <p role="status">Loading...</p>
        {:then { default: SkillsPanel }}
          <SkillsPanel />
        {:catch}
          <p class="error" role="alert">This page could not be loaded. Close and reopen Talo to try again.</p>
        {/await}
      {:else if page==='Tasks'}
        {#await import('./components/TasksPanel.svelte')}
          <p role="status">Loading...</p>
        {:then { default: TasksPanel }}
          <TasksPanel focusRun={taskFocus} onOpenChat={()=>navigatePage('Chat')} />
        {:catch}
          <p class="error" role="alert">This page could not be loaded. Close and reopen Talo to try again.</p>
        {/await}
      {:else}
        <section class="empty"><span class="empty-badge"><span class="empty-icon">&#9745;</span></span><h2>A clear slate.</h2><p>Your scheduled tasks will live here. Automatic execution is not enabled.</p><span class="coming">Foundation in place</span></section>
      {/if}
    </div>
  </main>
</div>
<svelte:window onkeydown={onKeydown} />
{#if loaded && (!settings.complete || editing)}
  <div class="overlay"><dialog open class="onboarding" aria-modal="true" aria-labelledby="setup-title"><img src={mascot} alt="Talo"/><div class="steps">{step===0?'Getting acquainted':'Choosing a model'} · {step+1} of 2</div><h2 id="setup-title">{step===0?'Hi, I’m Talo.':'Where should answers come from?'}</h2>
    {#if step===0}
      <p class="setup-lead">One companion and one continuing chat. I remember what matters in each project, tell you what I’m doing while I work, and bring results back to the chat you started in.</p>
      <label>Your name <span>(optional)</span><input bind:value={settings.name} maxlength="80" placeholder="What should Talo call you?"/></label><label>How it should sound<select bind:value={settings.personality}><option>Thoughtful and direct</option><option>Warm and encouraging</option><option>Brief and practical</option></select></label>
    {:else}
      <p class="setup-lead">Talo thinks with a model from a provider you choose. You connect your own account and that provider bills you for what Talo uses. Talo never charges you.</p>
      <label>Provider<select value={preferChatGPT ? 'chatgpt' : settings.provider} onchange={(event) => { const value = event.currentTarget.value; preferChatGPT = value === 'chatgpt'; if (!preferChatGPT) settings.provider = value; }}><option value="chatgpt">ChatGPT plan — sign in, no key</option>{#each PROVIDERS as item (item)}<option value={item.id}>{item.name}</option>{/each}</select></label>
      {#if preferChatGPT}
        <p class="provider-card"><strong>ChatGPT plan</strong><span>Use the plan you already pay for. Sign-in opens in your browser, plan limits apply, and there is no key to copy.</span></p>
        <p class="setup-note">Talo opens the ChatGPT sign-in on the next screen. You can switch providers any time in Settings → Models.</p>
      {:else if settings.provider==='custom'}
        <label>Base URL<input bind:value={settings.baseUrl} maxlength="2048" placeholder={CUSTOM_BASE_URL_EXAMPLE}/></label><label>Model ID<input bind:value={settings.model} maxlength="200" placeholder="llama3.3"/></label><label class="check"><input type="checkbox" bind:checked={settings.reasoning}/> <span>This endpoint supports reasoning</span></label>
        <p class="setup-note">Ollama, LM Studio, vLLM or any OpenAI-compatible server. Local servers need no key and cost nothing to run. Custom endpoints are finished in Settings → Models.</p>
      {:else}
        <p class="provider-card"><strong>{provider.name}</strong><span>{provider.tagline}</span></p>
        <p class="setup-note">Next, paste a key from {provider.name}. Windows holds the key, never a chat. You can change this later in Settings → Models.</p>
      {/if}
    {/if}
    <div class="setup-actions">{#if step>0}<button onclick={()=>step--}>Back</button>{:else}<button onclick={()=>{void skipSetup();}}>Skip for now</button>{/if}{#if step<1}<button class="primary" onclick={()=>step++}>Continue</button>{:else}<span class="setup-right"><button disabled={busy} onclick={()=>{void skipSetup();}}>I’ll add a key later</button><button class="primary" disabled={busy} onclick={()=>{void connectKeyNow();}}>{preferChatGPT?'Use my ChatGPT plan':settings.provider==='custom'?'Open model settings':'Add my key'}</button></span>{/if}</div>
  </dialog></div>
{/if}
{#if pending}<div class="overlay"><dialog open class="onboarding" aria-modal="true" aria-labelledby="permission-title"><span class="permission-eyebrow">Your permission</span><h2 id="permission-title">{permissionCopy.title}</h2><p>{permissionCopy.body}</p><p class="detail">{pending.resource} · This time only. Nothing happens unless you allow it.</p><div class="setup-actions"><button onclick={()=>resolve(false)}>Don’t allow</button><button class="primary" onclick={()=>resolve(true)}>Allow once</button></div></dialog></div>{/if}

<ToolApprovals initial={startup?.approvals ?? null} {startupReady} />
<style>


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

  nav button.selected::before{content:"";position:absolute;left:0;top:9px;bottom:9px;width:2px;border-radius:0 2px 2px 0;background:var(--yellow)}
  nav button.thinking{color:var(--yellow)}
  .more-chevron{width:11px;height:11px;margin-left:auto;flex-shrink:0;opacity:0;transition:opacity .13s ease,transform .15s ease}
  .rail:hover .more-chevron,.rail:focus-within .more-chevron,.rail-pinned .more-chevron{opacity:.6}
  .more-toggle[aria-expanded="true"] .more-chevron{transform:rotate(180deg)}
  .more-toggle.open{color:var(--text)}
  .rail-more{display:grid;gap:2px;margin-left:18px;border-left:1px solid var(--line)}
  .rail-more button{padding-left:12px}
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
  .content.settings-mode{overflow:hidden;padding:0;scrollbar-gutter:auto}
  .page-heading h1{font-size:34px;letter-spacing:-1.3px;font-weight:560;margin:0 0 9px}
  .page-heading p,.empty p{color:var(--muted);font-size:14px;line-height:1.7;margin:0}
  .page-heading p{max-width:76ch}
  .empty{margin:auto;padding:40px 20px;text-align:center;max-width:520px}
  .empty p{margin:0 0 24px}
  .empty-badge{display:grid;place-items:center;width:76px;height:76px;margin:0 auto 28px;border:1px solid var(--line-strong);border-radius:50%;background:var(--paper);box-shadow:0 0 64px 6px rgba(255,212,92,.09),inset 0 0 22px rgba(255,212,92,.05)}
  .empty-icon{font-size:28px;color:var(--yellow-deep)}
  .empty h2{font-size:23px;letter-spacing:-.7px;font-weight:550;margin:0 0 12px}
  .coming{display:inline-block;font-size:11px;color:var(--muted);background:var(--soft);border:1px solid var(--line);padding:7px 12px;border-radius:20px}
  button{border:1px solid var(--line-strong);background:var(--soft);color:var(--text);padding:9px 14px;border-radius:8px;font-size:12px}
  button:hover:not(:disabled){background:#2c2c27;border-color:#4a4a41}
  button.primary{background:var(--yellow);border-color:var(--yellow);color:var(--black);font-weight:600;padding:11px 18px}
  button.primary:hover:not(:disabled){background:#ffdd7f;border-color:#ffdd7f}
  .overlay{position:fixed;inset:0;background:var(--overlay);backdrop-filter:blur(5px);display:grid;place-items:center;padding:24px;z-index:5}
  .onboarding{position:relative;margin:auto;border:1px solid var(--line);color:var(--text);background:var(--paper);border-radius:24px;padding:36px;width:440px;max-width:100%;max-height:90vh;overflow-y:auto;box-shadow:0 30px 90px rgba(0,0,0,.65)}
  .onboarding>img{width:55px;margin-bottom:20px;filter:drop-shadow(0 0 26px rgba(255,212,92,.2))}
  .steps{font-size:11px;color:var(--muted);margin-bottom:14px}
  .onboarding h2{font-size:26px;letter-spacing:-.7px;margin-bottom:16px}
  .setup-lead{margin:0 0 6px;font-size:13px;line-height:1.7;color:var(--muted)}
  .provider-card{display:grid;gap:5px;margin:16px 0 0;padding:13px 15px;border:1px solid var(--line);border-radius:11px;background:var(--soft)}
  .provider-card strong{font-size:12.5px;font-weight:600;color:var(--text)}
  .provider-card span{font-size:11.5px;color:var(--muted);line-height:1.6}
  .permission-eyebrow{display:block;font-size:10px;letter-spacing:.12em;text-transform:uppercase;color:var(--yellow)}
  .detail{font-size:11.5px;color:var(--muted);line-height:1.6;margin:10px 0 0}
  label{display:block;font-size:13px;font-weight:550;margin:18px 0}
  label span{font-weight:400;color:var(--muted)}
  input,select{display:block;width:100%;padding:12px;border:1px solid var(--line-strong);border-radius:8px;margin-top:9px;background:var(--soft);color:var(--text)}
  input:hover,select:hover{border-color:#4a4a41}
  .setup-note{font-size:12px;margin-top:20px;color:var(--muted);line-height:1.65}
  .setup-actions{display:flex;justify-content:space-between;align-items:center;margin-top:30px;gap:12px;flex-wrap:wrap}
  .setup-right{display:flex;gap:10px;flex-wrap:wrap;justify-content:flex-end;margin-left:auto}
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
