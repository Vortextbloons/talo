<script lang="ts">
  let { color = 'var(--companion)', size = 27, title = '' }: { color?: string; size?: number; title?: string } = $props();
  const height = $derived(Math.round(size * 80 / 96));
</script>

<svg viewBox="0 0 96 80" style:width="{size}px" style:height="{height}px" style:color={color} role={title ? 'img' : undefined} aria-label={title || undefined} aria-hidden={title ? undefined : 'true'}>
  <rect x="2" y="2" width="92" height="76" rx="32" fill="currentColor" />
  <rect x="28" y="25" width="10" height="28" rx="5" fill="#151515" />
  <rect x="58" y="25" width="10" height="28" rx="5" fill="#151515" />
</svg>

<style>
  svg{display:block;flex-shrink:0}
</style>
