<script lang="ts">
  import type { ConversationInfo, ConversationSnapshot, DelegationTask } from '../../../../packages/contracts/src/chat';
  import MessageBody from './MessageBody.svelte';
  import AskCard from './AskCard.svelte';
  import { asksFor } from '../lib/agent.svelte';

  let { snapshot, parent, task = null, onBack, agentName = 'Talo' }: {
    snapshot: ConversationSnapshot;
    parent: ConversationInfo | null;
    task?: DelegationTask | null;
    onBack: () => void;
    agentName?: string;
  } = $props();

  const working = $derived(snapshot.state === 'generating' || snapshot.state === 'cancelling');
  const stateLabel = $derived(
    task?.state === 'waiting' ? 'Needs an answer'
      : task?.state === 'queued' ? 'Queued'
      : task?.state === 'completed' ? 'Done'
      : task?.state === 'cancelled' ? 'Cancelled'
      : task?.state === 'failed' ? 'Failed'
      : task?.state === 'interrupted' ? 'Interrupted'
      : snapshot.state === 'generating' ? 'Working'
      : snapshot.state === 'cancelling' ? 'Stopping'
        : snapshot.state === 'completed' ? 'Done'
          : snapshot.state === 'cancelled' ? 'Cancelled'
            : snapshot.state === 'failed' ? 'Failed'
              : 'Waiting',
  );
</script>

<div class="conversation-head child-head">
  <div class="conversation-lead">
    <button type="button" class="back" onclick={onBack} aria-label="Back to parent chat">← {parent?.title ?? 'Parent chat'}</button>
    <div class="conversation-title">
      <strong>{snapshot.conversation.title || 'Delegated task'}</strong>
      <span>{stateLabel}{parent ? ` · child of ${parent.title}` : ' · delegated task'}</span>
    </div>
  </div>
</div>

{#if task?.error}<div class="chat-message error" role="alert">{task.error}</div>{:else if snapshot.error}<div class="chat-message error" role="alert">{snapshot.error}</div>{/if}

<div class="log child-log" aria-live="polite" aria-busy={working} role="log">
  {#each snapshot.messages as message (message.id)}
    <article class="message {message.role}" class:streaming={message.status === 'generating'}>
      <div class="message-body">
        <div class="message-role">
          <span>{message.role === 'user' ? 'Task' : agentName}</span>
          {#if message.status === 'cancelled'}<span class="status-tag">Stopped</span>{/if}
          {#if message.status === 'failed'}<span class="status-tag failed">Failed</span>{/if}
        </div>
        {#if message.text}<MessageBody text={message.text} />{:else if message.status === 'generating'}<span class="typing" aria-label="Working"><i></i><i></i><i></i></span>{/if}
        {#if message.error}<p class="message-error">{message.error}</p>{/if}
      </div>
    </article>
  {/each}
</div>

{#if parent}
  {#each asksFor(parent.id) as ask (ask.id)}
    <AskCard {ask} />
  {/each}
{/if}

<p class="child-note">This is an inspection view. To change the task, send instructions in the parent chat.</p>

<style>
  .child-head{align-items:center}
  .back{background:transparent;border:1px solid var(--line);border-radius:8px;color:var(--muted);font-size:11px;padding:5px 9px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:240px}
  .back:hover{border-color:var(--line-strong);color:var(--text)}
  .child-log{flex:1;min-height:0;overflow-y:auto;padding:8px 10px 20px 2px}
  .message{display:flex;gap:12px;padding:14px 0 20px;border-bottom:1px solid var(--line)}
  .message:last-child{border-bottom:0}
  .message.user{justify-content:flex-end}
  .message-body{min-width:0;max-width:92%}
  .message.user .message-body{max-width:82%;background:#23231e;border:1px solid #33332a;border-radius:13px;padding:10px 14px}
  .message-role{display:flex;align-items:center;gap:8px;font-size:11px;color:var(--muted);margin-bottom:6px}
  .message.user .message-role{justify-content:flex-end;margin-bottom:4px}
  .status-tag{font-size:9.5px;border:1px solid var(--line);border-radius:20px;padding:2px 7px;color:var(--muted)}
  .status-tag.failed{border-color:var(--error-line);color:var(--error-text)}
  .typing{display:inline-flex;gap:5px;padding:7px 0}
  .typing i{width:5px;height:5px;border-radius:50%;background:var(--yellow-deep);animation:pulse 1.1s ease-in-out infinite}
  .message-error{margin-top:9px;font-size:11.5px;color:var(--error-text);line-height:1.6}
  .child-note{font-size:10.5px;line-height:1.5;color:var(--faint);margin:8px 2px 0}
  @keyframes pulse{0%,100%{opacity:.4}50%{opacity:1}}
</style>
