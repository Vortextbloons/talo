<script lang="ts">
  import { agent, agentAction } from '../lib/agent.svelte';
  import type { PendingAsk } from '../../../../packages/contracts/src/agent';

  let { ask, onOpenChat }: { ask: PendingAsk; onOpenChat?: () => void } = $props();

  let step = $state(0);
  let chosen = $state<Record<string, string>>({});
  let other = $state<Record<string, string>>({});
  let custom = $state<Record<string, boolean>>({});
  let sending = $state(false);
  let error = $state('');

  const questions = $derived(ask.questions);
  const index = $derived(Math.min(step, questions.length - 1));
  const current = $derived(questions[index]);
  const last = $derived(index >= questions.length - 1);

  const expires = $derived(new Date(ask.expiresAt).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }));

  function label(questionId: string, optionId: string): string {
    const question = ask.questions.find(item => item.id === questionId);
    return question?.options.find(option => option.id === optionId)?.label ?? optionId;
  }

  function draftProblem(questionId: string): string | null {
    if (!custom[questionId]) return null;
    return (other[questionId] ?? '').trim() ? null : 'Write your answer, or choose an option.';
  }

  async function submit(useDefaults = false): Promise<void> {
    const answers: Record<string, string | { other: string }> = {};
    for (const question of ask.questions) {
      if (useDefaults) { answers[question.id] = 'decided'; continue; }
      if (custom[question.id]) {
        const text = (other[question.id] ?? '').trim();
        if (!text) { error = 'Write your answer, or choose an option.'; return; }
        answers[question.id] = { other: text };
        continue;
      }
      answers[question.id] = chosen[question.id] ?? 'decided';
    }
    error = ''; sending = true;
    const ok = await agentAction('answer', { id: ask.id, answers });
    sending = false;
    if (!ok) error = 'That question could not be answered. It may have expired.';
  }

  function advance(): void {
    const problem = draftProblem(current.id);
    if (problem) { error = problem; return; }
    error = '';
    if (last) void submit();
    else step = index + 1;
  }

  function retreat(): void {
    error = '';
    if (index > 0) step = index - 1;
  }

  function defaultSummary(): string {
    return ask.questions.map(question => label(question.id, question.recommended)).join(' · ');
  }
</script>

