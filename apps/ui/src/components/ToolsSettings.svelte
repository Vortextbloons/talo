<script lang="ts">
  import { onMount } from 'svelte';
  import { isTauri } from '@tauri-apps/api/core';
  import { tools,refreshTools,toolAction } from '../lib/tools.svelte';
  import type { McpServerDefinition } from '../../../../packages/contracts/src/tools';
  let adding=$state(false), id=$state(''), transport=$state<'stdio'|'http'>('stdio');
  let command=$state(''), args=$state('[]'), url=$state(''), credential=$state(''), trusted=$state(false);
  onMount(()=>{void refreshTools();});
  async function add(){
    try{
      const server:McpServerDefinition={id,transport,enabled:true,trusted,...(transport==='stdio'?{command,args:JSON.parse(args)}:{url,credential})};
      if(await toolAction('save',{server})){adding=false;id='';command='';args='[]';url='';trusted=false;}
    }catch{tools.error='The extra arguments need to be a list, like ["--port","3000"].';}finally{credential='';}
  }
  async function toggle(server:typeof tools.state.servers[number]){
    const {hasCredential,state,tools:catalog,error,...definition}=server;
    await toolAction('save',{server:{...definition,enabled:!server.enabled}});
  }
</script>
<section class="tools-card" aria-labelledby="tools-heading">
  <div class="heading">
    <div><h2 id="tools-heading">Other apps</h2></div>
    <button disabled={!isTauri()||tools.busy} onclick={()=>adding=!adding}>{adding?'Cancel':'Add an app'}</button>
  </div>
  {#if tools.error}<p class="error" role="alert">{tools.error}</p>{/if}
  {#if adding}<form onsubmit={event=>{event.preventDefault();void add();}}>
    <label>What to call it<input required pattern={'[a-z][a-z0-9-]{0,47}'} bind:value={id} placeholder="notes" maxlength="48"/></label>
    <label>How it connects
      <select bind:value={transport}>
        <option value="stdio">An app installed on this PC</option>
        <option value="http">A web address</option>
      </select>
    </label>
    {#if transport==='stdio'}
      <label>App or script<input required bind:value={command} placeholder="C:\Tools\notes.exe"/></label>
      <details class="extra">
        <summary>Extra launch options</summary>
        <label>Arguments<input bind:value={args} spellcheck="false" placeholder='["--port","3000"]'/></label>
      </details>
    {:else}
      <label>Address<input required type="url" bind:value={url} placeholder="https://example.com/mcp"/></label>
      <label>Access token <span>(if it needs one)</span><input type="password" bind:value={credential} autocomplete="off"/></label>
    {/if}
    <label class="trust"><input type="checkbox" bind:checked={trusted} required/><span>I trust this app to run with my Windows account's access.</span></label>
    <button class="primary" disabled={tools.busy||!trusted}>Add app</button>
  </form>{/if}
  <div class="servers">
    {#each tools.state.servers as server (server.id)}<article>
      <div class="server-title"><strong>{server.id}</strong><span class:live={server.state==='connected'} class="status">{!server.enabled?'Off':server.state==='connected'?'Connected':server.state??'Not connected'}</span></div>
      <small>{server.transport==='stdio'?server.command:server.url}{server.hasCredential?' · with a saved token':''}</small>
      <div class="actions"><button disabled={tools.busy||!server.enabled} onclick={()=>toolAction('test',{id:server.id})}>Check it works</button><button disabled={tools.busy} onclick={()=>toolAction('cancel',{id:server.id})}>Disconnect</button><button disabled={tools.busy} onclick={()=>toggle(server)}>{server.enabled?'Turn off':'Turn on'}</button><button disabled={tools.busy} onclick={()=>toolAction('remove',{id:server.id})}>Remove</button></div>
      {#if server.error}<p class="error">{server.error}</p>{/if}
      {#if server.tools?.length}<details class="extra"><summary>{server.tools.length} things it can do</summary>{#each server.tools as tool (tool.id)}<p class="tool-detail"><strong>{tool.id}</strong> · {tool.description}</p>{/each}</details>{/if}
    </article>{:else}<div class="empty">No other apps connected.</div>{/each}
  </div>
</section>
<style>
  .tools-card{min-width:0;border-top:1px solid var(--line);padding-top:16px;margin-bottom:30px}
  .heading{display:flex;align-items:center;justify-content:space-between;gap:14px}
  h2{font-size:12.5px;font-weight:600;margin:0;letter-spacing:.01em}
  .error{color:var(--error-text);font-size:11.5px;margin:10px 0 0}
  button{padding:7px 11px;border-radius:7px;border:1px solid var(--line-strong);background:transparent;color:var(--muted);font-size:12px}
  button:hover:not(:disabled){color:var(--text);background:var(--soft)}
  .primary{background:var(--yellow);color:var(--black);border-color:var(--yellow);font-weight:600;justify-self:start}
  .primary:hover:not(:disabled){background:#ffdd7f;color:var(--black)}
  button:disabled{opacity:.45}
  form{display:grid;gap:12px;border-left:2px solid var(--yellow);padding:14px 16px;margin:16px 0;background:var(--paper)}
  label{display:grid;gap:6px;font-size:11.5px;color:var(--muted)}
  label span{color:var(--faint)}
  input,select{background:var(--bg);color:var(--text);border:1px solid var(--line-strong);border-radius:6px;padding:8px 10px;width:100%;font:inherit;font-size:12.5px}
  .trust{display:flex;gap:9px;align-items:flex-start;line-height:1.6;font-size:11.5px}
  .trust input{width:auto;margin-top:3px}
  .extra{border:0;padding:0;margin:0}
  .extra>summary{cursor:pointer;font-size:11.5px;color:var(--faint);padding:2px 0}
  .extra>summary:hover{color:var(--text)}
  .extra[open]>summary{margin-bottom:8px}
  .servers{margin-top:14px}
  .empty{font-size:11.5px;color:var(--faint);padding:14px 0;border-top:1px solid var(--line);line-height:1.6}
  article{border-top:1px solid var(--line);padding:14px 0}
  .server-title{display:flex;align-items:center;justify-content:space-between;font-size:13px;gap:12px}
  .server-title strong{font-weight:550}
  .status{font-size:11px;color:var(--faint)}
  .live{color:var(--yellow)}
  small{display:block;font-size:11px;color:var(--faint);margin-top:4px;overflow-wrap:anywhere}
  .actions{display:flex;flex-wrap:wrap;gap:6px;margin-top:11px}
  .actions button{font-size:11.5px;padding:5px 9px;color:var(--muted)}
  .tool-detail{overflow-wrap:anywhere;font-size:11px;color:var(--muted);margin:6px 0 0}
</style>
