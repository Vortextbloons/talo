<script module lang="ts">
  let markdownModule: Promise<typeof import('../lib/markdown')> | undefined;
  let katexModule: Promise<typeof import('katex')> | undefined;
  let katexRenderer: typeof import('katex').default | undefined;
</script>

<script lang="ts">
  import { onDestroy } from 'svelte';
  import { isTauri } from '@tauri-apps/api/core';
  import { openBrowserSource } from '../lib/browser.svelte';
  import type { Align, Block, Inline, ListItem } from '../lib/markdown';

  let { text }: { text: string } = $props();








  const PARSE_INTERVAL = 150;
  let tree = $state.raw<Block[] | null>(null);
  let mathReady = $state(false);
  let lastParse = 0;

  $effect(() => {
    const source = text;
    const since = Date.now() - lastParse;
    let cancelled = false;
    let frame = 0;
    const timer = setTimeout(() => {
      frame = requestAnimationFrame(() => {
        void (async () => {
          const module = await (markdownModule ??= import('../lib/markdown'));
          const parsed = module.parseMarkdown(source);
          if (cancelled) return;
          lastParse = Date.now();
          tree = parsed;
          if (containsMath(parsed)) {
            const math = await (katexModule ??= import('katex'));
            if (!cancelled) {
              katexRenderer = math.default;
              mathReady = true;
            }
          }
        })();
      });
    }, Math.max(0, PARSE_INTERVAL - since));
    return () => { cancelled = true; clearTimeout(timer); cancelAnimationFrame(frame); };
  });

  function containsMath(blocks: Block[]): boolean {
    return blocks.some(block => {
      if (block.kind === 'math') return true;
      if (block.kind === 'paragraph' || block.kind === 'heading') return inlineHasMath(block.children);
      if (block.kind === 'quote') return containsMath(block.blocks);
      if (block.kind === 'list') return block.items.some(item => containsMath(item.blocks));
      if (block.kind === 'table') return block.header.some(inlineHasMath) || block.rows.some(row => row.some(inlineHasMath));
      return false;
    });
  }

  function inlineHasMath(nodes: Inline[]): boolean {
    return nodes.some(node => node.kind === 'math' || ('children' in node && inlineHasMath(node.children)));
  }

  function renderMath(tex: string, display: boolean): string {
    if (!mathReady || !katexRenderer) return `<code>${tex.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')}</code>`;
    try {
      return katexRenderer.renderToString(tex, { displayMode: display, throwOnError: false, strict: 'ignore', trust: false });
    } catch {
      return `<code>${tex.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')}</code>`;
    }
  }


  let copied = $state('');
  let copyTimer: ReturnType<typeof setTimeout> | undefined;

  onDestroy(() => clearTimeout(copyTimer));

  function legacyCopy(value: string): boolean {
    const area = document.createElement('textarea');
    area.value = value;
    area.setAttribute('readonly', '');
    area.style.position = 'fixed';
    area.style.opacity = '0';
    document.body.appendChild(area);
    area.select();
    let ok = false;
    try {
      ok = document.execCommand('copy');
    } catch {
      ok = false;
    }
    area.remove();
    return ok;
  }

  async function copyLink(event: MouseEvent, href: string): Promise<void> {
    event.preventDefault();
    if (isTauri() && /^https?:\/\//iu.test(href)) {
      try { await openBrowserSource(href); return; }
      catch { /* copy remains available if the in-app browser is unavailable */ }
    }
    let ok = false;
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(href);
        ok = true;
      }
    } catch {
      ok = false;
    }
    if (!ok) ok = legacyCopy(href);
    if (!ok) return;
    copied = href;
    clearTimeout(copyTimer);
    copyTimer = setTimeout(() => { copied = ''; }, 1600);
  }

  function linkTitle(href: string, title: string): string {
    return `${title ? `${title} · ` : ''}${href} · ${isTauri() && /^https?:\/\//iu.test(href) ? 'open source' : 'click to copy'}`;
  }

  function alignOf(align: Align[] | undefined, index: number): string {
    return `align-${align?.[index] ?? 'left'}`;
  }
</script>

