<script lang="ts">
  import { onMount } from 'svelte';
  import AskCard from './AskCard.svelte';
  import { agent, agentAction, refreshAgent } from '../lib/agent.svelte';
  import { library, refreshLibrary } from '../lib/library.svelte';
  import { openConversation } from '../lib/chat.svelte';
  import type { Assignment, Skill, WorkspaceBundle } from '../../../../packages/contracts/src/agent';

  let { onOpenChat }: { onOpenChat: () => void } = $props();

  const open = $derived(agent.assignments.filter(item => item.status === 'open'));
  const settled = $derived(agent.assignments.filter(item => item.status !== 'open').slice(0, 8));

  type SkillDraft = { title: string; description: string; instructions: string; workspaceId: string };
  let edits = $state<Record<string, SkillDraft>>({});

  onMount(() => { void refreshAgent(); void refreshLibrary(); });

  function draftOf(proposal: Skill): SkillDraft {
    return edits[proposal.id] ?? {
      title: proposal.title,
      description: proposal.description,
      instructions: proposal.instructions,
      workspaceId: proposal.workspaceId ?? '',
    };
  }
  function change(proposal: Skill, patch: Partial<SkillDraft>): void {
    edits[proposal.id] = { ...draftOf(proposal), ...patch };
  }
  async function approve(proposal: Skill): Promise<void> {
    const draft = draftOf(proposal);
    if (await agentAction('skill_save', { id: proposal.id, ...draft, workspaceId: draft.workspaceId || 'global' })) delete edits[proposal.id];
  }
  function skillScope(proposal: Skill): string {
    if (!proposal.workspaceId) return 'every project';
    return library.workspaces.find(space => space.id === proposal.workspaceId)?.name ?? 'a project';
  }

  async function openChat(conversationId: string | null): Promise<void> {
    if (conversationId && await openConversation(conversationId)) onOpenChat();
  }
  function due(item: Assignment): string | null {
    if (!item.due) return null;
    const date = new Date(item.due.length === 10 ? `${item.due}T12:00:00` : item.due);
    return Number.isNaN(date.getTime()) ? item.due : date.toLocaleDateString([], { month: 'short', day: 'numeric' });
  }
  function bundleSummary(space: WorkspaceBundle): string {
    const parts: string[] = [];
    if (space.instructions) parts.push('own brief');
    if (space.pinned.length) parts.push(`${space.pinned.length} pinned note${space.pinned.length === 1 ? '' : 's'}`);
    if (space.capabilities.length) parts.push(space.capabilities.join(', '));
    if (space.memoryScope !== 'inherit') parts.push(`memory ${space.memoryScope}`);
    return parts.join(' · ') || 'an empty project';
  }
</script>

