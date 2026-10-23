<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { isTauri } from '@tauri-apps/api/core';
  import ProviderSettings from './ProviderSettings.svelte';
  import ToolsSettings from './ToolsSettings.svelte';
  import IntegrationsSettings from './IntegrationsSettings.svelte';
  import { chat, selectionLabel, routing } from '../lib/chat.svelte';
  import { rolesFor } from '../../../../packages/contracts/src/index';
  import { library } from '../lib/library.svelte';
  import { tools, refreshTools, toolAction } from '../lib/tools.svelte';
  import { updates, checkForUpdates, installUpdate } from '../lib/updates.svelte';
  import type { Onboarding, RuntimeStatus, ServiceId } from '../../../../packages/contracts/src/index';

  let {
    settings, status, busy,
    onPersist, onService, onBrowserStart, onAutostart, onMemory, onOpenChat, onRerunSetup,
  }: {
    settings: Onboarding;
    status: RuntimeStatus | null;
    busy: boolean;
    onPersist: (patch: Partial<Onboarding>) => Promise<void>;
    onService: (id: ServiceId, start: boolean) => Promise<void>;
    onBrowserStart: () => Promise<void>;
    onAutostart: (enabled: boolean) => Promise<void>;
    onMemory: (enabled: boolean) => Promise<void>;
    onOpenChat: () => void;
    onRerunSetup: () => void;
  } = $props();

  const TABS = ['You', 'Models', 'Capabilities', 'Integrations', 'Advanced'] as const;
  type Tab = typeof TABS[number];
  const SECTIONS: Record<Tab, { label: string; icon: string }> = {
    You: { label: 'Personalization', icon: 'M20 21v-2a7 7 0 0 0-14 0v2M13 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8Z' },
    Models: { label: 'Models & providers', icon: 'm12 3 9 5-9 5-9-5 9-5Zm-9 9 9 5 9-5M3 16l9 5 9-5' },
    Capabilities: { label: 'Tools & permissions', icon: 'm12 3 8 4v5c0 5-8 9-8 9s-8-4-8-9V7l8-4Zm-4 9 3 3 5-6' },
    Integrations: { label: 'Connected apps', icon: 'M9 7V3m6 4V3M7 7h10v5a5 5 0 0 1-10 0V7Zm5 10v4' },
    Advanced: { label: 'App & updates', icon: 'M4 4h16v12H4V4Zm4 16h8m-4-4v4' },
  };
  const TAB_KEY = 'talo.settings.tab';
  function initialTab(): Tab {
    try {
      const saved = localStorage.getItem(TAB_KEY);
      if (saved && (TABS as readonly string[]).includes(saved)) return saved as Tab;
    } catch { /* storage can be unavailable */ }
    return 'You';
  }
  let tab = $state<Tab>(initialTab());
  function selectTab(next: Tab): void {
    tab = next;
    try { localStorage.setItem(TAB_KEY, next); } catch { /* storage can be unavailable */ }
  }
  async function navigateTabs(event: KeyboardEvent, item: Tab): Promise<void> {
    const directions: Record<string, number> = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 };
    if (!(event.key in directions) && event.key !== 'Home' && event.key !== 'End') return;
    event.preventDefault();
    const index = event.key === 'Home' ? 0 : event.key === 'End' ? TABS.length - 1 : (TABS.indexOf(item) + directions[event.key] + TABS.length) % TABS.length;
    selectTab(TABS[index]);
    await tick();
    document.getElementById(`settings-tab-${TABS[index]}`)?.focus();
  }

  const section = $derived(SECTIONS[tab]);

  const VOICES: Array<{ value: string; label: string }> = [
    { value: 'Thoughtful and direct', label: 'Thoughtful' },
    { value: 'Warm and encouraging', label: 'Warm' },
    { value: 'Brief and practical', label: 'Brief' },
  ];


  const tierLabels = $derived.by(() => {
    for (const connection of chat.connections) {
      if (!connection.connected) continue;
      const roles = rolesFor(connection.providerId);
      if (roles) return roles.tierLabels;
    }
    return { economy: 'Economy', recommended: 'Recommended' };
  });

  let editingName = $state(false);
  let draftName = $state('');
  async function beginName(): Promise<void> {
    draftName = settings.name ?? '';
    editingName = true;
    await tick();
    document.getElementById('settings-name')?.focus();
  }
  async function commitName(): Promise<void> {
    const next = draftName.trim().slice(0, 80);
    editingName = false;
    if (next === (settings.name ?? '')) return;
    await onPersist({ name: next });
  }
  function nameKey(event: KeyboardEvent): void {
    if (event.key === 'Enter') { event.preventDefault(); void commitName(); }
    if (event.key === 'Escape') { editingName = false; }
  }

  const selection = $derived(selectionLabel(chat.active?.conversation.selection ?? chat.draftSelection));

  const TOOL_LABELS: Record<string, string> = {
    web_search: 'Search the web', web_fetch: 'Read a page', web_crawl: 'Read a site', web_screenshot: 'Capture a page',
    browser_open: 'Open a page', browser_tabs: 'Work with tabs', browser_snapshot: 'Read the page', browser_interact: 'Click and type',
    browser_ai: 'Work out an unfamiliar page', browser_handoff: 'Hand control to you', browser_screenshot: 'Screenshot the browser',
    browser_download: 'Download a file', read: 'Read a file', write: 'Write a file', edit: 'Change a file', mcp: 'Use a connected app',
    google_gmail_list: 'Find emails', google_gmail_read: 'Read an email', google_gmail_draft: 'Draft an email', google_gmail_send: 'Send an email',
    google_calendar_list: 'Read your calendar', google_calendar_read: 'Read an event', google_calendar_create: 'Add an event',
    google_calendar_update: 'Change an event', google_calendar_delete: 'Delete an event',
    google_drive_list: 'Find Drive files', google_drive_read: 'Read a Drive file', google_drive_create: 'Create a Drive file', google_drive_update: 'Update a Drive file',
  };
  const book = (id: string): string => TOOL_LABELS[id] ?? id.replace(/[_-]+/gu, ' ').replace(/^./u, c => c.toUpperCase());

  const CATEGORY_COPY: Record<string, { label: string }> = {
    'Web Research': { label: 'Looking things up' },
    'Browser': { label: 'Its own browser' },
    'Files': { label: 'Your files' },
    'Integrations': { label: 'Connected apps' },
    'Memory': { label: 'Memory' },
  };
  const ORDER = ['Web Research', 'Browser', 'Files', 'Integrations', 'Memory'];
  const groups = $derived.by(() => {
    const byCategory = new Map<string, typeof tools.state.tools>();
    for (const tool of tools.state.tools) {
      const list = byCategory.get(tool.category) ?? [];
      list.push(tool);
      byCategory.set(tool.category, list);
    }
    return [...byCategory.entries()]
      .sort((a, b) => ORDER.indexOf(a[0]) - ORDER.indexOf(b[0]))
      .map(([category, list]) => ({ category, ...(CATEGORY_COPY[category] ?? { label: category }), tools: list }));
  });
  const enabledTools = $derived(tools.state.tools.filter(tool => tool.availability !== 'disabled').length);

  const DEPENDENCIES: Record<string, string> = {
    runtime: 'Assistant engine', chromium: 'Browser engine', donsetch: 'Web research engine',
    pi: 'Assistant files', stagehand: 'Browser intelligence', mcpAdapter: 'App connections',
  };
  const serviceState = (id: ServiceId): { text: string; running: boolean } => {
    const value = status?.services?.[id];
    if (!status) return { text: 'Desktop app required', running: false };
    if (value === 'running') return { text: 'Running', running: true };
    if (value === 'failed') return { text: 'Stopped unexpectedly', running: false };
    return { text: id === 'pi' ? 'Resting' : 'Sleeping', running: false };
  };
  const pi = $derived(serviceState('pi'));
  const browser = $derived(serviceState('browser'));

  let appVersion = $state('');
  onMount(() => {
    void refreshTools();
    if (isTauri()) void import('@tauri-apps/api/app').then(({ getVersion }) => getVersion()).then(version => { appVersion = version; }).catch(() => {});

    void checkForUpdates({ quiet: true });
  });

  const megabytes = (bytes: number): string => (bytes / (1024 * 1024)).toFixed(1);
  const downloaded = $derived(updates.total ? Math.min(100, Math.round((updates.received / updates.total) * 100)) : 0);
  const updateState = $derived(
    updates.phase === 'available' ? 'Update available.'
    : updates.phase === 'checking' ? 'Checking for updates…'
    : updates.phase === 'current' ? 'Up to date.'
    : updates.phase === 'preparing' ? 'Closing background services…'
    : updates.phase === 'downloading' ? `Downloading ${megabytes(updates.received)}${updates.total ? ` of ${megabytes(updates.total)} MB` : ''} MB…`
    : updates.phase === 'installing' ? 'Installing…'
    : updates.phase === 'error' ? updates.error
    : 'Checks automatically.'
  );

  async function toggleTool(id: string, enabled: boolean): Promise<void> {
    await toolAction('toggle', { id, enabled });
  }
