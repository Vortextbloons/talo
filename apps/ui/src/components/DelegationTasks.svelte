<script lang="ts">
  import type { DelegationTask } from '../../../../packages/contracts/src/chat';
  import mascot from '../../../../packages/design/mascot.svg';

  let { tasks, onOpen, onCancel, onCancelAll }: {
    tasks: DelegationTask[];
    onOpen: (task: DelegationTask) => void;
    onCancel: (task: DelegationTask) => void;
    onCancelAll: () => void;
  } = $props();

  const active = $derived(tasks.filter(task => task.state === 'queued' || task.state === 'running' || task.state === 'waiting'));
  /* Finished work leaves the panel; the chat keeps the record of the call. */
  const shown = $derived(tasks.filter(task => task.state !== 'completed' && task.state !== 'cancelled'));

  function label(state: DelegationTask['state']): string {
    return state === 'waiting' ? 'Needs you'
      : state === 'running' ? 'Working'
        : state === 'queued' ? 'Queued'
          : state === 'interrupted' ? 'Interrupted'
            : 'Failed';
  }

  function current(task: DelegationTask): string {
    if (task.state === 'waiting') return 'Waiting for your answer';
    if (task.state === 'queued') return 'Waiting to start';
    return task.currentActivity || 'Starting work';
  }
</script>

{#if shown.length}
  <section class="delegations" aria-label="Active subagents">
    <header>
      <span>Subagents</span>
      {#if active.length > 1}
        <button type="button" class="cancel-all" onclick={onCancelAll}>Cancel all</button>
      {/if}
    </header>
    {#each shown as task (task.id)}
      {@const busy = task.state === 'queued' || task.state === 'running' || task.state === 'waiting'}
      <article class="task" class:busy class:failed={task.state === 'failed' || task.state === 'interrupted'}>
        <button type="button" class="card-open" aria-label={`Open ${task.title}`} onclick={() => onOpen(task)}></button>
        <img class="agent-avatar" class:busy src={mascot} alt="" aria-hidden="true" />
        <div class="task-main">
          <div class="line">
            <strong class="title">{task.title}</strong>
            <span class="state">{label(task.state)}</span>
          </div>
          {#if task.error}
            <p class="detail failed-text">{task.error}</p>
          {:else if busy}
            <p class="detail"><span class="pulse" aria-hidden="true"></span>{current(task)}</p>
          {/if}
        </div>
        {#if busy}
          <button
            type="button"
            class="stop"
            aria-label={`Cancel ${task.title}`}
            title="Cancel"
            onclick={(event) => { event.stopPropagation(); onCancel(task); }}
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true" focusable="false"><path d="M7 7l10 10M17 7 7 17" /></svg>
          </button>
        {/if}
      </article>
    {/each}
  </section>
{/if}

<style>
  .delegations{display:grid;gap:4px}
  header{display:flex;align-items:center;justify-content:space-between;gap:8px;margin-bottom:4px}
  header>span{font-size:9px;letter-spacing:.1em;text-transform:uppercase;color:var(--faint)}
  .cancel-all{background:transparent;border:0;padding:0;color:var(--faint);font-size:9.5px;text-decoration:underline}
  .cancel-all:hover{color:var(--error-text)}
  .task{position:relative;display:flex;align-items:center;gap:7px;padding:5px 6px;border:1px solid var(--line);border-radius:8px;background:var(--soft);transition:border-color .15s ease}
  .task:hover,.task:has(.card-open:focus-visible){border-color:var(--line-strong)}
  .task.busy{border-color:#4e4324}
  .task.failed{border-color:var(--error-line)}
  .card-open{position:absolute;inset:0;padding:0;border:0;border-radius:8px;background:transparent;cursor:pointer;z-index:0}
  .card-open:focus-visible{outline:2px solid var(--yellow);outline-offset:2px}
  .agent-avatar,.task-main,.stop{position:relative;z-index:1}
  .agent-avatar{width:18px;height:18px;flex-shrink:0;pointer-events:none}
  .agent-avatar.busy{animation:agent-think 1.4s ease-in-out infinite}
  .task-main{min-width:0;flex:1;pointer-events:none}
  .line{display:flex;align-items:center;gap:6px;min-width:0}
  .title{flex:1;min-width:0;font-size:11px;font-weight:550;color:var(--text);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
  .state{flex-shrink:0;font-size:9px;color:var(--faint);white-space:nowrap}
  .task.busy .state{color:var(--yellow-deep)}
  .task.failed .state{color:var(--error-text)}
  .detail{display:flex;align-items:center;gap:5px;margin:1px 0 0;font-size:9.5px;line-height:1.4;color:var(--faint);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
  .failed-text{color:var(--error-text);white-space:normal}
  .pulse{width:4px;height:4px;border-radius:50%;background:var(--yellow);flex-shrink:0;animation:pulse 1.2s ease-in-out infinite}
  .stop{display:grid;place-items:center;width:16px;height:16px;flex-shrink:0;padding:0;border:0;border-radius:5px;background:transparent;color:var(--faint)}
  .stop svg{width:10px;height:10px}
  .stop:hover{color:var(--error-text);background:var(--error-bg)}
  @keyframes pulse{0%,100%{opacity:.35}50%{opacity:1}}
  @keyframes agent-think{0%,100%{transform:rotate(-4deg) translateY(.5px)}50%{transform:rotate(4deg) translateY(-1.5px)}}
  @media(prefers-reduced-motion:reduce){.agent-avatar.busy,.pulse{animation:none}}
</style>
