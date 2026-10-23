<script lang="ts">
  import type { DelegationTask } from '../../../../packages/contracts/src/chat';
  import { agentIdentity } from '../lib/agents';

  let { tasks, siblings = [], onOpen }: {
    tasks: DelegationTask[];
    siblings?: DelegationTask[];
    onOpen: (task: DelegationTask) => void;
  } = $props();

  function state(task: DelegationTask): string {
    return task.state === 'waiting' ? 'Needs your input'
      : task.state === 'running' ? 'Working'
        : task.state === 'queued' ? 'Starting'
          : task.state === 'completed' ? 'Done'
            : task.state === 'cancelled' ? 'Stopped'
              : task.state === 'interrupted' ? 'Interrupted'
                : 'Failed';
  }

  function detail(task: DelegationTask): string {
    if (task.state === 'waiting') return 'Waiting for your answer';
    if (task.state === 'queued') return 'Waiting to start';
    if (task.state === 'running') return task.currentActivity || 'Working on it';
    if (task.state === 'completed') return (task.result ?? '').replace(/\s+/gu, ' ').trim().slice(0, 180);
    if (task.state === 'failed' || task.state === 'interrupted') return task.error ?? 'This task did not finish';
    return '';
  }

  function runningBorder(color: string): string {
    return `color-mix(in srgb, ${color} 45%, var(--line))`;
  }
</script>

<div class="calls" role="group" aria-label="Tasks Talo is working on">
  {#each tasks as task (task.id)}
    {@const agent = agentIdentity(task, siblings.length ? siblings : tasks)}
    <button
      type="button"
      class="call"
      class:running={task.state === 'running' || task.state === 'queued'}
      class:waiting={task.state === 'waiting'}
      class:done={task.state === 'completed'}
      class:failed={task.state === 'failed' || task.state === 'interrupted'}
      style:border-color={task.state === 'running' || task.state === 'queued' ? runningBorder(agent.color) : undefined}
      title={`Open ${task.title}`}
      onclick={() => onOpen(task)}
    >
      <svg class="glyph" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false" style:color={agent.color}>
        <circle cx="6" cy="5.5" r="2.2" />
        <circle cx="18" cy="18.5" r="2.2" />
        <path d="M6 7.7v3.8a5 5 0 0 0 5 5h4.8" />
      </svg>
      <span class="body">
        <span class="line">
          <span class="agent" style:color={agent.color}>{agent.name}</span>
          <span class="title">{task.title}</span>
          <span class="state">{state(task)}</span>
        </span>
        {#if detail(task)}<span class="detail">{detail(task)}</span>{/if}
      </span>
    </button>
  {/each}
</div>

<style>
  .calls{display:grid;gap:4px;margin:2px 0 12px}
  .call{display:flex;align-items:flex-start;gap:9px;width:100%;min-width:0;text-align:left;font:inherit;background:var(--soft);border:1px solid var(--line);border-radius:8px;padding:8px 10px;color:var(--muted);transition:border-color .15s ease}
  .call:hover{border-color:var(--line-strong)}
  .call.waiting{border-color:var(--yellow-deep)}
  .call.failed{border-color:var(--error-line)}
  .glyph{width:13px;height:13px;flex-shrink:0;margin-top:2px;color:var(--faint)}
  .call.waiting .glyph{color:var(--yellow-deep)}
  .call.failed .glyph{color:var(--error-text)}
  .body{display:grid;gap:3px;min-width:0;flex:1}
  .line{display:flex;align-items:baseline;gap:7px;min-width:0}
  .agent{flex-shrink:0;font-size:10px;font-weight:620;letter-spacing:.02em;white-space:nowrap}
  .title{flex:1;min-width:0;font-size:11.5px;color:var(--text);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
  .state{flex-shrink:0;font-size:10px;color:var(--faint);white-space:nowrap}
  .call.running .state{color:var(--yellow-deep)}
  .call.waiting .state{color:var(--yellow)}
  .call.done .state{color:#9fb398}
  .call.failed .state{color:var(--error-text)}
  .detail{font-size:10px;line-height:1.45;color:var(--faint);overflow:hidden;display:-webkit-box;line-clamp:2;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow-wrap:anywhere}
  .call.failed .detail{color:var(--error-text)}
</style>
