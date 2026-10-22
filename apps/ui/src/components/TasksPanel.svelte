<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { chat, openConversation } from '../lib/chat.svelte';
  let { onOpenChat, focusRun = '' }: { onOpenChat: () => void; focusRun?: string } = $props();
  type Task = { id:string; title:string; prompt:string; localTime:string; timezone:string; recurrence:string; paused:boolean; nextAt:number|null; timeoutSeconds:number; connectionId:string; modelId:string };
  type Run = { id:string; taskId:string; title:string; status:string; scheduledAt:number; startedAt:number|null; finishedAt:number|null; conversationId:string|null; error:string|null; usage?:{cost:number;tokens:number;toolCalls:number}|null };
  let resources=$state<{memoryBytes:number;cpuPercent:number}|null>(null);
  let tasks=$state<Task[]>([]), runs=$state<Run[]>([]), paused=$state(false), runtime=$state('sleeping'), error=$state(''), busy=$state(false);
  let draft=$state<Task|null>(null);
  const running=$derived(runs.filter(r=>['running','awaiting_approval'].includes(r.status)));
  const attention=$derived(runs.filter(r=>['failed','uncertain','awaiting_approval'].includes(r.status)));
  const history=$derived(runs.filter(r=>!['running','awaiting_approval'].includes(r.status)));
  const date=(at:number|null)=>at===null?'—':new Date(at*1000).toLocaleString();
  async function refresh(){
    if(!isTauri())return;
    try{const state=await invoke<{tasks:Task[];runs:Run[];paused:boolean;runtime:string;resources:typeof resources}>('automation_action',{operation:'state'});tasks=state.tasks;runs=state.runs;paused=state.paused;runtime=state.runtime;resources=state.resources;}catch(e){error=String(e);}
  }
  async function action(operation:string,payload:unknown){
    if(!isTauri())return;
    busy=true;error='';
    try{await invoke('automation_action',{operation,payload});await refresh();if(operation==='save')draft=null;}catch(e){error=String(e);}finally{busy=false;}
  }
  function create(){
    const selection=chat.active?.conversation.selection??chat.draftSelection;
    const when=new Date(Date.now()+3600000);
    const localTime=new Date(when.getTime()-when.getTimezoneOffset()*60000).toISOString().slice(0,16);
    draft={id:'',title:'',prompt:'',localTime,timezone:Intl.DateTimeFormat().resolvedOptions().timeZone,recurrence:'once',paused:false,nextAt:null,timeoutSeconds:600,connectionId:selection?.connectionId??'',modelId:selection?.modelId??''};
  }
  async function open(run:Run){if(run.conversationId&&await openConversation(run.conversationId))onOpenChat();else error=chat.error||'No conversation was created for this run.';}
  onMount(()=>{
    let active=true;const stops:(()=>void)[]=[];
    void refresh();
    if(isTauri())for(const event of ['automation-changed','runtime-status'])void listen(event,()=>void refresh()).then(stop=>active?stops.push(stop):stop());
    // Only this visible panel polls for progress; the native scheduler owns execution.
    const timer=setInterval(()=>void refresh(),5000);
    return()=>{active=false;stops.forEach(stop=>stop());clearInterval(timer);};
  });
</script>

