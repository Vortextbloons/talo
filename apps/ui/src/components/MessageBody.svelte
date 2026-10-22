<script lang="ts">
  interface Piece { kind: 'text' | 'code'; text: string; language: string }

  let { text }: { text: string } = $props();

  // A deliberately small renderer: fenced code is displayed in a <pre>; everything else stays
  // plain text so model output can never inject markup into the desktop webview.
  function pieces(value: string): Piece[] {
    const result: Piece[] = [];
    const fence = /```([^\n`]*)\n?([\s\S]*?)(?:```|$)/g;
    let cursor = 0;
    let match: RegExpExecArray | null;
    while ((match = fence.exec(value)) !== null) {
      if (match.index > cursor) {
        result.push({ kind: 'text', text: value.slice(cursor, match.index), language: '' });
      }
      result.push({ kind: 'code', text: match[2].replace(/\n$/, ''), language: match[1].trim() });
      cursor = fence.lastIndex;
    }
    if (cursor < value.length) result.push({ kind: 'text', text: value.slice(cursor), language: '' });
    return result.length ? result : [{ kind: 'text', text: value, language: '' }];
  }

  const blocks = $derived(pieces(text));
</script>

{#each blocks as block, index (index)}
  {#if block.kind === 'code'}
    <pre class="code"><code>{block.text}</code></pre>
  {:else if block.text}
    <div class="text">{block.text}</div>
  {/if}
{/each}

<style>
  .text{white-space:pre-wrap;overflow-wrap:anywhere;font-size:14px;line-height:1.75}
  .text:empty{display:none}
  .code{margin:12px 0;padding:14px 16px;background:var(--sidebar);border:1px solid var(--line);border-radius:10px;overflow-x:auto;font-family:ui-monospace,'Cascadia Code',Consolas,monospace;font-size:12.5px;line-height:1.65;color:#e6e4dc;white-space:pre}
  .code code{font:inherit}
</style>
