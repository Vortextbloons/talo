<script lang="ts">
  import { onMount } from 'svelte';
  import { library, refreshLibrary, mutateLibrary, libraryCall, switchWorkspace, type Memory, type Workspace } from '../lib/library.svelte';
  import { agentAction } from '../lib/agent.svelte';
  let { mode }: { mode:'Workspaces'|'Memory' } = $props();
  let name=$state('');let folder=$state('');let folderConsent=$state(false);let workspaceId=$state<string|null>(null);
  let instructions=$state('');let pinned=$state('');let memoryScope=$state<'inherit'|'workspace'|'off'>('inherit');
  let capabilities=$state<string[]>([]);let renamingAgent=$state<string|null>(null);
  let scope=$state<'global'|'workspace'>('workspace');let query=$state('');let content=$state('');let source=$state('');let personal=$state(true);let consent=$state(false);
  let memoryId=$state<string|null>(null);let memories=$state<Memory[]>([]);let history=$state(false);let deleting=$state<string|null>(null);let loading=$state(false);
  let requestVersion=0;
  onMount(()=>{void refreshLibrary().then(loadMemories);});
  async function loadMemories(){const version=++requestVersion;loading=true;try{const result=await libraryCall<Memory[]>('memory_list',{scope,workspaceId:library.activeWorkspace,query,history});if(version===requestVersion)memories=result;}catch(e){library.error=String(e);}finally{if(version===requestVersion)loading=false;}}
  function resetWorkspace(){workspaceId=null;name='';folder='';folderConsent=false;instructions='';pinned='';memoryScope='inherit';capabilities=[];}
  function editWorkspace(w:Workspace){workspaceId=w.id;name=w.name;folder=w.folder??'';folderConsent=false;instructions=w.instructions??'';pinned=(w.pinned??[]).join('\n');memoryScope=w.memoryScope??'inherit';capabilities=[...(w.capabilities??[])];}
  function toggleCapability(value:'web'|'browser'|'files'|'google',on:boolean){capabilities=on?[...new Set([...capabilities,value])]:capabilities.filter(item=>item!==value);}
  async function saveWorkspace(){
    const notes=pinned.split('\n').map(line=>line.trim()).filter(Boolean).slice(0,8);
    const saved=await mutateLibrary('workspace_save',{id:workspaceId??undefined,name,folder,folderConsent,instructions,pinned:notes,memoryScope,capabilities});
    if(saved)resetWorkspace();
  }
  async function letAgentEdit(w:Workspace){renamingAgent=w.id;await agentAction('space_permission',{id:w.id,agentOwned:!(w.agentOwned??false)});await refreshLibrary();renamingAgent=null;}
  function editMemory(m:Memory){memoryId=m.id;scope=m.scope;content=m.content;source=m.source;personal=m.personal;consent=false;}
  function resetMemory(){memoryId=null;content='';source='';personal=true;consent=false;}
  async function saveMemory(){library.error='';library.busy=true;try{await libraryCall(memoryId?'memory_update':'memory_save',{id:memoryId,scope,workspaceId:library.activeWorkspace,content,source:source||'User memory editor',personal,consent});resetMemory();await loadMemories();}catch(e){library.error=String(e);}finally{library.busy=false;}}
  async function removeMemory(id:string){try{await libraryCall('memory_delete',{id});deleting=null;await loadMemories();}catch(e){library.error=String(e);}}