<div class="automation-bar"><div><span class="lamp" class:paused></span><strong>{paused?'Automation paused':'Automation on'}</strong><small>Pi {runtime} · One task at a time</small></div><button disabled={busy||!isTauri()} onclick={()=>action('pause_automation',{paused:!paused})}>{paused?'Resume automation':'Pause automation'}</button><button class="primary" disabled={!isTauri()} onclick={create}>Create task</button></div>
{#if resources}<p class="hint">Local processes · {(resources.memoryBytes/1048576).toFixed(0)} MB RAM · {resources.cpuPercent.toFixed(1)}% CPU (100% = one core)</p>{/if}
{#if !isTauri()}<p>Open Talo on Windows to schedule tasks.</p>{/if}
{#if error}<p class="error" role="alert">{error}</p>{/if}
{#if draft}
  <form class="editor" onsubmit={event=>{event.preventDefault();if(draft)void action('save',draft);}}>
    <h2>{draft.id?'Edit task':'Create task'}</h2>
    <label>Title<input required maxlength="200" bind:value={draft.title}/></label>
    <label>What should Talo do?<textarea required maxlength="32000" rows="4" bind:value={draft.prompt}></textarea></label>
    <div class="fields"><label>Local date and time<input type="datetime-local" required bind:value={draft.localTime}/></label><label>Timezone<input required bind:value={draft.timezone}/></label><label>Repeat<select bind:value={draft.recurrence}><option value="once">Once</option><option value="daily">Daily</option><option value="weekly">Weekly</option></select></label></div>
    <div class="fields"><label>Provider<select required bind:value={draft.connectionId}>{#each chat.connections.filter(c=>c.connected) as connection (connection.id)}<option value={connection.id}>{connection.name}</option>{/each}</select></label><label>Model ID<input required bind:value={draft.modelId}/></label><label>Timeout (seconds)<input type="number" min="30" max="3600" required bind:value={draft.timeoutSeconds}/></label></div>
    <p class="hint">Talo runs while Windows is awake and you are signed in. Missed repeats are combined into one run. Sensitive actions wait for approval. Limits: 50 tool calls, 100,000 tokens or $1 estimated model usage; the final request can exceed the estimate.</p>
    <div class="actions"><button type="button" onclick={()=>draft=null}>Cancel</button><button class="primary" disabled={busy}>Save task</button></div>
  </form>
{/if}
<section><h2>Upcoming <span>{tasks.filter(t=>t.nextAt!==null&&!t.paused).length}</span></h2>
  {#each tasks as task (task.id)}
    <article><div><strong>{task.title}</strong><p>{task.paused?'Paused':task.nextAt===null?'Schedule complete':date(task.nextAt)} · {task.recurrence} · {task.timezone}</p></div><div class="actions"><button disabled={busy} onclick={()=>draft={...task}}>Edit</button><button disabled={busy} onclick={()=>action('pause',{id:task.id,paused:!task.paused})}>{task.paused?'Resume':'Pause'}</button><button disabled={busy} onclick={()=>action('delete',{id:task.id})}>Delete</button></div></article>
  {:else}<p class="hint">Schedule a reminder, a report, or a repeatable piece of work. Results stay in your conversations.</p>{/each}
</section>
{#snippet runRow(run:Run)}
  <article class:focused={run.id===focusRun}><div><strong>{run.title}</strong><p><span class="status">{run.status.replaceAll('_',' ')}</span> · {date(run.startedAt)}{#if run.finishedAt&&run.startedAt} · {run.finishedAt-run.startedAt}s{/if}</p>{#if run.usage}<p>{run.usage.tokens.toLocaleString()} tokens · ${run.usage.cost.toFixed(4)} estimated · {run.usage.toolCalls} tool checks</p>{/if}{#if run.error}<p class="error">{run.error}</p>{/if}</div><div class="actions">{#if run.conversationId}<button onclick={()=>open(run)}>View conversation</button>{/if}{#if ['running','awaiting_approval'].includes(run.status)}<button disabled={busy} onclick={()=>action('cancel',{id:run.id})}>Cancel run</button>{/if}</div></article>
{/snippet}
<section><h2>Running <span>{running.length}</span></h2>{#each running as run (run.id)}{@render runRow(run)}{:else}<p class="hint">Nothing running. Pi rests after two idle minutes.</p>{/each}</section>
<section><h2>Needs attention <span>{attention.length}</span></h2>{#each attention as run (run.id)}{@render runRow(run)}{:else}<p class="hint">No tasks need attention.</p>{/each}</section>
<section><h2>Execution history</h2>{#each history as run (run.id)}{@render runRow(run)}{:else}<p class="hint">Completed and interrupted runs appear here.</p>{/each}</section>
<style>
  .automation-bar{display:flex;align-items:center;gap:12px;border:1px solid var(--line-strong);border-left:4px solid var(--yellow);border-radius:10px;padding:18px;margin-bottom:24px}.automation-bar>div{flex:1}.automation-bar small{display:block;color:var(--muted);margin:6px 0 0 17px;font-size:12px}.lamp{display:inline-block;width:7px;height:7px;border-radius:50%;background:var(--yellow);margin-right:10px}.lamp.paused{background:var(--faint)}button{border:1px solid var(--line-strong);background:var(--soft);border-radius:7px;padding:9px 12px;font-size:12px}.primary{background:var(--yellow);color:var(--black);border-color:var(--yellow)}section{margin-bottom:24px}h2{font-size:16px;font-weight:600}h2 span{font-size:12px;color:var(--yellow);margin-left:8px}article{display:flex;align-items:center;justify-content:space-between;gap:16px;padding:16px 0;border-bottom:1px solid var(--line)}article>div:first-child{min-width:0}article strong{font-size:14px;overflow-wrap:anywhere}p{font-size:12px;color:var(--muted);line-height:1.6;margin:6px 0}.status{color:var(--yellow)}.actions{display:flex;gap:8px;flex-wrap:wrap}.focused{border-left:3px solid var(--yellow);padding-left:14px}.error{color:var(--error-text);overflow-wrap:anywhere}.editor{background:var(--paper);padding:24px;border:1px solid var(--line-strong);border-radius:12px;margin-bottom:24px}.editor label{display:flex;flex:1;flex-direction:column;gap:7px;font-size:12px;margin-bottom:16px}.fields{display:flex;gap:14px;flex-wrap:wrap}input,textarea,select{min-width:0;width:100%;background:var(--bg);border:1px solid var(--line-strong);border-radius:6px;padding:10px}textarea{resize:vertical}.hint{max-width:850px}.editor .actions{justify-content:flex-end}@media(max-width:750px){.automation-bar,article{align-items:flex-start;flex-wrap:wrap}.automation-bar>div{flex-basis:100%}.fields{display:block}}
</style>
