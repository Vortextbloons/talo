<script lang="ts">
  import { onMount } from 'svelte';
  import { isTauri } from '@tauri-apps/api/core';
  import ProviderSettings from './ProviderSettings.svelte';
  import ToolsSettings from './ToolsSettings.svelte';
  import { chat, selectionLabel } from '../lib/chat.svelte';
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

  const TABS = ['You', 'Models', 'Capabilities', 'Advanced'] as const;
  type Tab = typeof TABS[number];
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

  const VOICES: Array<{ value: string; label: string }> = [
    { value: 'Thoughtful and direct', label: 'Thoughtful' },
    { value: 'Warm and encouraging', label: 'Warm' },
    { value: 'Brief and practical', label: 'Brief' },
  ];

  let editingName = $state(false);
  let draftName = $state('');
  function beginName(): void { draftName = settings.name ?? ''; editingName = true; }
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
  };
  const book = (id: string): string => TOOL_LABELS[id] ?? id.replace(/[_-]+/gu, ' ').replace(/^./u, c => c.toUpperCase());

  const CATEGORY_COPY: Record<string, { label: string; hint: string }> = {
    'Web Research': { label: 'Looking things up', hint: 'Search, read pages, and cite real sources.' },
    'Browser': { label: 'Its own browser', hint: 'A private browser Talo drives for you. Changes ask first.' },
    'Files': { label: 'Your files', hint: 'Only files you pick, one action at a time, with approval.' },
    'Integrations': { label: 'Connected apps', hint: 'Apps you add below. They run on this PC.' },
    'Memory': { label: 'Memory', hint: 'The things worth keeping between conversations.' },
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
      .map(([category, list]) => ({ category, ...(CATEGORY_COPY[category] ?? { label: category, hint: '' }), tools: list }));
  });
  const enabledTools = $derived(tools.state.tools.filter(tool => tool.availability !== 'disabled').length);

  const DEPENDENCIES: Record<string, string> = {
    runtime: 'Assistant engine', chromium: 'Browser engine', donsetch: 'Web research engine',
    pi: 'Assistant files', stagehand: 'Browser intelligence', mcpAdapter: 'App connections',
  };
  const serviceState = (id: ServiceId): { text: string; running: boolean; note: string } => {
    const value = status?.services?.[id];
    if (!status) return { text: 'Desktop app required', running: false, note: '' };
    if (value === 'running') return { text: 'Running', running: true, note: id === 'pi' ? 'Awake while you talk, then it rests.' : 'Open only while the browser is in use.' };
    if (value === 'failed') return { text: 'Stopped unexpectedly', running: false, note: 'Open the app again to restart it.' };
    return { text: id === 'pi' ? 'Resting' : 'Sleeping', running: false, note: id === 'pi' ? 'Starts when you send a message.' : 'Starts when you open the browser.' };
  };
  const pi = $derived(serviceState('pi'));
  const browser = $derived(serviceState('browser'));

  let appVersion = $state('');
  onMount(() => {
    void refreshTools();
    if (isTauri()) void import('@tauri-apps/api/app').then(({ getVersion }) => getVersion()).then(version => { appVersion = version; }).catch(() => {});
    // Opening the desktop app's settings is a good moment to ask the release feed.
    void checkForUpdates({ quiet: true });
  });

  const megabytes = (bytes: number): string => (bytes / (1024 * 1024)).toFixed(1);
  const downloaded = $derived(updates.total ? Math.min(100, Math.round((updates.received / updates.total) * 100)) : 0);
  const updateState = $derived(
    updates.phase === 'available' ? 'A new version is ready to download.'
    : updates.phase === 'checking' ? 'Checking for updates…'
    : updates.phase === 'current' ? 'Talo is up to date.'
    : updates.phase === 'preparing' ? 'Closing background services…'
    : updates.phase === 'downloading' ? `Downloading ${megabytes(updates.received)}${updates.total ? ` of ${megabytes(updates.total)} MB` : ''} MB…`
    : updates.phase === 'installing' ? 'Installing. Talo will close and reopen itself.'
    : updates.phase === 'error' ? updates.error
    : 'Talo checks quietly when you open the desktop app.'
  );

  async function toggleTool(id: string, enabled: boolean): Promise<void> {
    await toolAction('toggle', { id, enabled });
  }
</script>

