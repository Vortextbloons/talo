<script lang="ts">
  import { onMount } from 'svelte';
  import { chat, openConversation } from '../lib/chat.svelte';
  import { library, refreshLibrary, libraryCall, mutateLibrary, switchWorkspace, type HistoryHit } from '../lib/library.svelte';
  let { showArchived = $bindable(false) }: { showArchived?: boolean } = $props();
  let query = $state('');
  let hits = $state<HistoryHit[]>([]);
  let searched = $state(false);
  let searching = $state(false);
  let editing = $state(false);
  let title = $state('');
  let deleting = $state(false);
  let searchVersion=0;
  let rebuildNotice=$state('');
  let open = $state(false);
  const activeWorkspaceName = $derived(library.workspaces.find(w=>w.id===library.activeWorkspace)?.name ?? 'General');
  async function rebuild(){
    library.busy=true; library.error='';
    try { const result=await libraryCall<{unavailableConversations:string[]}>('index_rebuild');rebuildNotice=result.unavailableConversations.length?'Search rebuilt. Some saved histories are unavailable; their cached messages were kept.':'Search rebuilt from saved history.'; }
    catch(e){library.error=String(e);}finally{library.busy=false;}
  }
  const working = $derived(chat.sending || chat.active?.state==='generating' || chat.active?.state==='cancelling');
  onMount(() => { void refreshLibrary(); });
  async function search() {
    const version=++searchVersion; searching=true; library.error='';
    try { const result=await libraryCall<HistoryHit[]>('history_search',{query,workspaceId:library.activeWorkspace}); if(version===searchVersion){hits=result;searched=true;} }
    catch(e){library.error=String(e);} finally{if(version===searchVersion) searching=false;}
  }
  async function update(remove=false, archive=false) {
    const c=chat.active?.conversation; if(!c)return;
    if(await mutateLibrary(remove?'conversation_delete':'conversation_update',{id:c.id,name:title||c.title,workspaceId:c.workspaceId??library.activeWorkspace,archived:archive})){editing=false;deleting=false;}
  }
