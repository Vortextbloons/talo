<script lang="ts">
  import { onMount } from 'svelte';
  import { isTauri } from '@tauri-apps/api/core';
  import { tools,refreshTools,toolAction } from '../lib/tools.svelte';
  import type { McpServerDefinition } from '../../../../packages/contracts/src/tools';
  let adding=$state(false), id=$state(''), transport=$state<'stdio'|'http'>('stdio');
  let command=$state(''), args=$state('[]'), url=$state(''), credential=$state(''), trusted=$state(false);
  const categories=['Web Research','Browser','Files','Memory','Integrations'];
  onMount(()=>{void refreshTools();});
  async function add(){
    try{
      const server:McpServerDefinition={id,transport,enabled:true,trusted,...(transport==='stdio'?{command,args:JSON.parse(args)}:{url,credential})};
      if(await toolAction('save',{server})){adding=false;id='';command='';args='[]';url='';trusted=false;}
    }catch{tools.error='Arguments must be a JSON array of strings.';}finally{credential='';}
  }
  async function toggle(server:typeof tools.state.servers[number]){
    const {hasCredential,state,tools:catalog,error,...definition}=server;
    await toolAction('save',{server:{...definition,enabled:!server.enabled}});
  }
</script>
<section class="tools-card" aria-labelledby="tools-heading">
  <div class="heading"><div><span class="eyebrow">Capabilities / on your terms</span><h2 id="tools-heading">Tools &amp; integrations</h2></div><button disabled={!isTauri()||tools.busy} onclick={()=>adding=!adding}>{adding?'Close':'Add server'}</button></div>
  <p>Public research is ready to use. Files and integration actions ask first. Shell execution stays off.</p>
  {#if tools.error}<p class="error" role="alert">{tools.error}</p>{/if}
  {#if adding}<form onsubmit={event=>{event.preventDefault();void add();}}>
    <label>Server ID<input required pattern={'[a-z][a-z0-9-]{0,47}'} bind:value={id} placeholder="my-integration" maxlength="48"/></label>
    <label>Connection<select bind:value={transport}><option value="stdio">Local process (stdio)</option><option value="http">Remote HTTP</option></select></label>
    {#if transport==='stdio'}<label>Executable<input required bind:value={command} placeholder="C:\Tools\server.exe"/></label><label>Arguments (JSON array)<textarea bind:value={args} rows="2" spellcheck="false"></textarea></label>
    {:else}<label>Endpoint<input required type="url" bind:value={url} placeholder="https://example.com/mcp"/></label><label>Bearer token (optional)<input type="password" bind:value={credential} autocomplete="off"/></label>{/if}
    <label class="trust"><input type="checkbox" bind:checked={trusted} required/><span>I trust this server and its source. A local server runs code with my Windows account's access.</span></label>
    <button class="primary" disabled={tools.busy||!trusted}>Trust and add server</button>
  </form>{/if}
  <div class="servers">
    {#each tools.state.servers as server (server.id)}<article>
      <div class="server-title"><strong>{server.id}</strong><span class:live={server.state==='connected'} class="status">{server.enabled?server.state??'disconnected':'disabled'}</span></div>
      <small>{server.transport==='stdio'?server.command:server.url}{server.hasCredential?' · Protected credential':''}</small>
      <div class="actions"><button disabled={tools.busy||!server.enabled} onclick={()=>toolAction('test',{id:server.id})}>Test connection</button><button disabled={tools.busy} onclick={()=>toolAction('cancel',{id:server.id})}>Disconnect / cancel</button><button disabled={tools.busy} onclick={()=>toggle(server)}>{server.enabled?'Disable':'Enable'}</button><button disabled={tools.busy} onclick={()=>toolAction('remove',{id:server.id})}>Remove</button></div>
      {#if server.error}<p class="error">{server.error}</p>{/if}
      {#if server.tools?.length}<details><summary>{server.tools.length} tools available</summary>{#each server.tools as tool (tool.id)}<p class="tool-detail"><code>{tool.id}</code> · {tool.description}</p>{/each}</details>{/if}
    </article>{:else}<div class="empty">No MCP servers yet. Add one you trust; it starts when needed and disconnects after two idle minutes.</div>{/each}
  </div>
  <details><summary>Installed tools <span>{tools.state.tools.length}</span></summary>
    {#each categories as category (category)}<h3>{category}</h3>{#each tools.state.tools.filter(t=>t.category===category) as tool (tool.id)}<div class="tool"><div><code>{tool.id}</code><small>{tool.resources.join(' ? ')}</small><small>{tool.permissions.join(', ')} · {tool.timeoutMs/1000}s · {Math.round(tool.maxResultBytes/1024)} KiB</small></div>{#if category==='Memory'}<span class="muted">Managed in Memory</span>{:else}<button disabled={tools.busy||!isTauri()} onclick={()=>toolAction('toggle',{id:tool.id,enabled:tool.availability==='disabled'})}>{tool.availability==='disabled'?'Enable':'Disable'}</button>{/if}</div>{/each}{/each}
  </details>
  <details><summary>Remembered permissions <span>{tools.state.grants.length}</span></summary>{#each tools.state.grants as grant (grant.id)}<div class="grant"><div><strong>{grant.summary}</strong><pre>{grant.scope}</pre></div><button disabled={tools.busy} onclick={()=>toolAction('revoke',{id:grant.id})}>Revoke</button></div>{:else}<p class="muted">No remembered permissions. Allow once never creates a grant.</p>{/each}</details>
  <details><summary>Execution errors <span>{tools.state.errors.length}</span></summary>{#each tools.state.errors as error, i (`${error.at}-${i}`)}<p class="tool-detail"><strong>{error.server}</strong> · {new Date(error.at).toLocaleTimeString()}<br/>{error.message}</p>{:else}<p class="muted">No errors in this runtime.</p>{/each}</details>
</section>
<style>
  .tools-card{border:1px solid var(--line);border-radius:14px;background:var(--paper);padding:23px;min-width:0}.heading{display:flex;align-items:center;justify-content:space-between;gap:14px}.eyebrow{font-size:9px;letter-spacing:.13em;text-transform:uppercase;color:var(--yellow)}h2{font-size:20px;font-weight:550;letter-spacing:-.5px;margin:7px 0}p,.empty{font-size:12px;line-height:1.7;color:var(--muted)}button{padding:8px 10px;border-radius:7px;border:1px solid var(--line-strong);background:var(--soft);color:var(--text);font-size:11px}.primary{background:var(--yellow);color:var(--black);border-color:var(--yellow)}button:disabled{opacity:.45}button:focus-visible,input:focus-visible,select:focus-visible,textarea:focus-visible,summary:focus-visible{outline:2px solid var(--yellow);outline-offset:3px}form{display:grid;gap:13px;border-left:2px solid var(--yellow);padding:15px;margin:18px 0;background:var(--bg)}label{display:grid;gap:6px;font-size:11px}input,select,textarea{background:var(--paper);color:var(--text);border:1px solid var(--line-strong);border-radius:5px;padding:9px;width:100%;font:inherit}.trust{display:flex;gap:9px;align-items:start;line-height:1.6}.trust input{width:auto;margin-top:3px}.servers{margin-top:20px}.empty{padding:18px 0;border-block:1px solid var(--line)}article{border-top:1px solid var(--line);padding:17px 0}.server-title{display:flex;align-items:center;justify-content:space-between;font-size:13px;gap:12px}.status{font-size:10px;color:var(--muted)}.live{color:var(--yellow)}small{display:block;font-size:10px;color:var(--muted);margin-top:6px;overflow-wrap:anywhere}.actions{display:flex;flex-wrap:wrap;gap:6px;margin-top:13px}details{border-top:1px solid var(--line);padding:15px 0 0;margin-top:15px;font-size:12px}summary{cursor:pointer}summary span{float:right;font-size:10px;color:var(--yellow)}h3{font-size:10px;letter-spacing:.07em;text-transform:uppercase;color:var(--muted);margin:22px 0 7px}.tool,.grant{display:flex;justify-content:space-between;align-items:center;gap:14px;padding:9px 0}.tool code{font-size:11px}.grant{align-items:start}.grant>div{min-width:0}.muted{color:var(--muted);font-size:11px}.error{color:var(--error-text)}pre{white-space:pre-wrap;overflow-wrap:anywhere;font-size:10px;max-height:160px;overflow:auto}.tool-detail{overflow-wrap:anywhere;font-size:11px}article details{border:0;padding-top:0}
</style>