</script>

<div class="settings">
  <aside class="settings-sidebar" aria-label="Settings">
    <div class="settings-title"><h1>Settings</h1></div>
    <div class="tabs" role="tablist" aria-label="Settings sections" aria-orientation="vertical">
      {#each TABS as item (item)}
        <button id={`settings-tab-${item}`} role="tab" aria-label={SECTIONS[item].label} title={SECTIONS[item].label} aria-selected={tab === item} aria-controls="settings-panel" tabindex={tab === item ? 0 : -1} class:selected={tab === item} onclick={() => selectTab(item)} onkeydown={event => void navigateTabs(event, item)}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d={SECTIONS[item].icon}/></svg>
          <span>{SECTIONS[item].label}</span>
          {#if item === 'Advanced' && updates.phase === 'available'}<span class="update-dot" aria-label="Update available"></span>{/if}
        </button>
      {/each}
    </div>
    <div class="sidebar-footer"><span class="app-emblem" aria-hidden="true">t</span><div><strong>Talo</strong><small>{appVersion ? `Version ${appVersion}` : 'Your desktop assistant'}</small></div></div>
  </aside>
  <div class="settings-content">
    <div class="section-header"><h2>{section.label}</h2></div>
    <div id="settings-panel" class="section-body" role="tabpanel" aria-labelledby={`settings-tab-${tab}`} tabindex="0">
  {#if tab === 'You'}
    <div class="tabpanel cols personalization">
      <section class="group">
        <h2>Conversation style</h2>
        <div class="rows">
          <div class="row">
            <div class="what"><strong>Your name</strong></div>
            <div class="how">
              {#if editingName}
                <input id="settings-name" class="inline" bind:value={draftName} maxlength="80" placeholder="What should Talo call you?" onkeydown={nameKey} onblur={() => void commitName()} aria-label="Your name" />
              {:else}
                <span class="value">{settings.name || 'Not set'}</span>
                <button class="quiet" onclick={beginName}>Change</button>
              {/if}
            </div>
          </div>
          <div class="row">
            <div class="what"><strong>Response style</strong></div>
            <div class="how">
              <div class="seg" role="group" aria-label="Voice">
                {#each VOICES as voice (voice.value)}
                  <button aria-pressed={settings.personality === voice.value} onclick={() => void onPersist({ personality: voice.value })}>{voice.label}</button>
                {/each}
              </div>
            </div>
          </div>
          <div class="row">
            <div class="what"><strong>Show reasoning</strong></div>
            <div class="how">
              <button class="switch" role="switch" aria-checked={settings.showReasoning === true} aria-label="Show reasoning" onclick={() => void onPersist({ showReasoning: !(settings.showReasoning === true) })}></button>
            </div>
          </div>
        </div>
      </section>

      <section class="group">
        <h2>Memory</h2>
        <div class="rows">
          <div class="row">
            <div class="what"><strong>Remember between chats</strong></div>
            <div class="how">
              <button class="switch" role="switch" aria-checked={library.memoryEnabled} aria-label="Remember between chats" onclick={() => void onMemory(!library.memoryEnabled)}></button>
            </div>
          </div>
        </div>
      </section>

      <details class="disclosure full">
        <summary><svg class="chev" viewBox="0 0 24 24" aria-hidden="true"><path d="m9 6 6 6-6 6"/></svg>Where your things live</summary>
        <div class="facts">
          <span>Chats and memory</span><b>Stored on this PC, in your user profile</b>
          <span>Browser profile</span><b>Separate from chats, kept with the app</b>
          <span>API keys</span><b>Held by Windows for this account, never in a chat</b>
          <span>Backups</span><b class="muted">Not set up yet</b>
        </div>
      </details>
    </div>
  {/if}

  {#if tab === 'Models'}
    <div class="tabpanel cols">
      <ProviderSettings />
      <section class="group full">
        <h2>Which model answers</h2>
        <div class="rows">
          <div class="row">
            <div class="what"><strong>Chat model</strong></div>
            <div class="how">
              <span class="value">{selection || 'Chosen when you send your first message'}</span>
              <button class="quiet" onclick={onOpenChat}>Choose in Chat</button>
            </div>
          </div>
          <div class="row">
            <div class="what"><strong>AI performance</strong></div>
            <div class="how">
              <div class="seg" role="group" aria-label="AI performance">
                <button aria-pressed={settings.performance === 'economy'} onclick={() => void onPersist({ performance: 'economy' })}>{tierLabels.economy}</button>
                <button aria-pressed={settings.performance !== 'economy'} onclick={() => void onPersist({ performance: 'recommended' })}>{tierLabels.recommended}</button>
              </div>
            </div>
          </div>
          <div class="row">
            <div class="what"><strong>Model selection</strong></div>
            <div class="how">
              <div class="seg" role="group" aria-label="Model selection">
                <button aria-pressed={settings.routing !== 'manual'} onclick={() => void onPersist({ routing: 'auto' })}>Auto</button>
                <button aria-pressed={settings.routing === 'manual'} onclick={() => void onPersist({ routing: 'manual' })}>Manual</button>
              </div>
            </div>
          </div>
        </div>
      </section>
    </div>
  {/if}

  {#if tab === 'Capabilities'}
    <div class="tabpanel cols">
      <section class="group full">
        <h2>What it can do</h2>
        {#if tools.state.tools.length}<p class="tool-total">{enabledTools} of {tools.state.tools.length} enabled</p>{/if}
        <div class="rows">
          {#each groups as group (group.category)}
            <details class="disclosure row-disclosure">
              <summary>
                <svg class="chev" viewBox="0 0 24 24" aria-hidden="true"><path d="m9 6 6 6-6 6"/></svg>
                <span class="summary-main"><strong>{group.label}</strong></span>
                <span class="count">{group.tools.filter(tool => tool.availability !== 'disabled').length} of {group.tools.length} on</span>
              </summary>
              <div class="tool-list">
                {#each group.tools as tool (tool.id)}
                  <div class="tool-row">
                    <div class="tool-what"><strong>{book(tool.id)}</strong></div>
                    <button class="switch small" role="switch" aria-checked={tool.availability !== 'disabled'} aria-label={book(tool.id)} onclick={() => void toggleTool(tool.id, tool.availability === 'disabled')}></button>
                  </div>
                {/each}
              </div>
            </details>
          {/each}
        </div>
      </section>

      {#if tools.state.grants.length}
        <details class="disclosure full">
          <summary><svg class="chev" viewBox="0 0 24 24" aria-hidden="true"><path d="m9 6 6 6-6 6"/></svg>Permissions you've remembered <span class="count">{tools.state.grants.length}</span></summary>
          <div class="grant-list">
            {#each tools.state.grants as grant (grant.id)}
              <div class="grant">
                <div><strong>{grant.summary}</strong><code>{grant.scope}</code></div>
                <button class="quiet" disabled={tools.busy} onclick={() => void toolAction('revoke', { id: grant.id })}>Revoke</button>
              </div>
            {/each}
          </div>
        </details>
      {/if}
    </div>
  {/if}

  {#if tab === 'Integrations'}
    <div class="tabpanel cols"><IntegrationsSettings /><ToolsSettings /></div>
  {/if}

  {#if tab === 'Advanced'}
    <div class="tabpanel cols">
      <section class="group full">
        <h2>Updates</h2>
        <div class="rows">
          <div class="row">
            <div class="what">
              <strong>Talo {appVersion || 'version unknown'}</strong>
              <small class:bad={updates.phase === 'error'}>{updateState}</small>
              {#if updates.phase === 'available' && updates.notes}
                <p class="notes">{updates.notes}</p>
              {/if}
            </div>
            <div class="how">
              {#if updates.phase === 'downloading'}
                <div class="progress" role="progressbar" aria-valuenow={downloaded} aria-valuemin="0" aria-valuemax="100" aria-label="Update download"><span style="width:{downloaded}%"></span></div>
              {:else if updates.phase === 'available'}
                <button class="primary" onclick={() => void installUpdate()}>Update now</button>
              {:else if updates.phase === 'preparing' || updates.phase === 'installing'}
                <span class="state">Working…</span>
              {:else if updates.phase === 'checking'}
                <span class="state">Checking…</span>
              {:else}
                <button class="quiet" disabled={!status} onclick={() => void checkForUpdates()}>{updates.phase === 'error' ? 'Try again' : 'Check now'}</button>
              {/if}
            </div>
          </div>
        </div>
        {#if updates.phase === 'available' || updates.phase === 'downloading'}
          <p class="after">Talo restarts to install the update.</p>
        {/if}
      </section>

      <section class="group">
        <h2>App behaviour</h2>
        <div class="rows">
          <div class="row">
            <div class="what"><strong>Start with Windows</strong></div>
            <div class="how">
              <button class="quiet" disabled={busy} onclick={() => void onAutostart(true)}>Turn on</button>
              <button class="quiet" disabled={busy} onclick={() => void onAutostart(false)}>Turn off</button>
            </div>
          </div>
          <div class="row">
            <div class="what"><strong>Closing the window</strong></div>
            <div class="how"><span class="value muted">Minimize to tray</span></div>
          </div>
        </div>
      </section>

      <section class="group">
        <h2>Background services</h2>
        <div class="rows">
          <div class="row">
            <div class="what"><strong>Assistant engine</strong></div>
            <div class="how">
              <span class="state" class:live={pi.running}>{pi.text}</span>
              <button class="quiet" disabled={busy || !status} onclick={() => void onService('pi', !pi.running)}>{pi.running ? 'Stop' : 'Start'}</button>
            </div>
          </div>
          <div class="row">
            <div class="what"><strong>Browser engine</strong></div>
            <div class="how">
              <span class="state" class:live={browser.running}>{browser.text}</span>
              <button class="quiet" disabled={busy || !status} onclick={() => void (browser.running ? onService('browser', false) : onBrowserStart())}>{browser.running ? 'Stop' : 'Start'}</button>
            </div>
          </div>
        </div>
      </section>

      <section class="group full">
        <h2>Technical details</h2>
        <details class="disclosure">
          <summary><svg class="chev" viewBox="0 0 24 24" aria-hidden="true"><path d="m9 6 6 6-6 6"/></svg>Bundled files</summary>
          <div class="facts">
            {#if status}
              {#each Object.entries(status.dependencies) as [key, ready] (key)}
                <span>{DEPENDENCIES[key] ?? key}</span><b class:bad={!ready}>{ready ? 'Present' : 'Missing'}</b>
              {/each}
              <span>Local database</span><b>{status.database === 'ready' ? 'Ready' : status.database}</b>
            {:else}
              <span>Runtime</span><b class="muted">Desktop app required</b>
            {/if}
          </div>
        </details>
        {#if tools.state.errors.length}
          <details class="disclosure">
            <summary><svg class="chev" viewBox="0 0 24 24" aria-hidden="true"><path d="m9 6 6 6-6 6"/></svg>Recent tool errors <span class="count">{tools.state.errors.length}</span></summary>
            <div class="errors">
              {#each tools.state.errors as item, index (`${item.at}-${index}`)}
                <p><strong>{item.server}</strong> · {new Date(item.at).toLocaleTimeString()}<br/>{item.message}</p>
              {/each}
            </div>
          </details>
        {/if}
        <details class="disclosure">
          <summary><svg class="chev" viewBox="0 0 24 24" aria-hidden="true"><path d="m9 6 6 6-6 6"/></svg>Run setup again</summary>
          <div class="rerun">
            <button class="quiet" onclick={onRerunSetup}>Open setup</button>
          </div>
        </details>
      </section>
    </div>
  {/if}
    </div>
  </div>
</div>

<style>
  .settings{width:100%;flex:1;min-height:0;display:grid;grid-template-columns:224px minmax(0,1fr)}
  .settings-sidebar{display:flex;flex-direction:column;min-height:0;padding:30px 16px 20px;background:var(--paper);border-right:1px solid var(--line);overflow-y:auto}
  .settings-title{padding:0 14px 28px}
  .settings-title h1{font-size:23px;letter-spacing:-.8px;font-weight:620;margin:0 0 7px}
  .tabs{display:flex;flex-direction:column;gap:5px}
  .tabs button{position:relative;display:flex;align-items:center;gap:11px;width:100%;text-align:left;border:1px solid transparent;background:transparent;color:var(--muted);font-size:12.5px;font-weight:500;padding:12px 13px;border-radius:8px;min-height:44px}
  .tabs button:hover{color:var(--text);background:var(--soft)}
  .tabs button.selected{color:var(--text);background:var(--soft);border-color:var(--line-strong)}
  .tabs button.selected svg{color:var(--yellow)}
  .tabs svg{width:18px;height:18px;flex-shrink:0;fill:none;stroke:currentColor;stroke-width:1.6;stroke-linecap:round;stroke-linejoin:round}
  .update-dot{width:6px;height:6px;background:var(--yellow);border-radius:50%;margin-left:auto;flex-shrink:0}
  .sidebar-footer{margin-top:auto;padding:28px 14px 0;display:flex;align-items:center;gap:10px}
  .app-emblem{display:grid;place-items:center;width:30px;height:30px;border:1px solid var(--line-strong);border-radius:9px;font-size:23px;font-weight:650;color:var(--yellow);line-height:1}
  .sidebar-footer strong{display:block;font-size:12px;font-weight:550;color:var(--muted)}
  .sidebar-footer small{display:block;font-size:10.5px;color:var(--faint);margin-top:3px}
  .settings-content{min-width:0;min-height:0;display:flex;flex-direction:column;container:settings-content / inline-size}
  .section-header{flex-shrink:0;padding:28px 36px;border-bottom:1px solid var(--line)}
  .section-header h2{font-size:25px;letter-spacing:-.8px;font-weight:600;margin:0}
  .section-body{overflow-y:auto;min-height:0;flex:1;padding:28px 36px 40px;scrollbar-gutter:stable}
  .section-body:focus-visible{outline-offset:-3px}
  .tabpanel.cols{display:grid;grid-template-columns:minmax(0,1fr);gap:22px;align-items:start}
  .tabpanel .full{grid-column:1/-1}
  .tabpanel{display:block;max-width:1440px;margin:0 auto;width:100%}
  .group{min-width:0;background:var(--paper);border:1px solid var(--line);border-radius:12px;padding:22px 24px 6px}
  .group>h2{font-size:15px;font-weight:600;color:var(--text);margin:0 0 17px;letter-spacing:-.2px}
  .tool-total{font-size:12px;color:var(--muted);margin:-8px 0 18px}
  .after{font-size:11.5px;color:var(--muted);line-height:1.65;margin:0 0 17px;max-width:74ch}
  .rows{border-top:1px solid var(--line)}
  .row{display:flex;align-items:center;gap:20px;padding:15px 0;border-bottom:1px solid var(--line);min-height:64px;flex-wrap:wrap}
  .row:last-child{border-bottom:0}
  .what{flex:1 1 220px;min-width:0}
  .what strong{display:block;font-size:13px;font-weight:550}
  .what small{display:block;font-size:12px;color:var(--muted);margin-top:5px;line-height:1.6;max-width:66ch}
  .how{display:flex;align-items:center;justify-content:flex-end;gap:10px;flex:0 1 auto;margin-left:auto;max-width:100%;flex-wrap:wrap}
  .value{font-size:13px;color:var(--text);overflow-wrap:anywhere}
  .value.muted{color:var(--faint);font-size:12px}
  .quiet{border:1px solid var(--line-strong);background:transparent;color:var(--muted);padding:8px 13px;border-radius:8px;font-size:12.5px}
  .quiet:hover:not(:disabled){color:var(--text);background:var(--soft)}
  .inline{width:280px;padding:9px 12px;border:1px solid var(--line-strong);border-radius:8px;background:var(--soft);color:var(--text);font-size:14px}

  .seg{display:inline-flex;gap:3px;padding:3px;border:1px solid var(--line);background:var(--bg);border-radius:9px;max-width:100%;flex-wrap:wrap}
  .seg button{border:1px solid transparent;background:transparent;color:var(--muted);font-size:12px;padding:7px 12px;border-radius:6px}
  .seg button:hover{background:var(--paper);color:var(--text)}
  .seg button[aria-pressed="true"]{background:var(--soft);border-color:var(--line-strong);color:var(--text);box-shadow:0 1px 3px #0003}

  .switch{width:42px;height:24px;flex-shrink:0;border-radius:99px;border:1px solid var(--line-strong);background:var(--soft);position:relative;padding:0;transition:background .16s ease,border-color .16s ease}
  .switch::after{content:"";position:absolute;top:2px;left:2px;width:18px;height:18px;border-radius:50%;background:var(--faint);transition:transform .16s ease,background .16s ease}
  .switch[aria-checked="true"]{background:rgba(255,212,92,.16);border-color:rgba(255,212,92,.45)}
  .switch[aria-checked="true"]::after{transform:translateX(18px);background:var(--yellow)}
  .switch.small{width:36px;height:21px}
  .switch.small::after{width:15px;height:15px}
  .switch.small[aria-checked="true"]::after{transform:translateX(15px)}

  .state{font-size:12px;color:var(--faint)}
  .what small.bad{color:var(--error-text)}
  .notes{margin:8px 0 0;font-size:12px;line-height:1.6;color:var(--muted);white-space:pre-wrap;max-width:64ch}
  .progress{width:min(260px,40vw);height:6px;border-radius:99px;background:var(--soft);overflow:hidden;border:1px solid var(--line)}
  .progress span{display:block;height:100%;background:var(--yellow);transition:width .2s ease}
  .primary{background:var(--yellow);border:1px solid var(--yellow);color:var(--black);font-weight:600;padding:8px 14px;border-radius:8px;font-size:12.5px}
  .primary:hover:not(:disabled){background:#ffdd7f;border-color:#ffdd7f}
  .state.live{color:var(--yellow)}

  .disclosure{border-bottom:1px solid var(--line)}
  .tabpanel>.disclosure{border:1px solid var(--line);border-radius:12px;background:var(--paper);padding:0 24px}
  .disclosure summary{cursor:pointer;list-style:none;display:flex;align-items:center;gap:11px;padding:16px 0;font-size:13px;color:var(--muted)}
  .disclosure summary::-webkit-details-marker{display:none}
  .disclosure summary:hover{color:var(--text)}
  .chev{width:12px;height:12px;flex-shrink:0;stroke:currentColor;fill:none;stroke-width:1.8;stroke-linecap:round;stroke-linejoin:round;transition:transform .16s ease}
  .disclosure[open]>summary .chev{transform:rotate(90deg)}
  .count{margin-left:auto;font-size:11.5px;color:var(--faint)}
  .facts{display:grid;grid-template-columns:minmax(120px,200px) minmax(0,1fr);gap:14px 22px;font-size:12px;color:var(--muted);padding:4px 0 22px 24px;overflow-wrap:anywhere}
  .facts b{color:var(--text);font-weight:500}
  .facts b.bad{color:var(--error-text)}
  .facts b.muted{color:var(--faint);font-weight:400}

  .row-disclosure{border-bottom:1px solid var(--line)}
  .row-disclosure>summary{padding:15px 0}
  .summary-main{display:block;min-width:0}
  .summary-main strong{display:block;font-size:13.5px;font-weight:550;color:var(--text)}
  .tool-list{padding:0 0 14px 24px}
  .tool-row{display:flex;align-items:center;gap:20px;padding:12px 0;border-top:1px solid var(--line)}
  .tool-what{flex:1;min-width:0}
  .tool-what strong{display:block;font-size:13px;font-weight:500}
  .grant-list{padding:0 0 16px 24px}
  .grant{display:flex;align-items:flex-start;justify-content:space-between;gap:16px;padding:10px 0;border-top:1px solid var(--line)}
  .grant strong{display:block;font-size:12px;font-weight:550}
  .grant code{display:block;font-size:10.5px;color:var(--faint);margin-top:3px;overflow-wrap:anywhere}
  .errors{padding:0 0 16px 24px;font-size:12px;color:var(--muted);line-height:1.6}
  .errors p{margin:8px 0 0;overflow-wrap:anywhere}
  .rerun{padding:0 0 18px 24px}

  .settings :global(.providers),.settings :global(.tools-card){grid-column:1/-1;background:var(--paper);border:1px solid var(--line);border-radius:12px;padding:22px 24px;margin:0;min-width:0}
  .settings :global(.providers h2),.settings :global(.tools-card h2){font-size:15px;font-weight:600;letter-spacing:-.2px}
  .settings :global(.providers .field-row){grid-template-columns:repeat(2,minmax(0,1fr));gap:18px}
  .settings :global(.providers .field-row label:only-child){max-width:none}
  .settings :global(.providers .connection){background:var(--bg);border-radius:9px}
  .settings :global(.providers .connected-list){grid-template-columns:repeat(auto-fit,minmax(min(100%,360px),1fr));align-items:start}
  .settings :global(.providers .connected-list h3){grid-column:1/-1}
  .settings :global(.providers .connection-actions){flex-wrap:wrap}
  .settings :global(.integrations){grid-column:1/-1;max-width:none;gap:22px;align-items:start}
  .settings :global(.integration){max-width:none;width:100%;min-width:0;background:var(--paper);border-radius:12px}
  .settings :global(.integration .heading){padding:21px 24px}
  .settings :global(.integration .body){padding:22px 24px}
  .settings :global(.integration h2){font-size:16px;font-weight:600;letter-spacing:-.3px}
  .settings :global(.tools-card form){border:1px solid var(--line-strong);border-radius:9px;background:var(--bg);padding:18px}
  .settings :global(.tools-card .servers){margin-top:18px}

  @container settings-content (min-width:1100px){
    .tabpanel.cols{grid-template-columns:repeat(2,minmax(0,1fr))}
    .tabpanel.personalization{grid-template-columns:minmax(0,1.6fr) minmax(0,1fr)}
    .settings :global(.integrations){grid-template-columns:repeat(2,minmax(0,1fr))}
  }
  @container settings-content (max-width:640px){
    .how{justify-content:flex-start;margin-left:0;width:100%}
    .inline{width:100%}
    .row:has(.switch){flex-wrap:nowrap}
    .row:has(.switch) .how{width:auto;margin-left:auto;flex-shrink:0}
    .group{padding:19px 20px 4px}
    .facts{grid-template-columns:1fr;gap:4px}
    .facts span{margin-top:8px}
    .settings :global(.providers .field-row){grid-template-columns:minmax(0,1fr);gap:0}
    .settings :global(.providers .connection){align-items:flex-start;flex-direction:column;gap:12px}
    .settings :global(.providers .connection-actions){flex-shrink:1}
    .settings :global(.integration .heading){flex-wrap:wrap;padding:19px 20px}
    .settings :global(.integration .body){padding:20px}
    .settings :global(.integration .status){margin-left:55px}
    .settings :global(.integration .token-row){flex-direction:column}
    .settings :global(.integration .token-row button){align-self:flex-start}
  }
  @media(max-width:980px){
    .settings{grid-template-columns:200px minmax(0,1fr)}
    .settings-sidebar{padding:26px 10px 18px}
    .tabs button{font-size:12px;gap:9px;padding:11px 10px}
    .section-header{padding:27px 24px 23px}
    .section-body{padding:24px 24px 32px}
  }
  @media(max-width:700px){
    .settings{grid-template-columns:54px minmax(0,1fr)}
    .settings-sidebar{padding:18px 6px}
    .settings-title,.sidebar-footer{display:none}
    .tabs{gap:10px}
    .tabs button{justify-content:center;padding:12px 0}
    .tabs button>span:not(.update-dot){display:none}
    .update-dot{position:absolute;right:5px;top:6px}
    .section-header{padding:22px 18px 20px}
    .section-header h2{font-size:22px}
    .section-body{padding:18px 14px 28px}
    .tabpanel.cols{gap:16px}
    .tabpanel>.disclosure{padding:0 20px}
    .settings :global(.providers),.settings :global(.tools-card){padding:19px 20px}
  }
  @media(prefers-reduced-motion:reduce){*{transition:none!important}}
</style>
