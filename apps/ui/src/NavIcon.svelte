<script lang="ts">
  export type IconName = 'Chat' | 'Workspaces' | 'Tasks' | 'Browser' | 'Memory' | 'Settings';
  let { name }: { name: IconName } = $props();
</script>

<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">
  {#if name === 'Chat'}
    <path d="M4.8 4h14.4A1.8 1.8 0 0 1 21 5.8v8.4a1.8 1.8 0 0 1-1.8 1.8H10l-4.6 3.6v-3.6H4.8A1.8 1.8 0 0 1 3 14.2V5.8A1.8 1.8 0 0 1 4.8 4Z" />
  {:else if name === 'Workspaces'}
    <path d="M3.5 7.6a2 2 0 0 1 2-2h3.2c.6 0 1.2.3 1.6.8l1 1.4h7.2a2 2 0 0 1 2 2v6.6a2 2 0 0 1-2 2H5.5a2 2 0 0 1-2-2V7.6Z" />
  {:else if name === 'Tasks'}
    <rect x="3" y="3" width="18" height="18" rx="5" />
    <path d="m8.4 12.2 2.5 2.5 4.8-5.2" />
  {:else if name === 'Browser'}
    <rect x="3" y="4" width="18" height="16" rx="4" />
    <path d="M3 9.5h18" />
    <circle cx="6.7" cy="6.8" r=".9" fill="currentColor" stroke="none" />
    <circle cx="9.4" cy="6.8" r=".9" fill="currentColor" stroke="none" />
  {:else if name === 'Memory'}
    <path d="M6.5 3.5h11a1.5 1.5 0 0 1 1.5 1.5v15.2L12 16.6l-7 3.6V5a1.5 1.5 0 0 1 1.5-1.5Z" />
  {:else if name === 'Settings'}
    <path d="M4 7.5h9M17 7.5h3M4 16.5h2.5M10.5 16.5h9.5" />
    <circle cx="15" cy="7.5" r="2.2" />
    <circle cx="8.5" cy="16.5" r="2.2" />
  {/if}
</svg>

<style>
  svg{width:var(--icon-size,19px);height:var(--icon-size,19px);display:block;opacity:var(--icon-opacity,.6);transition:opacity .15s ease}
</style>
