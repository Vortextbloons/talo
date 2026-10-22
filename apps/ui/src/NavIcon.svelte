<script lang="ts">
  export type IconName = 'Chat' | 'Inbox' | 'Workspaces' | 'Tasks' | 'Settings';
  let { name, working = false }: { name: IconName; working?: boolean } = $props();
  const chatting = $derived(name === 'Chat' && working === true);
</script>

<svg class:working={chatting} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">
  {#if name === 'Chat'}
    <path d="M4.8 4h14.4A1.8 1.8 0 0 1 21 5.8v8.4a1.8 1.8 0 0 1-1.8 1.8H10l-4.6 3.6v-3.6H4.8A1.8 1.8 0 0 1 3 14.2V5.8A1.8 1.8 0 0 1 4.8 4Z" />
    {#if chatting}
      <g fill="currentColor" stroke="none"><circle class="d" cx="8.4" cy="10.6" r="1.15" /><circle class="d" cx="12" cy="10.6" r="1.15" /><circle class="d" cx="15.6" cy="10.6" r="1.15" /></g>
    {/if}
  {:else if name === 'Inbox'}
    <path d="M4 4.8h16v9.4H13l-4.2 4.3v-4.3H4V4.8Z" />
    <path d="M7.4 8.6h9.2M7.4 11.4h5.6" />
  {:else if name === 'Workspaces'}
    <path d="M3.5 7.6a2 2 0 0 1 2-2h3.2c.6 0 1.2.3 1.6.8l1 1.4h7.2a2 2 0 0 1 2 2v6.6a2 2 0 0 1-2 2H5.5a2 2 0 0 1-2-2V7.6Z" />
  {:else if name === 'Tasks'}
    <rect x="3" y="3" width="18" height="18" rx="5" />
    <path d="m8.4 12.2 2.5 2.5 4.8-5.2" />
  {:else if name === 'Settings'}
    <path d="M4 7.5h9M17 7.5h3M4 16.5h2.5M10.5 16.5h9.5" />
    <circle cx="15" cy="7.5" r="2.2" />
    <circle cx="8.5" cy="16.5" r="2.2" />
  {/if}
</svg>

<style>
  svg{width:var(--icon-size,19px);height:var(--icon-size,19px);display:block;opacity:var(--icon-opacity,.6);transition:opacity .15s ease}
  svg.working{animation:icon-bob 1.15s ease-in-out infinite}
  .d{opacity:.25;animation:dot-type 1.15s ease-in-out infinite}
  .d:nth-of-type(2){animation-delay:.15s}
  .d:nth-of-type(3){animation-delay:.3s}
  @keyframes icon-bob{0%,100%{transform:translateY(0) rotate(0)}30%{transform:translateY(-1.3px) rotate(-2.5deg)}70%{transform:translateY(.4px) rotate(2deg)}}
  @keyframes dot-type{0%,100%{opacity:.25;transform:translateY(.5px)}45%{opacity:1;transform:translateY(-.6px)}}
  @media(prefers-reduced-motion:reduce){svg.working,.d{animation:none}.d{opacity:.75}}
</style>