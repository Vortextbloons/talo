<script lang="ts">
  import { onMount } from 'svelte';
  import { agent, agentAction, refreshAgent } from '../lib/agent.svelte';
  import { library, refreshLibrary } from '../lib/library.svelte';
  import type { Skill } from '../../../../packages/contracts/src/agent';

  let editing = $state<string | null>(null);
  let title = $state('');
  let description = $state('');
  let instructions = $state('');
  let workspaceId = $state('');
  let removing = $state<string | null>(null);

  const skills = $derived([...agent.skills].sort((a, b) =>
    a.status === b.status ? a.title.localeCompare(b.title) : a.status === 'approved' ? -1 : 1));

  onMount(() => { void refreshAgent(); void refreshLibrary(); });

  function scopeLabel(skill: Skill): string {
    if (!skill.workspaceId) return 'Every project';
    return library.workspaces.find(space => space.id === skill.workspaceId)?.name ?? 'A deleted project';
  }
  function edit(skill: Skill): void {
    editing = skill.id;
    title = skill.title;
    description = skill.description;
    instructions = skill.instructions;
    workspaceId = skill.workspaceId ?? '';
    removing = null;
  }
  function reset(): void {
    editing = null;
    title = '';
    description = '';
    instructions = '';
    workspaceId = '';
  }
  async function save(): Promise<void> {
    if (!editing) return;
    const saved = await agentAction('skill_save', {
      id: editing,
      title,
      description,
      instructions,
      workspaceId: workspaceId || 'global',
    });
    if (saved) reset();
  }
  async function toggle(skill: Skill): Promise<void> {
    await agentAction('skill_enable', { id: skill.id, enabled: skill.status !== 'approved' });
  }
  async function remove(skill: Skill): Promise<void> {
    if (await agentAction('skill_delete', { id: skill.id })) removing = null;
  }
</script>

<div class="skills-page">
  {#if agent.error}<p class="error" role="alert">{agent.error}</p>{/if}
  <section>
    <h2>Saved skills</h2>
    <p>Steps Talo saved after they worked, ready to reuse when a similar request comes up. They only describe how to do something: they never add tools or access.</p>

    {#each skills as skill (skill.id)}
      <article class:disabled={skill.status === 'disabled'}>
        {#if editing === skill.id}
          <form onsubmit={(event) => { event.preventDefault(); void save(); }}>
            <label>Name<input required maxlength="60" bind:value={title} /></label>
            <label>When to use it<input required maxlength="1024" bind:value={description} placeholder="What it does, and the request this skill fits." /></label>
            <label>Where it applies
              <select bind:value={workspaceId}>
                <option value="">Every project</option>
                {#each library.workspaces.filter(space => !space.archived) as space (space.id)}
                  <option value={space.id}>{space.name}</option>
                {/each}
              </select>
            </label>
            <label>Instructions<textarea required maxlength="12000" rows="14" bind:value={instructions}></textarea></label>
            <div class="actions">
              <button class="primary" disabled={agent.busy || !title.trim() || !description.trim() || !instructions.trim()}>Save skill</button>
              <button type="button" onclick={reset}>Cancel</button>
            </div>
            <p class="hint">Saving replaces the active version. Chats pick up the new instructions on their next turn.</p>
          </form>
        {:else}
          <div class="head">
            <div>
              <h3>{skill.title}</h3>
              <small>{scopeLabel(skill)}{skill.status === 'disabled' ? ' · Turned off' : ''} · Version {skill.revision}</small>
            </div>
            <div class="actions">
              <button onclick={() => edit(skill)}>Edit</button>
              <button disabled={agent.busy} onclick={() => void toggle(skill)}>{skill.status === 'disabled' ? 'Turn on' : 'Turn off'}</button>
              {#if removing === skill.id}
                <button class="danger" disabled={agent.busy} onclick={() => void remove(skill)}>Confirm remove</button>
                <button onclick={() => removing = null}>Keep</button>
              {:else}
                <button onclick={() => removing = skill.id}>Remove</button>
              {/if}
            </div>
          </div>
          <p class="description">{skill.description}</p>
          <details>
            <summary>Instructions</summary>
            <pre>{skill.instructions}</pre>
          </details>
        {/if}
      </article>
    {:else}
      <p class="hint">No saved skills yet. After Talo helps you with something repeatable, use <strong>Save as skill</strong> in the chat, or let it offer to save the steps.</p>
    {/each}
  </section>
</div>

<style>
  .skills-page{padding-bottom:30px}
  .error{padding:12px 14px;background:var(--error-bg);border:1px solid var(--error-line);color:var(--error-text);border-radius:10px;font-size:12px;margin:0 0 16px}
  section{background:var(--paper);border:1px solid var(--line);border-radius:16px;padding:22px 24px;min-width:0}
  h2{font-size:17px;margin:0 0 6px;letter-spacing:-.4px;font-weight:550;color:var(--text)}
  section>p{font-size:12.5px;line-height:1.65;color:var(--muted);margin:0 0 6px}
  h3{font-size:14px;margin:0;font-weight:550;color:var(--text)}
  article{background:var(--soft);border:1px solid var(--line);border-radius:12px;padding:14px 16px;margin-top:12px}
  article.disabled{opacity:.68}
  .head{display:flex;gap:14px;align-items:start;justify-content:space-between;flex-wrap:wrap}
  small{font-size:11px;color:var(--faint);line-height:1.5;display:block;margin-top:3px}
  .description{font-size:12.5px;color:var(--muted);line-height:1.6;margin:9px 0 0}
  details{margin-top:10px}
  summary{cursor:pointer;font-size:11.5px;color:var(--faint);text-transform:uppercase;letter-spacing:.06em}
  pre{white-space:pre-wrap;overflow-wrap:anywhere;font-size:12.5px;line-height:1.65;color:var(--text);background:var(--paper);border:1px solid var(--line);border-radius:10px;padding:12px 14px;margin:9px 0 0;max-height:420px;overflow:auto}
  form,label{display:grid;gap:7px}
  form{gap:13px}
  label{font-size:12px;color:var(--muted);font-weight:550}
  input:not([type=checkbox]),textarea,select{width:100%;box-sizing:border-box;border:1px solid var(--line-strong);background:var(--soft);border-radius:8px;padding:9px 11px;color:var(--text);font:inherit;font-size:12.5px;min-width:0}
  input:not([type=checkbox]):hover,textarea:hover,select:hover{border-color:var(--hover-line)}
  textarea{resize:vertical;line-height:1.6;font-family:var(--font-mono,inherit)}
  button{font:inherit;font-size:12px;border:1px solid var(--line-strong);border-radius:8px;padding:8px 13px;background:var(--soft);color:var(--text);cursor:pointer;line-height:1.35}
  button:hover:not(:disabled){background:var(--hover-bg);border-color:var(--hover-line)}
  button:disabled{opacity:.45;cursor:not-allowed}
  .primary{background:var(--yellow);border-color:var(--yellow);color:var(--black);font-weight:600;padding:9px 16px}
  .primary:hover:not(:disabled){background:var(--accent-bright);border-color:var(--accent-bright)}
  .danger{border-color:var(--error-line);color:var(--error-text)}
  .actions{display:flex;flex-wrap:wrap;gap:8px}
  .hint{font-size:11.5px;color:var(--faint);line-height:1.6;margin:8px 0 0}
</style>
