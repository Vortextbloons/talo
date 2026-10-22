<script lang="ts">
  import type { DelegationTask } from '../../../../packages/contracts/src/chat';

  let { tasks, onOpen }: {
    tasks: DelegationTask[];
    onOpen: (task: DelegationTask) => void;
  } = $props();

  function state(task: DelegationTask): string {
    return task.state === 'waiting' ? 'Needs you'
      : task.state === 'running' ? 'Running'
        : task.state === 'queued' ? 'Starting'
          : task.state === 'completed' ? 'Done'
            : task.state === 'cancelled' ? 'Stopped'
              : task.state === 'interrupted' ? 'Interrupted'
                : 'Failed';
  }
</script>

<div class="calls" role="group" aria-label="Subagent calls">
  {#each tasks as task (task.id)}
    <button
      type="button"
      class="call"
      class:running={task.state === 'running' || task.state === 'queued'}
      class:failed={task.state === 'failed' || task.state === 'interrupted'}
      title={`Open ${task.title}`}
      onclick={() => onOpen(task)}
    >
      <svg class="glyph" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">
        <circle cx="6" cy="5.5" r="2.2" />
        <circle cx="18" cy="18.5" r="2.2" />
        <path d="M6 7.7v3.8a5 5 0 0 0 5 5h4.8" />
      </svg>
      <span class="kind">Subagent</span>
      <span class="title">{task.title}</span>
      <span class="state">{state(task)}</span>
    </button>
  {/each}
</div>

<style>
  .calls{display:grid;gap:4px;margin:2px 0 12px}
  .call{display:flex;align-items:center;gap:9px;width:100%;min-width:0;text-align:left;font:inherit;background:var(--soft);border:1px solid var(--line);border-radius:8px;padding:7px 10px;color:var(--muted);transition:border-color .15s ease}
  .call:hover{border-color:var(--line-strong)}
  .call.running{border-color:#4e4324}
  .call.failed{border-color:var(--error-line)}
  .glyph{width:13px;height:13px;flex-shrink:0;color:var(--yellow-deep)}
  .call.failed .glyph{color:var(--error-text)}
  .kind{font-size:9px;letter-spacing:.1em;text-transform:uppercase;color:var(--faint);flex-shrink:0}
  .title{flex:1;min-width:0;font-size:11.5px;color:var(--text);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
  .state{flex-shrink:0;font-size:10px;color:var(--faint)}
  .call.running .state{color:var(--yellow-deep)}
  .call.failed .state{color:var(--error-text)}
</style>