<div class="inbox">
  {#if agent.error}<p class="error" role="alert">{agent.error}</p>{/if}
  <p class="budget">Talo can create <strong>{agent.budget.limit}</strong> things a day and has made <strong>{agent.budget.used}</strong> today. It only acts inside a conversation, never on a timer.</p>

  <section>
    <h2>Waiting on you <span>{agent.asks.length}</span></h2>
    {#each agent.asks as ask (ask.id)}
      <AskCard {ask} onOpenChat={() => void openChat(ask.parentConversationId ?? ask.conversationId)} />
    {:else}
      <p class="hint">No open questions. When Talo needs a decision it cannot infer, the card appears here and in the chat.</p>
    {/each}
  </section>

  <section>
    <h2>Tasks Talo left you <span>{open.length}</span></h2>
    {#each open as item (item.id)}
      <article>
        <div class="body">
          <strong>{item.title}</strong>
          {#if item.why}<p class="why">{item.why}</p>{/if}
          {#if item.detail}<p class="detail">{item.detail}</p>{/if}
          <small>
            {due(item) ? `Due ${due(item)} · ` : ''}Added {new Date(item.createdAt).toLocaleDateString()}
            {#if item.conversationId}<button class="link" type="button" onclick={() => void openChat(item.conversationId)}>where it came from</button>{/if}
          </small>
        </div>
        <div class="actions">
          <button disabled={agent.busy} onclick={() => void agentAction('assignment', { id: item.id, status: 'done' })}>Done</button>
          <button disabled={agent.busy} onclick={() => void agentAction('assignment', { id: item.id, status: 'dismissed' })}>Dismiss</button>
        </div>
      </article>
    {:else}
      <p class="hint">Nothing assigned. Talo only hands you work it cannot do itself.</p>
    {/each}
  </section>

  <section>
    <h2>Proposed projects <span>{agent.drafts.length}</span></h2>
    {#each agent.drafts as space (space.id)}
      <article>
        <div class="body">
          <strong>{space.name}</strong>
          <p class="why">{bundleSummary(space)}</p>
          {#if space.instructions}<p class="detail">{space.instructions}</p>{/if}
          {#if space.pinned.length}
            <ul class="pinned">{#each space.pinned as note (note)}<li>{note}</li>{/each}</ul>
          {/if}
        </div>
        <div class="actions">
          <button class="primary" disabled={agent.busy} onclick={() => void agentAction('space_accept', { id: space.id }).then(refreshLibrary)}>Add project</button>
          <button disabled={agent.busy} onclick={() => void agentAction('space_dismiss', { id: space.id })}>Dismiss</button>
        </div>
      </article>
    {:else}
      <p class="hint">No drafts. Talo proposes a project when ongoing work needs its own chats, instructions, and memories.</p>
    {/each}
  </section>

  <section>
    <h2>Proposed skills <span>{agent.skillProposals.length}</span></h2>
    {#each agent.skillProposals as proposal (proposal.id)}
      {@const draft = draftOf(proposal)}
      <article>
        <div class="body">
          <strong>{draft.title}</strong>
          <p class="why">{proposal.replaces ? 'An update to a skill you already approved' : 'A new skill'} · available in {skillScope(proposal)}</p>
          <label class="field">When to use it
            <input maxlength="1024" value={draft.description} oninput={(event) => change(proposal, { description: event.currentTarget.value })} />
          </label>
          <label class="field">Where it applies
            <select value={draft.workspaceId} onchange={(event) => change(proposal, { workspaceId: event.currentTarget.value })}>
              <option value="">Every project</option>
              {#each library.workspaces.filter(space => !space.archived) as space (space.id)}
                <option value={space.id}>{space.name}</option>
              {/each}
            </select>
          </label>
          <details>
            <summary>Instructions</summary>
            <textarea rows="12" maxlength="12000" aria-label="Skill instructions" value={draft.instructions} oninput={(event) => change(proposal, { instructions: event.currentTarget.value })}></textarea>
          </details>
          <small>
            Nothing changes in your chats until you approve it.{proposal.replaces ? ' The approved version stays active until then.' : ''}
            {#if proposal.sourceConversationId}<button class="link" type="button" onclick={() => void openChat(proposal.sourceConversationId)}>where it came from</button>{/if}
          </small>
        </div>
        <div class="actions">
          <button class="primary" disabled={agent.busy || !draft.title.trim() || !draft.description.trim() || !draft.instructions.trim()} onclick={() => void approve(proposal)}>Approve</button>
          <button disabled={agent.busy} onclick={() => void agentAction('skill_dismiss', { id: proposal.id })}>Dismiss</button>
        </div>
      </article>
    {:else}
      <p class="hint">No proposed skills. After a workflow works well, Talo can offer to save the steps for next time.</p>
    {/each}
  </section>

  {#if settled.length}
    <section>
      <h2>Recently settled</h2>
      {#each settled as item (item.id)}
        <article class="settled">
          <div class="body">
            <strong>{item.title}</strong>
            <small>{item.status === 'done' ? 'Done' : 'Dismissed'} · {new Date(item.updatedAt).toLocaleDateString()}</small>
          </div>
          <div class="actions">
            <button disabled={agent.busy} onclick={() => void agentAction('assignment', { id: item.id, status: 'open' })}>Reopen</button>
          </div>
        </article>
      {/each}
    </section>
  {/if}
</div>

<style>
  .inbox{padding-bottom:30px;display:grid;gap:18px}
  .error{padding:12px 14px;background:var(--error-bg);border:1px solid var(--error-line);color:var(--error-text);border-radius:10px;font-size:12px;margin:0}
  .budget{font-size:12px;color:var(--muted);margin:0;line-height:1.6}
  .budget strong{color:var(--text);font-weight:600}
  section{background:var(--paper);border:1px solid var(--line);border-radius:16px;padding:20px 22px}
  h2{font-size:16px;margin:0 0 12px;letter-spacing:-.35px;font-weight:550;color:var(--text);display:flex;align-items:center;gap:8px}
  h2 span{font-size:11px;font-weight:550;color:var(--faint);background:var(--soft);border:1px solid var(--line);border-radius:999px;padding:1px 7px}
  article{background:var(--soft);border:1px solid var(--line);border-radius:12px;padding:13px 15px;margin-top:10px;display:flex;gap:14px;align-items:start;justify-content:space-between;flex-wrap:wrap}
  article .body{min-width:0;flex:1}
  article strong{font-size:13.5px;font-weight:550;color:var(--text)}
  .why{font-size:12px;color:var(--text);margin:4px 0 0;line-height:1.55}
  .detail{font-size:12px;color:var(--muted);margin:5px 0 0;line-height:1.6;white-space:pre-wrap;overflow-wrap:anywhere}
  small{display:block;font-size:11px;color:var(--faint);margin-top:6px}
  .pinned{margin:7px 0 0;padding-left:17px;font-size:11.5px;color:var(--muted);line-height:1.65}
  .actions{display:flex;gap:8px;flex-wrap:wrap}
  button{font:inherit;font-size:12px;border:1px solid var(--line-strong);border-radius:8px;padding:8px 13px;background:var(--paper);color:var(--text);cursor:pointer}
  button:hover:not(:disabled){background:#2c2c27}
  button:disabled{opacity:.45;cursor:not-allowed}
  .primary{background:var(--yellow);border-color:var(--yellow);color:var(--black);font-weight:600}
  .primary:hover:not(:disabled){background:#ffdd7f;border-color:#ffdd7f}
  .link{background:none;border:0;padding:0;margin-left:5px;color:var(--yellow);font:inherit;font-size:11px;text-decoration:underline;cursor:pointer}
  .hint{font-size:12px;color:var(--muted);line-height:1.65;margin:0}
  .field{display:grid;gap:6px;font-size:11px;font-weight:550;color:var(--muted);margin-top:9px}
  .field input,.field select,textarea{width:100%;box-sizing:border-box;border:1px solid var(--line-strong);background:var(--paper);border-radius:8px;padding:8px 10px;color:var(--text);font:inherit;font-size:12px;line-height:1.6;min-width:0}
  textarea{resize:vertical;margin-top:8px;font-family:var(--font-mono,inherit)}
  details summary{cursor:pointer;font-size:11px;font-weight:550;color:var(--muted);margin-top:9px}
  article .body{max-width:100%}
  .settled{opacity:.72}
  .settled strong{font-weight:500}
</style>