</script>
<div class="library-controls">
  <button type="button" class="controls-toggle" aria-expanded={open} onclick={()=>open=!open}>
    <span>Project · {activeWorkspaceName}</span>
    <svg class="chevron" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><path d="m6 9.5 6 6 6-6" /></svg>
  </button>
  {#if open}
  <div class="library-body">
  <label>Project<select aria-label="Current project" value={library.activeWorkspace} disabled={working || library.busy} onchange={e=>{query='';hits=[];searched=false;void switchWorkspace(e.currentTarget.value);}}>{#each library.workspaces.filter(w=>!w.archived) as w(w.id)}<option value={w.id}>{w.name}</option>{/each}</select></label>
  <form onsubmit={e=>{e.preventDefault();void search();}}><input aria-label="Search conversations" placeholder="Search conversations" bind:value={query} maxlength="300"/><button disabled={searching || !query.trim()} aria-label="Search history">{searching?'…':'Find'}</button></form>
  {#if searched}<div class="search-results"><button class="clear" onclick={()=>{searched=false;query='';}}>Clear search</button>{#each hits as hit, i(`${hit.conversationId}-${i}`)}<button disabled={working} onclick={()=>{void openConversation(hit.conversationId);}}><strong>{hit.title}</strong><small>{hit.text}</small></button>{:else}<p>No conversations found.</p>{/each}</div>{/if}
  <label class="archive"><input type="checkbox" bind:checked={showArchived}/> Show archived chats</label>
  {#if chat.active}<button class="manage" disabled={working} onclick={()=>{title=chat.active?.conversation.title??'';editing=!editing;}}>Manage this chat</button>{/if}
  {#if editing}<form class="edit-chat" onsubmit={e=>{e.preventDefault();void update();}}><label>Chat title<input bind:value={title} maxlength="100" required/></label><button disabled={library.busy}>Save title</button><button type="button" disabled={library.busy} onclick={()=>{void update(false,!chat.active?.conversation.archived);}}>{chat.active?.conversation.archived?'Restore chat':'Archive chat'}</button><button type="button" onclick={()=>deleting=true}>Delete chat</button>{#if deleting}<p>Delete this chat from Talo? Its original history file stays on this device.</p><button type="button" disabled={library.busy} onclick={()=>{void update(true);}}>Confirm delete</button>{/if}</form>{/if}
  <button disabled={working || library.busy || chat.preview} onclick={()=>{void rebuild();}}>Rebuild search</button>
  {#if rebuildNotice}<p role="status">{rebuildNotice}</p>{/if}
  {#if library.error}<p role="alert">{library.error}</p>{/if}
  </div>
  {/if}
</div>
<style>
  .library-controls{padding:2px 2px 12px;display:grid;gap:9px;border-bottom:1px solid var(--line);margin-bottom:12px;font-size:12px}
  .controls-toggle{display:flex;align-items:center;justify-content:space-between;gap:8px;border:1px solid var(--line);background:var(--soft);color:var(--muted);border-radius:8px;padding:7px 11px;font-size:11.5px;cursor:pointer;line-height:1.35}
  .controls-toggle:hover{border-color:#4a4a41;color:var(--text)}
  .controls-toggle span{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .chevron{width:11px;height:11px;flex-shrink:0;transition:transform .15s ease}
  .controls-toggle[aria-expanded="true"] .chevron{transform:rotate(180deg)}
  .library-body{display:grid;gap:9px}
  label{display:grid;gap:5px;font-size:10px;letter-spacing:.08em;text-transform:uppercase;color:var(--faint)}
  input,select{display:block;width:100%;padding:8px 10px;border:1px solid var(--line-strong);border-radius:8px;background:var(--soft);color:var(--text);font:inherit;font-size:12px;box-sizing:border-box;min-width:0}
  input[type=checkbox]{width:auto;accent-color:var(--yellow);padding:0}
  input:hover:not([type=checkbox]):not(:disabled),select:hover:not(:disabled){border-color:#4a4a41}
  form{display:flex;gap:6px}
  form input{flex:1}
  form button{flex-shrink:0;padding:7px 12px}
  button{border:1px solid var(--line-strong);background:var(--soft);color:var(--text);border-radius:8px;padding:7px 11px;font-size:11.5px;cursor:pointer;line-height:1.35;min-width:0}
  button:hover:not(:disabled){background:#2c2c27;border-color:#4a4a41}
  button:disabled{opacity:.45;cursor:not-allowed}
  .archive{display:flex;align-items:center;gap:8px;font-size:12px;letter-spacing:0;text-transform:none;color:var(--muted);cursor:pointer;user-select:none}
  .manage{text-align:left;width:100%}
  .library-body>button{text-align:left;width:100%}
  .edit-chat{display:grid;gap:8px}
  .edit-chat label{display:grid;gap:5px;text-transform:none;letter-spacing:0;font-size:12px;color:var(--muted)}
  .edit-chat button{text-align:left}
  .search-results{display:grid;gap:6px;max-height:260px;overflow:auto;scrollbar-gutter:stable}
  .search-results button{text-align:left;display:grid;gap:4px;background:transparent;border:1px solid var(--line);padding:8px 10px}
  .search-results button:hover:not(:disabled){border-color:var(--yellow-deep);background:#1d1d19}
  .search-results strong{font-size:12px;font-weight:550;color:var(--text)}
  .search-results small{font-size:11px;color:var(--muted);overflow:hidden;display:-webkit-box;line-clamp:2;-webkit-line-clamp:2;-webkit-box-orient:vertical}
  .clear{background:transparent;border:0;padding:2px 0;color:var(--faint);font-size:11px;justify-self:start;width:auto}
  .clear:hover:not(:disabled){background:transparent;border:0;color:var(--yellow-deep)}
  p{margin:0;font-size:11px;color:var(--muted);overflow-wrap:anywhere;line-height:1.5}
  @media(prefers-reduced-motion:reduce){.chevron{transition:none}}
</style>