</script>
<div class="library-page">
  {#if library.error}<p class="error" role="alert">{library.error}</p>{/if}
  {#if mode==='Workspaces'}
    <div class="library-grid"><section><h2>Your projects</h2><p>Projects group related chats, instructions, and memories. You talk to the same Talo in every project. Archive a project when you are done; its chats and memories stay saved, and you can restore it later. Archiving does not stop scheduled tasks.</p>{#each library.workspaces as w(w.id)}<article><div><h3>{w.name}{w.archived?' · Archived':''}{w.agentOwned?' · Talo can edit':''}</h3><p>{w.folder||'No project folder linked'}</p></div><div class="actions"><button disabled={w.archived||library.busy} onclick={()=>{void switchWorkspace(w.id).then(loadMemories);}}>{library.activeWorkspace===w.id?'Selected':'Switch'}</button><button onclick={()=>editWorkspace(w)}>Edit</button>{#if w.id!=='general'}<button disabled={library.busy} onclick={()=>{void mutateLibrary('workspace_archive',{id:w.id,archived:!w.archived});}}>{w.archived?'Restore project':'Archive project'}</button><button disabled={renamingAgent===w.id} title="Talo may keep updating this project's brief, notes and capabilities" onclick={()=>void letAgentEdit(w)}>{w.agentOwned?'Stop Talo editing':'Let Talo edit'}</button><button onclick={()=>deleting=w.id}>Delete</button>{/if}</div>{#if deleting===w.id}<p>Delete this project, its chat listings, and its memories? Original conversation files stay on this device.</p><div class="actions"><button class="primary" disabled={library.busy} onclick={()=>{void mutateLibrary('workspace_delete',{id:w.id}).then(ok=>{if(ok)deleting=null;});}}>Confirm delete</button><button onclick={()=>deleting=null}>Cancel</button></div>{/if}</article>{/each}</section>
    <section><h2>{workspaceId?'Edit project':'New project'}</h2><form onsubmit={e=>{e.preventDefault();void saveWorkspace();}}><label>Name<input required maxlength="100" bind:value={name}/></label><label>Project folder (optional)<input bind:value={folder} placeholder="C:\Projects\My project"/></label>{#if folder}<label class="check"><input type="checkbox" bind:checked={folderConsent}/><span>Allow Talo to link this folder. File access remains subject to Windows permissions.</span></label>{/if}
      <fieldset><legend>Project brief</legend>
        <label>How should Talo work here? (optional)<textarea rows="4" maxlength="4000" bind:value={instructions} placeholder="Decisions, tone and constraints that apply to every chat in this project."></textarea></label>
        <label>Pinned notes (one per line, up to 8)<textarea rows="3" maxlength="400" bind:value={pinned} placeholder="A launch date&#10;The invoice folder"></textarea></label>
        <div class="fields"><label>Memory in this project<select bind:value={memoryScope}><option value="inherit">Use saved memory</option><option value="workspace">Only this project's memory</option><option value="off">No saved memory here</option></select></label></div>
        <div class="check-row"><span class="check-label">Capabilities</span>{#each [['web','Web research'],['browser','Browser'],['files','Files'],['google','Google']] as [value,text](value)}<label class="check"><input type="checkbox" checked={capabilities.includes(value)} onchange={e=>toggleCapability(value as 'web'|'browser'|'files'|'google',e.currentTarget.checked)}/><span>{text}</span></label>{/each}</div>
        <p class="hint">Leave every capability unchecked to allow everything. The brief and notes are added to each turn; they are not saved as memory.</p>
      </fieldset>
      <div class="actions"><button class="primary" disabled={library.busy || (!!folder&&!folderConsent)}>{workspaceId?'Save project':'Create project'}</button>{#if workspaceId}<button type="button" onclick={resetWorkspace}>Cancel</button>{/if}</div></form></section></div>
  {:else}
    <div class="memory-toolbar"><label class="check"><input type="checkbox" checked={library.memoryEnabled} disabled={library.busy} onchange={e=>{void mutateLibrary('memory_enable',{enabled:e.currentTarget.checked});}}/><span>Use saved memory in chats</span></label><label>Project<select value={library.activeWorkspace} onchange={e=>{resetMemory();void switchWorkspace(e.currentTarget.value).then(loadMemories);}}>{#each library.workspaces.filter(w=>!w.archived) as w(w.id)}<option value={w.id}>{w.name}</option>{/each}</select></label></div>
    <div class="library-grid"><section><h2>Saved memory{memories.length ? ` · ${memories.length}` : ''}</h2><p>Talo keeps these notes between chats, and only on this PC. Correct one with Edit; remove it with Delete.</p><form class="filters" onsubmit={e=>{e.preventDefault();void loadMemories();}}><label>Scope<select bind:value={scope} onchange={()=>{resetMemory();void loadMemories();}}><option value="workspace">This project</option><option value="global">Every project</option></select></label><label>Search<input bind:value={query} placeholder="Find a memory"/></label><button>Search</button></form><label class="check"><input type="checkbox" bind:checked={history} onchange={()=>{void loadMemories();}}/><span>Show superseded versions</span></label>{#if loading}<p>Loading memory…</p>{/if}{#each memories as m(m.id)}<article><p class="memory-content">{m.content}</p><small>{m.scope==='global'?'Every project':'This project'} · {m.source} · {m.updatedAt}{m.supersededBy?' · Superseded':''}</small><div class="actions"><button disabled={!!m.supersededBy} onclick={()=>editMemory(m)}>Edit</button><button onclick={()=>deleting=m.id}>Delete</button></div>{#if deleting===m.id}<p>Delete this memory? It and its older versions are removed from this PC.</p><div class="actions"><button disabled={library.busy} onclick={()=>{void removeMemory(m.id);}}>Delete memory</button><button onclick={()=>deleting=null}>Keep it</button></div>{/if}</article>{:else}{#if !loading}<p class="empty-memory">Nothing saved here yet. Add a decision or preference Talo should keep.</p>{/if}{/each}</section>
    <section><h2>{memoryId?'Correct this memory':'Teach Talo something'}</h2><p>{memoryId?'Saving keeps the old version as superseded, so nothing is lost silently.':'A decision, a requirement or a preference Talo should remember next time.'}</p><form onsubmit={e=>{e.preventDefault();void saveMemory();}}><label>What should Talo remember?<textarea required maxlength="16000" rows="7" bind:value={content} placeholder="A project decision, requirement, or preference…"></textarea></label><label>Source<input maxlength="500" bind:value={source} placeholder="For example: project planning, October 21"/></label><label class="check"><input type="checkbox" bind:checked={personal}/><span>Contains a personal preference or personal information</span></label>{#if personal}<label class="check"><input type="checkbox" bind:checked={consent}/><span>I agree to save this personal information on this device.</span></label>{/if}<p class="hint">{scope==='global'?'Used in every project.':'Used only in the selected project.'} Editing keeps the old version as superseded. Deleting removes its history too.</p><div class="actions"><button class="primary" disabled={library.busy||!content.trim()||(personal&&!consent)}>Save memory</button>{#if memoryId}<button type="button" onclick={resetMemory}>Cancel edit</button>{/if}</div></form></section></div>
  {/if}
</div>
<style>
  .library-page{padding-bottom:30px}
  .error{padding:12px 14px;background:var(--error-bg);border:1px solid var(--error-line);color:var(--error-text);border-radius:10px;font-size:12px;margin:0 0 16px}
  .library-grid{display:grid;grid-template-columns:minmax(0,1.2fr) minmax(280px,1fr);gap:22px;align-items:start}
  section{background:var(--paper);border:1px solid var(--line);border-radius:16px;padding:22px 24px;min-width:0}
  h2{font-size:17px;margin:0 0 6px;letter-spacing:-.4px;font-weight:550;color:var(--text)}
  section>p{font-size:12.5px;line-height:1.65;color:var(--muted);margin:0 0 6px}
  h3{font-size:14px;margin:0;font-weight:550;color:var(--text)}
  article{background:var(--soft);border:1px solid var(--line);border-radius:12px;padding:14px 16px;margin-top:12px}
  article>p{margin:0 0 10px}
  .memory-content{white-space:pre-wrap;overflow-wrap:anywhere;font-size:13px;line-height:1.65;color:var(--text)}
  article small{font-size:11px;color:var(--faint);line-height:1.5;display:block}
  form,label{display:grid;gap:7px}
  form{gap:15px}
  label{font-size:12px;color:var(--muted);font-weight:550}
  input:not([type=checkbox]),textarea,select{width:100%;box-sizing:border-box;border:1px solid var(--line-strong);background:var(--soft);border-radius:8px;padding:9px 11px;color:var(--text);font:inherit;font-size:12.5px;min-width:0}
  input:not([type=checkbox]):hover,textarea:hover,select:hover{border-color:#4a4a41}
  textarea{resize:vertical;line-height:1.6}
  .check{display:flex;gap:9px;align-items:start;line-height:1.5;font-weight:400;color:var(--muted);cursor:pointer}
  .check input{accent-color:var(--yellow);margin-top:2px;flex-shrink:0}
  button{font:inherit;font-size:12px;border:1px solid var(--line-strong);border-radius:8px;padding:8px 13px;background:var(--soft);color:var(--text);cursor:pointer;line-height:1.35}
  button:hover:not(:disabled){background:#2c2c27;border-color:#4a4a41}
  button:disabled{opacity:.45;cursor:not-allowed}
  .primary{background:var(--yellow);border-color:var(--yellow);color:var(--black);font-weight:600;padding:9px 16px}
  .primary:hover:not(:disabled){background:#ffdd7f;border-color:#ffdd7f}
  .actions{display:flex;flex-wrap:wrap;gap:8px;margin-top:12px}
  fieldset{border:1px solid var(--line);border-radius:12px;padding:14px 16px 16px;margin:0;display:grid;gap:13px}
  legend{font-size:12px;font-weight:600;color:var(--text);padding:0 6px}
  .check-row{display:flex;flex-wrap:wrap;gap:14px;align-items:center}
  .check-label{font-size:12px;color:var(--muted);font-weight:550;width:100%}
  .check-row .check{align-items:center}
  .memory-toolbar{display:flex;gap:20px;align-items:end;flex-wrap:wrap;margin:0 0 20px}
  .memory-toolbar label{margin:0}
  .memory-toolbar .check{align-items:center;padding:9px 0}
  .filters{display:flex;align-items:end;gap:8px;margin:16px 0 14px}
  .filters label{flex:1;min-width:0}
  .filters button{flex-shrink:0;padding:9px 14px}
  small,.hint{font-size:11px;color:var(--faint);line-height:1.55}
  .hint{margin:0}
  .empty-memory{font-size:12.5px;color:var(--muted)}
  @media(max-width:900px){.library-grid{grid-template-columns:1fr}.memory-toolbar{align-items:start;flex-direction:column;gap:12px}}
</style>
