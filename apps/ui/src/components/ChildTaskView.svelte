<script lang="ts">
  import type { ConversationInfo, ConversationSnapshot, DelegationActivity, DelegationTask } from '../../../../packages/contracts/src/chat';
  import MessageBody from './MessageBody.svelte';
  import AskCard from './AskCard.svelte';
  import { asksFor } from '../lib/agent.svelte';
  import { browser, closeBrowser, openBrowser, selectBrowserSession } from '../lib/browser.svelte';
  import mascot from '../../../../packages/design/mascot.svg';

  let { snapshot, parent, task = null, activity = [], onBack, agentName = 'Talo' }: {
    snapshot: ConversationSnapshot;
    parent: ConversationInfo | null;
    task?: DelegationTask | null;
    activity?: DelegationActivity[];
    onBack: () => void;
    agentName?: string;
  } = $props();

  const working = $derived(snapshot.state === 'generating' || snapshot.state === 'cancelling');
  const stateLabel = $derived(
    task?.state === 'waiting' ? 'Needs an answer'
      : task?.state === 'queued' ? 'Queued'
      : task?.state === 'running' ? 'Working'
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
  function latestActivity(items: DelegationActivity[]): string {
    for (let index = items.length - 1; index >= 0; index -= 1) {
      if (items[index].state === 'running') return items[index].label;
    }
    return items.length ? items[items.length - 1].label : '';
  }
  const current = $derived(
    task?.state === 'waiting' ? 'Waiting for your answer'
      : latestActivity(activity) || (task?.state === 'queued' ? 'Waiting to start' : ''),
  );
  const browserConnected = $derived(browser.sessionId === snapshot.conversation.id && browser.status.state === 'connected');

  function watchBrowser(): void {
    const selected = browser.sessionId === snapshot.conversation.id;
    if (selected && browser.view !== 'closed') {
      closeBrowser();
      return;
    }
    selectBrowserSession(snapshot.conversation.id);
    if (browser.view === 'closed') void openBrowser('side');
  }
</script>

<div class="child-top">
  <button type="button" class="back" onclick={onBack} aria-label="Back to parent chat">
    <span aria-hidden="true">←</span> Back to {parent?.title || 'parent chat'}
  </button>
</div>

<div class="conversation-head child-head">
  <div class="agent-identity">
    <img class="agent-avatar" class:busy={working} src={mascot} alt="" aria-hidden="true" />
    <div class="conversation-title">
      <div class="agent-line"><span class="agent-name">{agentName}</span><span class="state">{stateLabel}</span></div>
      <strong>{snapshot.conversation.title || task?.title || 'Delegated task'}</strong>
      <span class="parent-line">{parent ? `Child of ${parent.title}` : 'Delegated task'}</span>
    </div>
  </div>
  <div class="head-actions">
    <button type="button" class="browser-chip" class:browser-active={browserConnected || (browser.sessionId === snapshot.conversation.id && browser.view !== 'closed')} onclick={watchBrowser} title="Watch this task's own browser">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="3"/><path d="M3 9h18M7 6.5h.01M10 6.5h.01"/></svg>
      <span>Browser</span>
    </button>
  </div>
</div>

{#if current && (working || task?.state === 'waiting')}
  <div class="current-line" role="status"><span class="pulse" aria-hidden="true"></span>{current}</div>
{/if}

{#if task?.error}<div class="chat-message error" role="alert">{task.error}</div>{:else if snapshot.error}<div class="chat-message error" role="alert">{snapshot.error}</div>{/if}

{#if activity.length}
  <section class="activity-panel" aria-label="What this agent is doing">
    <details open>
      <summary><span>Actions</span><small>{activity.length} {activity.length === 1 ? 'step' : 'steps'}</small></summary>
      <ol class="activity-list">
        {#each activity as item (item.id)}
          <li class="activity" class:running={item.state === 'running'} class:failed={item.state === 'failed'}>
            <span class="activity-dot" aria-hidden="true"></span>
            <div>
              <p class="activity-label">{item.label}</p>
              {#if item.detail}<p class="activity-detail">{item.detail}</p>{/if}
            </div>
            <span class="activity-state">{item.state === 'running' ? 'Now' : item.state === 'failed' ? 'Failed' : ''}</span>
          </li>
        {/each}
      </ol>
    </details>
  </section>
{/if}

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

<div class="child-foot">
  <span>To change the task, send instructions in the parent chat.</span>
  <button type="button" onclick={onBack}>Back to parent</button>
</div>

<style>
  .child-top{padding:0 0 8px}
  .back{display:inline-flex;align-items:center;gap:7px;background:transparent;border:1px solid var(--line);border-radius:9px;color:var(--text);font-size:11.5px;padding:7px 11px;max-width:100%;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
  .back:hover{border-color:var(--yellow-deep);color:var(--yellow)}
  .child-head{align-items:center;padding-bottom:10px;margin-bottom:10px}
  .agent-identity{display:flex;align-items:flex-start;gap:11px;min-width:0}
  .agent-avatar{width:34px;height:34px;flex-shrink:0;margin-top:1px}
  .agent-avatar.busy{animation:agent-think 1.4s ease-in-out infinite;filter:drop-shadow(0 0 8px rgba(255,212,92,.3))}
  .conversation-title{min-width:0}
  .agent-line{display:flex;align-items:center;gap:7px;margin-bottom:2px}
  .agent-name{font-size:10px;color:var(--yellow-deep);letter-spacing:.03em}
  .state{font-size:9.5px;border:1px solid var(--line-strong);border-radius:20px;padding:1px 6px;color:var(--muted)}
  .conversation-title strong{display:block;font-size:14.5px;font-weight:550;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
  .parent-line{display:block;font-size:10.5px;color:var(--muted);margin-top:3px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
  .head-actions{flex-shrink:0}
  .browser-chip{display:inline-flex;align-items:center;gap:6px;background:transparent;border:1px solid var(--line);border-radius:8px;padding:6px 9px;color:var(--muted);font-size:10px}
  .browser-chip svg{width:14px;height:14px}
  .browser-chip:hover,.browser-chip.browser-active{color:var(--yellow);border-color:var(--line-strong)}
  .current-line{display:flex;align-items:center;gap:7px;font-size:11px;color:var(--yellow-deep);margin:0 2px 10px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
  .pulse{width:5px;height:5px;border-radius:50%;background:var(--yellow);flex-shrink:0;animation:pulse 1.2s ease-in-out infinite}
  .activity-panel{border:1px solid var(--line);border-radius:11px;background:var(--paper);margin-bottom:10px;overflow:hidden;flex-shrink:0}
  details{padding:0}
  summary{display:flex;align-items:center;justify-content:space-between;gap:10px;padding:9px 12px;cursor:pointer;list-style:none;font-size:10px;letter-spacing:.09em;text-transform:uppercase;color:var(--faint)}
  summary::-webkit-details-marker{display:none}
  summary small{font-size:9.5px;letter-spacing:0;text-transform:none;color:var(--faint)}
  .activity-list{list-style:none;margin:0;padding:0 12px 9px;max-height:210px;overflow-y:auto;display:grid;gap:2px}
  .activity{display:grid;grid-template-columns:8px minmax(0,1fr) auto;gap:8px;align-items:start;padding:5px 0;border-top:1px solid var(--line)}
  .activity:first-child{border-top:0}
  .activity-dot{width:6px;height:6px;border-radius:50%;background:var(--line-strong);margin-top:5px}
  .activity.running .activity-dot{background:var(--yellow);animation:pulse 1.2s ease-in-out infinite}
  .activity.failed .activity-dot{background:var(--error-text)}
  .activity-label{margin:0;font-size:11.5px;line-height:1.4;color:var(--text)}
  .activity-detail{margin:1px 0 0;font-size:10.5px;line-height:1.4;color:var(--faint);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
  .activity-state{font-size:9px;color:var(--yellow-deep);white-space:nowrap;padding-top:2px}
  .activity.failed .activity-state{color:var(--error-text)}
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
  .child-foot{display:flex;align-items:center;justify-content:space-between;gap:12px;padding-top:8px;font-size:10.5px;color:var(--faint)}
  .child-foot button{background:transparent;border:1px solid var(--line);border-radius:8px;color:var(--muted);font-size:10.5px;padding:6px 10px;flex-shrink:0}
  .child-foot button:hover{border-color:var(--yellow-deep);color:var(--yellow)}
  @keyframes pulse{0%,100%{opacity:.4}50%{opacity:1}}
  @keyframes agent-think{0%,100%{transform:rotate(-4deg) translateY(.5px)}50%{transform:rotate(4deg) translateY(-1.5px)}}
  @media(max-width:700px){.child-top{padding-bottom:6px}.head-actions{display:none}.child-foot span{display:none}}
  @media(prefers-reduced-motion:reduce){.agent-avatar.busy,.pulse,.activity-dot{animation:none}}
</style>