{#snippet mathHtml(tex: string, display: boolean)}

  {@html renderMath(tex, display)}
{/snippet}

{#snippet inline(nodes: Inline[])}
  {#each nodes as node, index (index)}
    {#if node.kind === 'text'}{node.text}
    {:else if node.kind === 'strong'}<strong>{@render inline(node.children)}</strong>
    {:else if node.kind === 'em'}<em>{@render inline(node.children)}</em>
    {:else if node.kind === 'del'}<del>{@render inline(node.children)}</del>
    {:else if node.kind === 'code'}<code class="md-code">{node.text}</code>
    {:else if node.kind === 'math'}
      {#if node.display}<span class="md-math display">{@render mathHtml(node.tex, true)}</span>
      {:else}<span class="md-math">{@render mathHtml(node.tex, false)}</span>{/if}
    {:else if node.kind === 'break'}<br />
    {:else if node.kind === 'link'}
      <a
        class="md-link"
        class:copied={copied === node.href}
        href={node.href}
        title={linkTitle(node.href, node.title)}
        onclick={(event) => { void copyLink(event, node.href); }}
      >{@render inline(node.children)}</a>
    {:else if node.kind === 'image'}
      <a
        class="md-image"
        class:copied={copied === node.href}
        href={node.href}
        title={linkTitle(node.href, node.title)}
        onclick={(event) => { void copyLink(event, node.href); }}
      ><span class="md-image-mark" aria-hidden="true">image</span>{node.alt}</a>
    {/if}
  {/each}
{/snippet}

{#snippet blocks(nodes: Block[])}
  {#each nodes as node, index (index)}
    {#if node.kind === 'paragraph'}
      <p>{@render inline(node.children)}</p>
    {:else if node.kind === 'heading'}
      <div class="md-heading h{node.depth}" role="heading" aria-level={node.depth}>{@render inline(node.children)}</div>
    {:else if node.kind === 'code'}
      <div class="md-code-block">
        {#if node.language}<span class="md-code-language">{node.language}</span>{/if}
        <pre class="md-pre"><code>{node.text}</code></pre>
      </div>
    {:else if node.kind === 'math'}
      <div class="md-math display">{@render mathHtml(node.tex, true)}</div>
    {:else if node.kind === 'quote'}
      <blockquote>{@render blocks(node.blocks)}</blockquote>
    {:else if node.kind === 'list'}
      {#if node.ordered}
        <ol class="md-list" start={node.start}>{@render listItems(node.items, node.loose)}</ol>
      {:else}
        <ul class="md-list">{@render listItems(node.items, node.loose)}</ul>
      {/if}
    {:else if node.kind === 'table'}
      <div class="md-table">
        <table>
          <thead><tr>{#each node.header as cell, column (column)}<th class={alignOf(node.align, column)}>{@render inline(cell)}</th>{/each}</tr></thead>
          <tbody>
            {#each node.rows as row, rowIndex (rowIndex)}
              <tr>{#each row as cell, column (column)}<td class={alignOf(node.align, column)}>{@render inline(cell)}</td>{/each}</tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else if node.kind === 'rule'}
      <hr />
    {:else if node.kind === 'raw'}
      <p class="md-raw">{node.text}</p>
    {/if}
  {/each}
{/snippet}

{#snippet listItems(entries: ListItem[], loose: boolean)}
  {#each entries as entry, index (index)}
    <li class:task={entry.task} class:loose={loose}>
      {#if entry.task}
        <span
          class="md-task"
          class:checked={entry.checked}
          role="img"
          aria-label={entry.checked ? 'Checked' : 'Unchecked'}
        >{entry.checked ? '✓' : ''}</span>
      {/if}
      {@render blocks(entry.blocks)}
    </li>
  {/each}
{/snippet}

{#if tree}
  <div class="markdown">{@render blocks(tree)}</div>
{:else}
  <div class="markdown"><p class="md-raw">{text}</p></div>
{/if}

<style>
  .markdown{display:flex;flex-direction:column;gap:11px;min-width:0;font-size:15px;line-height:1.75;overflow-wrap:anywhere}
  .markdown p{margin:0}
  .md-heading{font-weight:600;line-height:1.4;letter-spacing:-.01em;margin-top:2px}
  .md-heading.h1{font-size:18px;letter-spacing:-.02em}
  .md-heading.h2{font-size:16.5px}
  .md-heading.h3{font-size:15.5px}
  .md-heading.h4,.md-heading.h5,.md-heading.h6{font-size:14.5px;color:var(--muted)}
  .md-heading.h5,.md-heading.h6{font-size:14px;text-transform:none}
  .md-link{color:var(--yellow);text-decoration:underline;text-decoration-color:color-mix(in srgb,var(--accent) 40%,transparent);text-underline-offset:2px;cursor:copy}
  .md-link:hover{text-decoration-color:var(--yellow)}
  .md-link.copied{text-decoration-color:var(--yellow)}
  .md-link.copied::after{content:' · copied';color:var(--faint);font-size:11px;text-decoration:none;white-space:nowrap}
  .md-image{display:inline-flex;align-items:center;gap:5px;color:var(--yellow);text-decoration:underline;text-decoration-color:color-mix(in srgb,var(--accent) 40%,transparent);text-underline-offset:2px;cursor:copy}
  .md-image:hover{text-decoration-color:var(--yellow)}
  .md-image-mark{font-size:9.5px;letter-spacing:.08em;text-transform:uppercase;color:var(--faint)}
  .md-image.copied::after{content:' · copied';color:var(--faint);font-size:11px;text-decoration:none}
  .md-code{background:var(--soft);border:1px solid var(--line);border-radius:5px;padding:1px 5px;font-family:ui-monospace,'Cascadia Code',Consolas,monospace;font-size:13px;color:var(--text);white-space:break-spaces}
  .md-code-block{display:flex;flex-direction:column;gap:0;background:var(--sidebar);border:1px solid var(--line);border-radius:10px;overflow:hidden}
  .md-code-language{font-size:9.5px;letter-spacing:.08em;text-transform:uppercase;color:var(--faint);padding:7px 14px 0;font-family:ui-monospace,'Cascadia Code',Consolas,monospace}
  .md-code-block .md-pre{margin:0;border:0;border-radius:0;background:transparent;padding:12px 16px 14px}
  .md-pre{padding:14px 16px;background:var(--sidebar);border:1px solid var(--line);border-radius:10px;overflow-x:auto;font-family:ui-monospace,'Cascadia Code',Consolas,monospace;font-size:13px;line-height:1.65;color:var(--text);white-space:pre}
  .md-pre code{font:inherit}
  blockquote{margin:0;padding:2px 0 2px 14px;border-left:2px solid var(--line-strong);color:var(--muted)}
  blockquote > * + *{margin-top:8px}
  .md-list{margin:0;padding-left:22px;display:flex;flex-direction:column;gap:4px}
  .md-list .md-list{margin-top:4px;gap:3px}
  .md-list li::marker{color:var(--faint)}
  .md-list li p{margin:0}
  .md-list li.loose > * + *{margin-top:6px}
  .md-list li.task{list-style:none;position:relative;padding-left:22px}
  .md-task{position:absolute;left:0;top:5px;width:14px;height:14px;border:1px solid var(--line-strong);border-radius:4px;font-size:10px;line-height:13px;text-align:center;color:var(--black)}
  .md-task.checked{background:var(--yellow);border-color:var(--yellow)}
  .md-table{min-width:0;overflow-x:auto;border:1px solid var(--line);border-radius:10px}
  .md-table table{border-collapse:collapse;width:100%;font-size:13.5px;line-height:1.6}
  .md-table th,.md-table td{padding:7px 12px;border-bottom:1px solid var(--line);text-align:left;vertical-align:top;overflow-wrap:break-word}
  .md-table th{font-weight:600;color:var(--muted);background:var(--soft);white-space:nowrap}
  .md-table tr:last-child td{border-bottom:0}
  .md-table .align-center{text-align:center}
  .md-table .align-right{text-align:right}
  hr{border:0;border-top:1px solid var(--line);margin:2px 0}
  .md-math{color:var(--text)}
  .md-math.display{display:block;overflow-x:auto;overflow-y:hidden;padding:2px 0;text-align:center}
  .md-math.display :global(.katex-display){margin:4px 0}
  .md-math :global(.katex){font-size:1.04em}
  .md-raw{font-family:ui-monospace,'Cascadia Code',Consolas,monospace;font-size:13px;color:var(--muted);white-space:pre-wrap}
  strong{font-weight:650}
  em{font-style:italic}
  del{color:var(--faint)}
  .markdown :is(p,li,blockquote,td,th) .md-link:hover{color:var(--accent-bright)}
</style>
