<script lang="ts">
  import type { ResearchActivity } from '../../../../packages/contracts/src/chat';
  import { invoke } from '@tauri-apps/api/core';
  import { sendMessage } from '../lib/chat.svelte';
  let { items, onCancel, working }: { items: ResearchActivity[]; onCancel: () => void; working: boolean } = $props();
  let openError = $state('');
  const label = (tool: string) => tool.startsWith('browser_') ? 'Browser ? ' + tool.slice(8).replaceAll('_', ' ') : tool === 'web_search' ? 'Searching' : tool === 'web_crawl' ? 'Exploring website' : tool === 'web_screenshot' ? 'Capturing page' : 'Reading';
  async function openArtifact(name: string) {
    try { await invoke('research_artifact_open', { name, browser: items.some(item => item.tool.startsWith('browser_') && item.artifact === name) }); }
    catch (error) { openError = String(error); }
  }
  async function openSource(event: MouseEvent, url: string) {
    event.preventDefault();
    try { await invoke('research_source_open', { url }); }
    catch (error) { openError = String(error); }
  }
</script>

<section class="research" aria-label="Web and browser activity">
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
</section>

<style>
  .research { margin: 12px 0; border-left: 3px solid var(--yellow); font-size: 12px; }
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
