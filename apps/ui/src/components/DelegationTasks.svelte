<script lang="ts">
  import type { DelegationTask } from '../../../../packages/contracts/src/chat';

  let { tasks, onOpen, onCancel, onCancelAll }: {
    tasks: DelegationTask[];
    onOpen: (task: DelegationTask) => void;
    onCancel: (task: DelegationTask) => void;
    onCancelAll: () => void;
  } = $props();

  const active = $derived(tasks.filter(task => task.state === 'queued' || task.state === 'running' || task.state === 'waiting'));

  function label(state: DelegationTask['state']): string {
    return state === 'waiting' ? 'Needs an answer'
      : state === 'running' ? 'Working'
        : state === 'queued' ? 'Queued'
          : state === 'completed' ? 'Done'
            : state === 'cancelled' ? 'Cancelled'
              : state === 'interrupted' ? 'Interrupted'
                : 'Failed';
  }
</script>

{#if tasks.length}
  <section class="delegations" aria-label="Delegated tasks">
    <header>
      <span>Delegated work</span>
      {#if active.length > 1}
        <button type="button" class="cancel-all" onclick={onCancelAll}>Cancel all</button>
      {/if}
    </header>
    <div class="task-list">
      {#each tasks as task (task.id)}
        {@const busy = task.state === 'queued' || task.state === 'running' || task.state === 'waiting'}
        <article class="task" class:busy class:failed={task.state === 'failed' || task.state === 'interrupted'}>
          <div class="task-main">
            <div class="task-head">
              <strong>{task.title}</strong>
              <span class="state">{label(task.state)}</span>
            </div>
            {#if task.error}
              <p class="outcome failed-text">{task.error}</p>
            {:else if task.result}
              <p class="outcome">{task.result.length > 220 ? `${task.result.slice(0, 220)}…` : task.result}</p>
            {:else if busy}
              <p class="outcome muted">Running independently. You can keep chatting or switch chats.</p>
            {/if}
          </div>
          <div class="task-actions">
            <button type="button" onclick={() => onOpen(task)}>Open</button>
            {#if busy}<button type="button" class="stop" onclick={() => onCancel(task)}>Cancel</button>{/if}
          </div>
        </article>
      {/each}
    </div>
  </section>
{/if}

<style>
  .delegations{border:1px solid var(--line);border-radius:12px;background:var(--paper);padding:10px 12px;margin:0 0 12px}
  header{display:flex;align-items:center;justify-content:space-between;gap:10px;margin-bottom:8px}
  header>span{font-size:10px;letter-spacing:.09em;text-transform:uppercase;color:var(--faint)}
  .cancel-all{background:transparent;border:1px solid var(--line);border-radius:7px;color:var(--muted);font-size:10px;padding:3px 8px}
  .cancel-all:hover{border-color:var(--error-line);color:var(--error-text)}
  .task-list{display:grid;gap:6px}
  .task{display:flex;align-items:flex-start;justify-content:space-between;gap:12px;padding:8px 9px;border:1px solid var(--line);border-radius:9px;background:var(--soft)}
  .task.busy{border-color:#4e4324}
  .task.failed{border-color:var(--error-line)}
  .task-main{min-width:0}
  .task-head{display:flex;align-items:center;gap:8px;min-width:0}
  .task-head strong{font-size:12px;font-weight:550;color:var(--text);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
  .state{font-size:9.5px;border:1px solid var(--line-strong);border-radius:20px;padding:2px 7px;color:var(--muted);white-space:nowrap}
  .task.busy .state{border-color:#4e4324;color:var(--yellow-deep)}
  .task.failed .state{border-color:var(--error-line);color:var(--error-text)}
  .outcome{margin:5px 0 0;font-size:11px;line-height:1.5;color:var(--muted);display:-webkit-box;line-clamp:3;-webkit-line-clamp:3;-webkit-box-orient:vertical;overflow:hidden}
  .outcome.muted{color:var(--faint)}
  .failed-text{color:var(--error-text)}
  .task-actions{display:flex;gap:6px;flex-shrink:0}
  .task-actions button{background:transparent;border:1px solid var(--line);border-radius:7px;color:var(--muted);font-size:10px;padding:4px 8px}
  .task-actions button:hover{border-color:var(--line-strong);color:var(--text)}
  .task-actions .stop:hover{border-color:var(--error-line);color:var(--error-text)}
</style>