<aside class="ask" aria-label="Question from Talo">
  <header>
    <div class="head-row">
      <span class="tag">Talo asks</span>
      {#if onOpenChat}<button class="link" type="button" onclick={onOpenChat}>Open chat</button>{/if}
    </div>
    <h3>{ask.header}</h3>
    <p class="hint">
      If you don’t answer by {expires}, Talo continues with
      <strong>{defaultSummary()}</strong>.
      {#if ask.taskTitle}<span class="task-note">From delegated task: {ask.taskTitle}.</span>{/if}
    </p>
  </header>
  <fieldset>
    <legend>{current.question}{#if questions.length > 1}<span class="step">{step + 1} of {questions.length}</span>{/if}</legend>
    <div class="options">
      {#each current.options as option (option.id)}
        <label class="option" class:selected={!custom[current.id] && chosen[current.id] === option.id}>
          <input
            type="radio"
            name={`${ask.id}-${current.id}`}
            value={option.id}
            checked={!custom[current.id] && chosen[current.id] === option.id}
            disabled={sending}
            onchange={() => { custom[current.id] = false; chosen[current.id] = option.id; }}
          />
          <span>
            <strong>{option.label}</strong>
            {#if option.id === current.recommended}<em class="rec">Talo’s pick</em>{/if}
            {#if option.detail}<small>{option.detail}</small>{/if}
          </span>
        </label>
      {/each}
      {#if current.other}
        <label class="option" class:selected={custom[current.id]}>
          <input
            type="radio"
            name={`${ask.id}-${current.id}`}
            checked={custom[current.id] === true}
            disabled={sending}
            onchange={() => { custom[current.id] = true; chosen[current.id] = ''; }}
          />
          <span><strong>Something else</strong></span>
        </label>
      {/if}
    </div>
    {#if custom[current.id]}
      <input
        class="other"
        maxlength="400"
        placeholder="Say what you want instead"
        bind:value={other[current.id]}
        disabled={sending}
      />
    {/if}
  </fieldset>
  {#if questions.length > 1}
    <div class="pager" aria-label="Question {step + 1} of {questions.length}">
      {#each questions as question, position (question.id)}
        <span class="pip" class:active={position === index} class:done={position < index}></span>
      {/each}
    </div>
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  <div class="actions">
    {#if index > 0}
      <button type="button" onclick={retreat} disabled={sending || agent.busy}>Back</button>
    {/if}
    <button class="ghost" type="button" disabled={sending || agent.busy} onclick={() => void submit(true)}>You decide</button>
    <button class="primary" type="button" disabled={sending || agent.busy} onclick={advance}>{last ? 'Send answer' : 'Next'}</button>
  </div>
</aside>

<style>
  .ask{background:var(--paper);border:1px solid var(--line-strong);border-radius:15px;padding:14px 16px;margin:10px 0 0;box-shadow:0 10px 26px var(--shadow)}
  header{margin-bottom:8px}
  .head-row{display:flex;align-items:center;gap:8px}
  .tag{font-size:10px;letter-spacing:.09em;text-transform:uppercase;color:var(--yellow);font-weight:600}
  h3{font-size:14.5px;margin:4px 0 6px;font-weight:550;letter-spacing:-.2px;color:var(--text)}
  .hint{font-size:11.5px;line-height:1.6;color:var(--muted);margin:0}
  .hint strong{color:var(--text);font-weight:550}
  .task-note{display:inline-block;margin-left:6px;color:var(--yellow-deep)}
  .link{background:none;border:0;padding:0;margin-left:auto;color:var(--yellow);font:inherit;font-size:11.5px;text-decoration:underline;cursor:pointer}
  fieldset{border:0;padding:8px 0 0;margin:0}
  legend{font-size:12.5px;font-weight:550;color:var(--text);padding:0 0 6px;display:flex;align-items:baseline;gap:8px}
  .step{font-size:10px;color:var(--faint);font-weight:500;letter-spacing:.04em}
  .options{display:grid;gap:6px}
  .option{display:flex;gap:9px;align-items:start;padding:7px 10px;border:1px solid var(--line);border-radius:9px;background:var(--soft);cursor:pointer;font-size:12.5px;line-height:1.5}
  .option:hover{border-color:var(--hover-line)}
  .option.selected{border-color:var(--yellow-deep);background:var(--accent-bg)}
  .option input{accent-color:var(--yellow);margin-top:2px;flex-shrink:0}
  .option strong{display:block;font-weight:550;color:var(--text)}
  .rec{display:inline-block;font-style:normal;font-size:10px;color:var(--yellow);margin-left:6px;letter-spacing:.03em}
  .option small{display:block;color:var(--muted);font-size:11px;margin-top:1px}
  .other{margin-top:7px;width:100%;box-sizing:border-box;border:1px solid var(--line-strong);background:var(--soft);border-radius:8px;padding:8px 10px;color:var(--text);font:inherit;font-size:12.5px}
  .pager{display:flex;gap:5px;justify-content:center;margin-top:10px}
  .pip{width:5px;height:5px;border-radius:50%;background:var(--line-strong)}
  .pip.done{background:var(--yellow-deep)}
  .pip.active{background:var(--yellow);box-shadow:0 0 0 3px color-mix(in srgb,var(--accent) 18%,transparent)}
  .actions{display:flex;gap:8px;margin-top:10px;flex-wrap:wrap}
  .actions .ghost{margin-left:auto}
  button{font:inherit;font-size:12px;border:1px solid var(--line-strong);border-radius:8px;padding:8px 13px;background:var(--soft);color:var(--text);cursor:pointer}
  button:disabled{opacity:.45;cursor:not-allowed}
  .primary{background:var(--yellow);border-color:var(--yellow);color:var(--black);font-weight:600}
  .primary:hover:not(:disabled){background:var(--accent-bright);border-color:var(--accent-bright)}
  .ghost{border-color:transparent;background:transparent;color:var(--muted)}
  .ghost:hover:not(:disabled){color:var(--text)}
  .error{font-size:11.5px;color:var(--error-text);margin:8px 0 0}
</style>
