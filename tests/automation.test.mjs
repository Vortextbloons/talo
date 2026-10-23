import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { startWorker } from './helpers/worker.mjs';
import { startMockProvider } from './helpers/mock-provider.mjs';

test('headless background work waits for exact approval, reports usage, hibernates and restores its session', {timeout:45000}, async()=>{
  const data=await mkdtemp(path.join(os.tmpdir(),'talo-automation-'));
  const provider=await startMockProvider({mode:'mcp-tool'});
  let worker=startWorker(data,{idleMs:2000});
  try{
    await worker.waitReady();
    await worker.request('configure','tools.configure',{servers:[{id:'fixture',transport:'stdio',command:process.execPath,args:[path.resolve('tests/fixtures/integration-mcp.mjs')],enabled:true,trusted:true}]});
    const selection={conversationId:'scheduled',provider:'custom',model:'mock-model',apiKey:provider.state.key,baseUrl:provider.url};
    const opened=await worker.request('open','session.open',{...selection,sessionPath:null});
    await worker.request('prompt','session.prompt',{conversationId:'scheduled',generationId:'run-1',text:'Use the fixture echo tool',background:true});
    const approval=await worker.wait(e=>e.type==='tools'&&e.message?.method==='permission');
    assert.equal(approval.message.params.action,'mcp.call');
    assert.equal(JSON.parse(approval.message.params.scope).tool,'echo');
    assert.equal(provider.state.requests.filter(r=>r.url.endsWith('/chat/completions')).length,1,'model cannot continue before permission');
    assert.equal(worker.generationEvents('scheduled','run-1').some(e=>e.state==='completed'),false);
    worker.send({type:'tools_response',message:{id:approval.message.id,result:{allowed:true}}});
    assert.equal((await worker.waitGeneration('scheduled','run-1')).event.state,'completed');
    const usage=await worker.wait(e=>e.type==='tools'&&e.message?.method==='automation.usage');
    assert.equal(usage.message.params.generationId,'run-1');
    assert.ok(Number.isFinite(usage.message.params.cost));
    assert.match(await readFile(opened.path,'utf8'),/Pi executed real MCP/);
    await worker.wait(e=>e.type==='sleeping');assert.equal(await worker.exited,0);
    provider.state.mode='success';
    worker=startWorker(data);
    await worker.waitReady();
    const restored=await worker.request('restore','session.open',{...selection,sessionPath:opened.path});
    assert.equal(restored.sessionId,opened.sessionId);
    assert.ok(restored.messageCount>0);
    const restoredHistory=await worker.request('restore-history','session.history',{conversationId:'scheduled'});
    assert.ok(restoredHistory.messages.some(m=>m.role==='assistant'));
    await worker.request('next','session.prompt',{conversationId:'scheduled',generationId:'run-2',text:'Continue from the saved result',background:true});
    assert.equal((await worker.waitGeneration('scheduled','run-2')).event.state,'completed');
    assert.ok(provider.state.requests.at(-1).body.messages.some(m=>m.role==='tool'&&m.content.includes('Pi executed real MCP')));
  }finally{await worker.kill();await provider.close();await rm(data,{recursive:true,force:true});}
});