<div class="settings">
  <div class="tabs" role="tablist" aria-label="Settings sections">
    {#each TABS as item (item)}
      <button role="tab" aria-selected={tab === item} class:selected={tab === item} onclick={() => selectTab(item)} onkeydown={(event) => {
        if (event.key !== 'ArrowRight' && event.key !== 'ArrowLeft') return;
        event.preventDefault();
        const index = TABS.indexOf(item) + (event.key === 'ArrowRight' ? 1 : TABS.length - 1);
        selectTab(TABS[index % TABS.length]);
      }}>{item}</button>
    {/each}
  </div>

  {#if tab === 'You'}
    <div class="tabpanel cols" role="tabpanel" aria-label="You">
      <section class="group">
        <h2>How it talks to you</h2>
        <div class="rows">
          <div class="row">
            <div class="what"><strong>Your name</strong><small>Used when Talo greets you.</small></div>
            <div class="how">
              {#if editingName}
                <input class="inline" bind:value={draftName} maxlength="80" placeholder="What should Talo call you?" onkeydown={nameKey} onblur={() => void commitName()} aria-label="Your name" />
              {:else}
                <span class="value">{settings.name || 'Not set'}</span>
                <button class="quiet" onclick={beginName}>Change</button>
              {/if}
            </div>
          </div>
          <div class="row">
            <div class="what"><strong>Voice</strong><small>Its default tone in every reply.</small></div>
            <div class="how">
              <div class="seg" role="group" aria-label="Voice">
                {#each VOICES as voice (voice.value)}
                  <button aria-pressed={settings.personality === voice.value} onclick={() => void onPersist({ personality: voice.value })}>{voice.label}</button>
                {/each}
              </div>
            </div>
          </div>
          <div class="row">
            <div class="what"><strong>Show its thinking</strong><small>See the working, not just the answer.</small></div>
            <div class="how">
              <button class="switch" role="switch" aria-checked={settings.showReasoning === true} aria-label="Show its thinking" onclick={() => void onPersist({ showReasoning: !(settings.showReasoning === true) })}></button>
            </div>
          </div>
        </div>
      </section>

      <section class="group">
        <h2>Memory</h2>
        <div class="rows">
          <div class="row">
            <div class="what"><strong>Remember between chats</strong><small>Kept on this PC. Nothing leaves the machine.</small></div>
            <div class="how">
              <button class="switch" role="switch" aria-checked={library.memoryEnabled} aria-label="Remember between chats" onclick={() => void onMemory(!library.memoryEnabled)}></button>
            </div>
          </div>
        </div>
        <p class="after">Talo surfaces what it remembers in your chats, when it is relevant.</p>
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
    <div class="tabpanel cols" role="tabpanel" aria-label="Models">
      <ProviderSettings />
      <section class="group full">
        <h2>Which model answers</h2>
        <div class="rows">
          <div class="row">
            <div class="what"><strong>Chat</strong><small>Used for every normal reply.</small></div>
            <div class="how">
              <span class="value">{selection || 'Chosen when you send your first message'}</span>
              <button class="quiet" onclick={onOpenChat}>Choose in Chat</button>
            </div>
          </div>
        </div>
      </section>
    </div>
  {/if}

  {#if tab === 'Capabilities'}
    <div class="tabpanel cols" role="tabpanel" aria-label="Capabilities">
      <section class="group full">
        <h2>What it can do</h2>
        <p class="hint">{tools.state.tools.length ? `${enabledTools} of ${tools.state.tools.length} abilities are on. Anything that touches the outside world asks you first.` : 'These load with the desktop app. Anything that touches the outside world asks you first.'}</p>
        <div class="rows">
          {#each groups as group (group.category)}
            <details class="disclosure row-disclosure">
              <summary>
                <svg class="chev" viewBox="0 0 24 24" aria-hidden="true"><path d="m9 6 6 6-6 6"/></svg>
                <span class="summary-main"><strong>{group.label}</strong><small>{group.hint}</small></span>
                <span class="count">{group.tools.filter(tool => tool.availability !== 'disabled').length} of {group.tools.length} on</span>
              </summary>
              <div class="tool-list">
                {#each group.tools as tool (tool.id)}
                  <div class="tool-row">
                    <div class="tool-what"><strong>{book(tool.id)}</strong><small>{tool.description}</small></div>
                    <button class="switch small" role="switch" aria-checked={tool.availability !== 'disabled'} aria-label={book(tool.id)} onclick={() => void toggleTool(tool.id, tool.availability === 'disabled')}></button>
                  </div>
                {/each}
              </div>
            </details>
          {/each}
        </div>
      </section>

      <ToolsSettings />

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

  {#if tab === 'Advanced'}
    <div class="tabpanel cols" role="tabpanel" aria-label="Advanced">
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
          <p class="after">Talo closes to install and reopens on the new version. Your chats, settings and memory are untouched.</p>
        {/if}
      </section>

      <section class="group">
        <h2>App behaviour</h2>
        <div class="rows">
          <div class="row">
            <div class="what"><strong>Start with Windows</strong><small>Wait quietly in the tray after you sign in.</small></div>
            <div class="how">
              <button class="quiet" disabled={busy} onclick={() => void onAutostart(true)}>Turn on</button>
              <button class="quiet" disabled={busy} onclick={() => void onAutostart(false)}>Turn off</button>
            </div>
          </div>
          <div class="row">
            <div class="what"><strong>Closing the window</strong><small>Closing hides Talo in the tray. Quit Talo from the tray menu to exit.</small></div>
            <div class="how"><span class="value muted">Always on</span></div>
          </div>
        </div>
      </section>

      <section class="group">
        <h2>Background services</h2>
        <p class="hint">You never have to touch these. They start on demand and rest when idle.</p>
        <div class="rows">
          <div class="row">
            <div class="what"><strong>Assistant engine</strong><small>{pi.note}</small></div>
            <div class="how">
              <span class="state" class:live={pi.running}>{pi.text}</span>
              <button class="quiet" disabled={busy || !status} onclick={() => void onService('pi', !pi.running)}>{pi.running ? 'Stop' : 'Start'}</button>
            </div>
          </div>
          <div class="row">
            <div class="what"><strong>Browser engine</strong><small>{browser.note}</small></div>
            <div class="how">
              <span class="state" class:live={browser.running}>{browser.text}</span>
              <button class="quiet" disabled={busy || !status} onclick={() => void (browser.running ? onService('browser', false) : onBrowserStart())}>{browser.running ? 'Stop' : 'Start'}</button>
            </div>
          </div>
        </div>
      </section>

      <section class="group full">
        <h2>Technical details</h2>
        <p class="hint">Only useful when something looks broken.</p>
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
            <p class="hint">Reopens the two-step introduction. Your chats, connections and memory stay as they are.</p>
            <button class="quiet" onclick={onRerunSetup}>Open setup</button>
          </div>
        </details>
      </section>
    </div>
  {/if}
</div>

<style>
  .settings{width:100%;max-width:none}
  .tabpanel.cols{display:grid;gap:36px 52px;align-items:start}
  .tabpanel .full{grid-column:1/-1}
  @media(min-width:1120px){.tabpanel.cols{grid-template-columns:repeat(2,minmax(0,1fr))}}
  .tabs{display:flex;gap:0;border-bottom:1px solid var(--line);margin:22px 0 32px;flex-wrap:wrap}
  .tabs button{position:relative;border:0;background:transparent;color:var(--muted);font-size:14px;padding:0 2px 13px;margin-right:32px;border-radius:0}
  .tabs button:hover{color:var(--text);background:transparent}
  .tabs button.selected{color:var(--text)}
  .tabs button.selected::after{content:"";position:absolute;left:0;right:0;bottom:-1px;height:2px;background:var(--yellow);border-radius:2px 2px 0 0}

  .tabpanel{display:block}
  .group>h2{font-size:13.5px;font-weight:600;color:var(--text);margin:0 0 5px;letter-spacing:.01em}
  .hint{font-size:12.5px;color:var(--faint);line-height:1.65;margin:4px 0 14px;max-width:74ch}
  .after{font-size:12px;color:var(--faint);line-height:1.65;margin:12px 0 0;max-width:74ch}
  .rows{border-top:1px solid var(--line)}
  .row{display:flex;align-items:center;gap:28px;padding:17px 0;border-bottom:1px solid var(--line);min-height:66px;flex-wrap:wrap}
  .what{flex:1 1 280px;min-width:0}
  .what strong{display:block;font-size:14px;font-weight:550}
  .what small{display:block;font-size:12px;color:var(--faint);margin-top:4px;line-height:1.6}
  .how{display:flex;align-items:center;justify-content:flex-end;gap:10px;flex:0 0 auto;margin-left:auto}
  .value{font-size:14px;color:var(--text)}
  .value.muted{color:var(--faint);font-size:12px}
  .quiet{border:1px solid var(--line-strong);background:transparent;color:var(--muted);padding:8px 13px;border-radius:8px;font-size:12.5px}
  .quiet:hover:not(:disabled){color:var(--text);background:var(--soft)}
  .inline{width:280px;padding:9px 12px;border:1px solid var(--line-strong);border-radius:8px;background:var(--soft);color:var(--text);font-size:14px}

  .seg{display:inline-flex;border:1px solid var(--line-strong);border-radius:9px;overflow:hidden}
  .seg button{border:0;background:transparent;color:var(--muted);font-size:12.5px;padding:9px 16px;border-radius:0}
  .seg button:hover{background:#232320;color:var(--text)}
  .seg button[aria-pressed="true"]{background:var(--soft);color:var(--text)}

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
  .disclosure summary{cursor:pointer;list-style:none;display:flex;align-items:center;gap:11px;padding:16px 0;font-size:13px;color:var(--muted)}
  .disclosure summary::-webkit-details-marker{display:none}
  .disclosure summary:hover{color:var(--text)}
  .chev{width:12px;height:12px;flex-shrink:0;stroke:currentColor;fill:none;stroke-width:1.8;stroke-linecap:round;stroke-linejoin:round;transition:transform .16s ease}
  .disclosure[open]>summary .chev{transform:rotate(90deg)}
  .count{margin-left:auto;font-size:11.5px;color:var(--faint)}
  .facts{display:grid;grid-template-columns:240px 1fr;gap:11px 22px;font-size:12.5px;color:var(--muted);padding:0 0 20px 24px;max-width:1000px}
  .facts b{color:var(--text);font-weight:500}
  .facts b.bad{color:var(--error-text)}
  .facts b.muted{color:var(--faint);font-weight:400}

  .row-disclosure{border-bottom:1px solid var(--line)}
  .row-disclosure>summary{padding:15px 0}
  .summary-main{display:block;min-width:0}
  .summary-main strong{display:block;font-size:13.5px;font-weight:550;color:var(--text)}
  .summary-main small{display:block;font-size:12px;color:var(--faint);margin-top:4px;line-height:1.55}
  .tool-list{padding:0 0 14px 24px}
  .tool-row{display:flex;align-items:center;gap:20px;padding:12px 0;border-top:1px solid var(--line)}
  .tool-what{flex:1;min-width:0}
  .tool-what strong{display:block;font-size:13px;font-weight:500}
  .tool-what small{display:block;font-size:11.5px;color:var(--faint);margin-top:3px;line-height:1.55;overflow:hidden;display:-webkit-box;-webkit-line-clamp:2;line-clamp:2;-webkit-box-orient:vertical}
  .grant-list{padding:0 0 16px 24px}
  .grant{display:flex;align-items:flex-start;justify-content:space-between;gap:16px;padding:10px 0;border-top:1px solid var(--line)}
  .grant strong{display:block;font-size:12px;font-weight:550}
  .grant code{display:block;font-size:10.5px;color:var(--faint);margin-top:3px;overflow-wrap:anywhere}
  .errors{padding:0 0 16px 24px;font-size:12px;color:var(--muted);line-height:1.6}
  .errors p{margin:8px 0 0;overflow-wrap:anywhere}
  .rerun{padding:0 0 18px 24px}
  .rerun .hint{margin:0 0 10px}

  /* The embedded provider and tool sections carry their own card chrome. */
  :global(.settings .providers),:global(.settings .tools-card){background:transparent;border:0;border-radius:0;padding:0;margin-bottom:26px;border-top:1px solid var(--line);padding-top:16px}
  :global(.settings .providers h2),:global(.settings .tools-card h2){font-size:12.5px;font-weight:600;letter-spacing:.01em}
  :global(.settings .providers .intro){font-size:12px;color:var(--faint)}
  :global(.settings .providers .empty-line){background:transparent;border:0;border-radius:0;padding:14px 0;border-bottom:1px solid var(--line)}
  :global(.settings .providers .connection){background:transparent;border:0;border-bottom:1px solid var(--line);border-radius:0;padding:13px 0}
  :global(.settings .providers .connected-list){margin-top:6px;gap:0}
  :global(.settings .providers .connect-form){border-top:0;margin-top:0;padding-top:0}
  :global(.settings .providers .field-row){grid-template-columns:repeat(auto-fit,minmax(260px,460px));justify-content:start}
  :global(.settings .providers .connect-form label){max-width:560px}
  :global(.settings .providers .field-row label:only-child){max-width:none}
  :global(.settings .providers){grid-column:1/-1}
  :global(.settings .providers .connect-actions small){max-width:56ch}
  :global(.settings .tools-card){grid-column:1/-1}
  :global(.settings .tools-card .eyebrow){display:none}

  @media(max-width:760px){
    .how{justify-content:flex-start;margin-left:0;width:100%}
    .inline{width:100%}
    .facts{grid-template-columns:1fr;gap:4px}
    .facts span{margin-top:8px}
  }
  @media(prefers-reduced-motion:reduce){*{transition:none!important}}
</style>
