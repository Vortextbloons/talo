<script lang="ts">
  let { text, thinking, hasReply }: { text: string; thinking: boolean; hasReply: boolean } = $props();

  // Thinking streams open; the reply pulls focus back unless the reader pinned the panel.
  // The initial props are deliberately captured once: this component mounts per message.
  let touched = $state(false);
  // svelte-ignore state_referenced_locally
  let open = $state(thinking && !hasReply);

  $effect(() => {
    if (touched) return;
    // Follow the turn: open while thinking streams, close once the reply takes over.
    if (thinking && !hasReply) open = true;
    else if (hasReply) open = false;
  });

  function toggle(): void {
    touched = true;
    open = !open;
  }
</script>

<div class="reasoning" class:thinking>
  <button type="button" class="reasoning-toggle" aria-expanded={open} onclick={toggle}>
    <svg class="reasoning-chevron" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><path d="m6 9.5 6 6 6-6" /></svg>
    Reasoning
    {#if thinking}<span class="reasoning-state">thinking…</span>{/if}
  </button>
  {#if open}
    <div class="reasoning-body">{text}</div>
  {/if}
</div>

<style>
  .reasoning{margin:2px 0 12px}
  .reasoning-toggle{display:inline-flex;align-items:center;gap:6px;background:transparent;border:0;padding:2px 0;color:var(--faint);font-size:10.5px;letter-spacing:.08em;text-transform:uppercase}
  .reasoning-toggle:hover{color:var(--yellow-deep)}
  .reasoning-chevron{width:11px;height:11px;transition:transform .15s ease}
  .reasoning-toggle[aria-expanded="true"] .reasoning-chevron{transform:rotate(180deg)}
  .reasoning-state{color:var(--yellow-deep);text-transform:none;letter-spacing:.02em}
  .reasoning-body{margin-top:8px;padding:2px 0 2px 12px;border-left:2px solid var(--line);color:var(--muted);font-size:12px;line-height:1.7;white-space:pre-wrap;overflow-wrap:anywhere;max-height:260px;overflow-y:auto;scrollbar-gutter:stable}
  .thinking .reasoning-body{border-left-color:#4e4324}
  @media(prefers-reduced-motion:reduce){.reasoning-chevron{transition:none}}
</style>