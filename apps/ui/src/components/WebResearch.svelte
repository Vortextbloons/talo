<script lang="ts">
  import type { ResearchActivity } from '../../../../packages/contracts/src/chat';
  import { invoke } from '@tauri-apps/api/core';
  import { openBrowserSource } from '../lib/browser.svelte';
  import { sendMessage } from '../lib/chat.svelte';
  let { items, onCancel, working }: { items: ResearchActivity[]; onCancel: () => void; working: boolean } = $props();
  let openError = $state('');
  const label = (tool: string) => tool.startsWith('browser_') ? 'Browser · ' + tool.slice(8).replaceAll('_', ' ') : tool === 'web_search' ? 'Searching' : tool === 'web_crawl' ? 'Exploring website' : tool === 'web_screenshot' ? 'Capturing page' : tool === 'web_fetch' ? 'Reading' : tool === 'skill' ? 'Used skill' : tool.replaceAll('_', ' ');
  const running = $derived(working && items.some(item => item.state === 'running'));
  const pages = $derived(new Set(items.filter(item => ['browser_open','web_fetch','web_crawl'].includes(item.tool)).map(item => item.input)).size);
  const failed = $derived(items.filter(item => item.state === 'failed').length);
  async function openArtifact(name: string) {
    try { await invoke('research_artifact_open', { name, browser: items.some(item => item.tool.startsWith('browser_') && item.artifact === name) }); }
    catch (error) { openError = String(error); }
  }
  async function openSource(event: MouseEvent, url: string) {
    event.preventDefault();
    try { await openBrowserSource(url); }
    catch (error) { openError = String(error); }
  }
</script>

<details class="research" class:active={running} aria-label="Turn activity">
  <summary class="bundle-summary"><span class="bundle-marker" aria-hidden="true">↗</span><span>{running ? 'Working' : pages ? `Browsed ${pages} ${pages === 1 ? 'page' : 'pages'}` : `${items.length} ${items.length === 1 ? 'action' : 'actions'}`}</span><span class="bundle-meta">{failed ? `${failed} failed` : `${items.length} steps`}</span></summary>
  <div class="bundle-body">
  {#each items as item (item.id)}
    <details class:running={item.state === 'running' && working}>
      <summary>
        <span class="marker" aria-hidden="true">↗</span>
        <span>{label(item.tool)} <span class="input">{item.input}</span></span>
        <span class="status">{item.state === 'running' && !working ? 'Stopped' : item.state}</span>
      </summary>
      <div class="body">
        {#if item.error}<p class="error">{item.error}</p>{/if}
        {#if item.results?.length}
          {#each item.results as result (result.url)}
            <article><a href={result.url} onclick={(event) => openSource(event, result.url)}>{result.title || result.url}</a><p>{result.snippet}</p></article>
          {/each}
        {:else if item.preview}<pre>{item.preview}</pre>{/if}
        {#if item.sources?.length}
          <ul>{#each item.sources as source (source)}<li><a href={source} onclick={(event) => openSource(event, source)} rel="noopener noreferrer">{source}</a></li>{/each}</ul>
        {/if}
        {#if item.artifact}<button onclick={() => openArtifact(item.artifact!)}>Open saved {item.artifact.endsWith('.png') ? 'screenshot' : 'full result'}</button>{/if}
        {#if item.state === 'running' && working}<button onclick={onCancel}>Stop activity</button>{/if}
        {#if item.state === 'failed' || item.state === 'cancelled'}
          <button disabled={working} onclick={() => void sendMessage(`Retry the ${item.tool} request for: ${item.input}`)}>Retry request</button>
        {/if}
      </div>
    </details>
  {/each}
  {#if openError}<p class="error">{openError}</p>{/if}
  </div>
</details>

<style>
  .research { margin: 10px 0; border:1px solid var(--line);border-radius:9px;overflow:hidden;font-size: 12px; }
  .bundle-summary{padding:9px 11px;color:var(--muted);font-size:11px;gap:7px;list-style:none}.bundle-summary::after{content:'⌄';color:var(--faint);margin-left:2px}.research[open]>.bundle-summary::after{transform:rotate(180deg)}.bundle-marker{font-size:13px;color:var(--faint)}.active>.bundle-summary .bundle-marker{color:var(--yellow)}.bundle-meta{margin-left:auto;font-size:10px;color:var(--faint)}.bundle-body{border-top:1px solid var(--line)}
  details { background: var(--paper); color: var(--text); border-bottom: 1px solid var(--line); }
  summary { cursor: pointer; display: flex; align-items: baseline; gap: 8px; padding: 10px 12px; }
  summary:focus-visible, a:focus-visible, button:focus-visible { outline: 2px solid var(--yellow); outline-offset: 3px; }
  .marker { color: var(--yellow); } .input { color: var(--muted); overflow-wrap: anywhere; }
  .status { margin-left: auto; color: var(--muted); font-size: 10px; text-transform: uppercase; }
  .running .status { color: var(--yellow); }
  .body { padding: 0 12px 12px; } pre { white-space: pre-wrap; overflow-wrap: anywhere; max-height: 210px; overflow: auto; font: inherit; line-height: 1.6; }
  ul { padding-left: 16px; } li { margin: 7px 0; } a { color: var(--yellow); overflow-wrap: anywhere; }
  button { background: var(--yellow); color: var(--black); border: 0; padding: 7px 10px; cursor: pointer; margin: 4px 8px 0 0; border-radius: 4px; }
  .error { color: var(--error-text); }
  article { padding: 9px 0; border-bottom: 1px solid var(--line); } article p { color: var(--muted); margin: 5px 0; line-height: 1.5; }
</style>
