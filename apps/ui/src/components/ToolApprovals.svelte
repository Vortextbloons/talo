<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { tools, refreshTools, toolAction } from '../lib/tools.svelte';
  import type { GoogleApproval } from '../../../../packages/contracts/src/google';
  import type { StartupSnapshot } from '../../../../packages/contracts/src/index';
  let { initial = null, startupReady = false }: { initial?: StartupSnapshot['approvals'] | null; startupReady?: boolean } = $props();
  let pending=$derived(tools.state.pending[0]);
  let browser=$state<{id:string;tool:string;arguments:unknown}[]>([]);
  let browserError=$state('');
  let google=$state<GoogleApproval[]>([]);
  let googleError=$state('');
  let googleBusy=$state(false);
  let toolEvent=false;
  let toolStateDirty=false;
  let googleEvent=false;
  let browserEvent=false;
  async function resolveBrowser(allowed:boolean){
    const request=browser[0];if(!request)return;
    try{await invoke('resolve_permission',{id:request.id,allowed});browser=browser.filter(b=>b.id!==request.id);}catch(e){browserError=String(e);}
  }
  async function resolveGoogle(decision:'once'|'deny'){
    const request=google[0];if(!request||googleBusy)return;
    googleBusy=true;googleError='';
    try{await invoke('google_action',{operation:'resolve',payload:{id:request.id,decision}});google=google.filter(item=>item.id!==request.id);}
    catch(e){googleError=String(e);}
    finally{googleBusy=false;}
  }
  onMount(()=>{
    const stops:(()=>void)[]=[];let active=true;
    if(isTauri())for(const event of ['tools-changed','tool-approvals-changed'])void listen(event,()=>{toolEvent=true;toolStateDirty=true;if(startupReady)void refreshTools();}).then(stop=>active?stops.push(stop):stop());
    if(isTauri())void listen<typeof browser>('browser-approvals',event=>{browserEvent=true;browser=event.payload;}).then(stop=>active?stops.push(stop):stop());
    if(isTauri())void listen<GoogleApproval[]>('google-approvals',event=>{googleEvent=true;google=event.payload;}).then(stop=>active?stops.push(stop):stop());
    return()=>{active=false;stops.forEach(stop=>stop());};
  });
  $effect(()=>{
    if(!startupReady)return;
    const snapshot=initial;
    if(!toolEvent){if(snapshot&&!toolStateDirty)tools.state.pending=snapshot.tools;else if(isTauri())void refreshTools();}
    else if(toolStateDirty&&isTauri())void refreshTools();
    if(!googleEvent){if(snapshot)google=snapshot.google;else if(isTauri())void invoke<GoogleApproval[]>('google_action',{operation:'approvals'}).then(value=>{if(!googleEvent)google=value;}).catch(()=>{});}
    if(!browserEvent&&snapshot)browser=snapshot.browser;
  });
</script>
{#if pending}
  <div class="veil"><dialog open aria-modal="true" aria-labelledby="tool-approval-title">
    <span class="eyebrow">Your permission</span><h2 id="tool-approval-title">Allow this tool action?</h2>
    <p>{pending.summary}</p><pre>{pending.scope}</pre>
    <p class="hint">Allow for scope remembers this exact operation and its arguments. Revoke it in Tools &amp; integrations.</p>
    {#if tools.error}<p role="alert">{tools.error}</p>{/if}
    <div class="actions"><button disabled={tools.busy} onclick={()=>toolAction('resolve',{id:pending.id,decision:'deny'})}>Deny</button><button disabled={tools.busy} onclick={()=>toolAction('resolve',{id:pending.id,decision:'scope'})}>Allow for scope</button><button class="allow" disabled={tools.busy} onclick={()=>toolAction('resolve',{id:pending.id,decision:'once'})}>Allow once</button></div>
  </dialog></div>
{/if}
{#if !pending && google[0]}
  <div class="veil"><dialog open aria-modal="true" aria-labelledby="google-approval-title"><span class="eyebrow">Google Workspace</span><h2 id="google-approval-title">Allow this Google change?</h2><p>{google[0].summary}</p><pre class="exact">{JSON.stringify(google[0].arguments,null,2)}</pre><p class="hint">Scroll to read the whole action. This approval expires after 85 seconds and applies only to this exact action. Nothing is sent unless you allow it.</p>{#if googleError}<p role="alert">{googleError}</p>{/if}<div class="actions"><button disabled={googleBusy} onclick={()=>void resolveGoogle('deny')}>Deny</button><button class="allow" disabled={googleBusy} onclick={()=>void resolveGoogle('once')}>Allow once</button></div></dialog></div>
{/if}
{#if !pending && !google[0] && browser[0]}
  <div class="veil"><dialog open aria-modal="true" aria-labelledby="browser-approval-title"><span class="eyebrow">Scheduled browser action</span><h2 id="browser-approval-title">Allow this browser action?</h2><p>{browser[0].tool}</p><pre>{JSON.stringify(browser[0].arguments,null,2)}</pre><p class="hint">This approval expires after 90 seconds and applies to this action only.</p>{#if browserError}<p role="alert">{browserError}</p>{/if}<div class="actions"><button onclick={()=>resolveBrowser(false)}>Deny</button><button class="allow" onclick={()=>resolveBrowser(true)}>Allow once</button></div></dialog></div>
{/if}
<style>
  .veil{position:fixed;inset:0;z-index:50;background:var(--overlay);display:grid;place-items:center;padding:24px;backdrop-filter:blur(5px)}dialog{position:relative;margin:auto;background:var(--paper);color:var(--text);border:1px solid var(--line-strong);border-top:3px solid var(--yellow);border-radius:14px;padding:28px;width:620px;max-width:100%;max-height:85vh;overflow:auto}.eyebrow{color:var(--yellow);font-size:11px;text-transform:uppercase;letter-spacing:.12em}h2{font-size:23px;letter-spacing:-.6px;margin:12px 0}p{font-size:13px;line-height:1.6;overflow-wrap:anywhere}.hint{color:var(--muted);font-size:12px}pre{white-space:pre-wrap;overflow-wrap:anywhere;font-size:11px;background:var(--bg);padding:14px;max-height:240px;overflow:auto}pre.exact{max-height:min(52vh,520px);line-height:1.5}.actions{display:flex;flex-wrap:wrap;justify-content:flex-end;gap:8px;margin-top:24px}button{background:var(--soft);color:var(--text);border:1px solid var(--line-strong);border-radius:7px;padding:10px 13px;font-size:12px}.allow{background:var(--yellow);color:var(--black);border-color:var(--yellow)}button:focus-visible{outline:2px solid var(--yellow);outline-offset:3px}
</style>
