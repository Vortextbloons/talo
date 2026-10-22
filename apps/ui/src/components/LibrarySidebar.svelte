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
  <label>Workspace<select aria-label="Current workspace" value={library.activeWorkspace} disabled={working || library.busy} onchange={e=>{query='';hits=[];searched=false;void switchWorkspace(e.currentTarget.value);}}>{#each library.workspaces.filter(w=>!w.archived) as w(w.id)}<option value={w.id}>{w.name}</option>{/each}</select></label>
  <form onsubmit={e=>{e.preventDefault();void search();}}><input aria-label="Search conversations" placeholder="Search conversations" bind:value={query} maxlength="300"/><button disabled={searching || !query.trim()} aria-label="Search history">{searching?'…':'Find'}</button></form>
  {#if searched}<div class="search-results"><button onclick={()=>{searched=false;query='';}}>Clear search</button>{#each hits as hit, i(`${hit.conversationId}-${i}`)}<button disabled={working} onclick={()=>{void openConversation(hit.conversationId);}}><strong>{hit.title}</strong><small>{hit.text}</small></button>{:else}<p>No conversations found.</p>{/each}</div>{/if}
  <label class="archive"><input type="checkbox" bind:checked={showArchived}/> Show archived chats</label>
  {#if chat.active}<button class="manage" disabled={working} onclick={()=>{title=chat.active?.conversation.title??'';editing=!editing;}}>Manage this chat</button>{/if}
  {#if editing}<form class="edit-chat" onsubmit={e=>{e.preventDefault();void update();}}><label>Chat title<input bind:value={title} maxlength="100" required/></label><button disabled={library.busy}>Save title</button><button type="button" disabled={library.busy} onclick={()=>{void update(false,!chat.active?.conversation.archived);}}>{chat.active?.conversation.archived?'Restore chat':'Archive chat'}</button><button type="button" onclick={()=>deleting=true}>Delete chat</button>{#if deleting}<p>Delete this chat from Talo? Its original history file stays on this device.</p><button type="button" disabled={library.busy} onclick={()=>{void update(true);}}>Confirm delete</button>{/if}</form>{/if}
  <button disabled={working || library.busy || chat.preview} onclick={()=>{void rebuild();}}>Rebuild search</button>
  {#if rebuildNotice}<p role="status">{rebuildNotice}</p>{/if}
  {#if library.error}<p role="alert">{library.error}</p>{/if}
</div>
<style>
  .library-controls{padding:0 12px 12px;display:grid;gap:10px;border-bottom:1px solid var(--border,#ddd);margin-bottom:10px;font-size:12px}label{display:grid;gap:5px;color:#676767}input,select,button{font:inherit;min-width:0}select,input:not([type=checkbox]){width:100%;padding:8px;border:1px solid #d8d8d8;border-radius:7px;background:white;color:#171717;box-sizing:border-box}form{display:flex;gap:5px}form input{min-width:0}button{border:1px solid #d8d8d8;border-radius:6px;background:white;padding:7px;color:#242424;cursor:pointer}button:hover{background:#fff3af}button:disabled{opacity:.5;cursor:default}.archive{display:flex;align-items:center;gap:7px}.manage{text-align:left}.edit-chat{display:grid}.search-results{display:grid;gap:5px;max-height:260px;overflow:auto}.search-results button{text-align:left;display:grid;gap:5px}.search-results small{overflow:hidden;display:-webkit-box;line-clamp:2;-webkit-line-clamp:2;-webkit-box-orient:vertical}p{margin:3px 0;overflow-wrap:anywhere}
</style>
