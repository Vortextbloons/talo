import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { createInterface } from 'node:readline';
import { SessionManager } from '@earendil-works/pi-coding-agent';
function worker(data,idle='300000'){
  const child=spawn(path.resolve('vendor/runtime/node/node.exe'),[path.resolve('vendor/bundle/workers/pi/src/main.js')],{env:{...process.env,TALO_DATA_DIR:data,TALO_IDLE_MS:idle},windowsHide:true,stdio:['pipe','pipe','pipe']});
  const events=[];let stderr='';child.stderr.on('data',d=>stderr+=d);createInterface({input:child.stdout}).on('line',l=>{try{events.push(JSON.parse(l));}catch{}});
  const exited=new Promise(resolve=>child.on('exit',(code)=>resolve(code)));
  async function wait(predicate,timeout=20000){const end=Date.now()+timeout;while(Date.now()<end){const event=events.find(predicate);if(event)return event;if(child.exitCode!==null)throw new Error(`Worker exited ${child.exitCode}: ${stderr}`);await new Promise(r=>setTimeout(r,25));}throw new Error(`Worker timeout: ${stderr}`);}
  return {child,wait,exited,send:r=>child.stdin.write(JSON.stringify(r)+'\n')};
}
test('bundled Pi worker protocol, safe prompt boundary, shutdown, and idle sleep',{timeout:45000},async()=>{
  const data=await mkdtemp(path.join(os.tmpdir(),'talo-worker-'));
  const w=worker(data);
  try{
    await w.wait(e=>e.type==='ready');
    w.send({id:'ping',method:'ping'});assert.equal((await w.wait(e=>e.id==='ping')).result.sdk,'1.4.0');
    w.child.stdin.write('{broken\n');assert.equal((await w.wait(e=>e.type==='error')).message,'Invalid protocol request');
    w.send({id:'prompt',method:'session.prompt',params:{text:'hello'}});assert.match((await w.wait(e=>e.id==='prompt')).error,/NOT_IMPLEMENTED/);
    w.send({id:'open',method:'session.open'});const opened=await w.wait(e=>e.id==='open');assert.equal(opened.error,undefined,opened.error);assert.ok(opened.result.sessionId);
    w.send({id:'stop',method:'shutdown'});assert.equal(await w.exited,0);
    // Seed a real user message through the unmodified SDK to exercise its durable format.
    const manager=SessionManager.create(path.join(data,'agent-workspace'),path.join(data,'sessions'));
    manager.appendMessage({role:'user',content:'Persistence fixture',timestamp:Date.now()});
    const durableId=manager.getSessionId();
    const restored=worker(data);await restored.wait(e=>e.type==='ready');restored.send({id:'restore',method:'session.open'});
    const result=await restored.wait(e=>e.id==='restore');assert.equal(result.error,undefined,result.error);
    assert.equal(result.result.sessionId,durableId);restored.send({id:'end',method:'shutdown'});assert.equal(await restored.exited,0);
    const sleepy=worker(data,'300');await sleepy.wait(e=>e.type==='ready');await sleepy.wait(e=>e.type==='sleeping');assert.equal(await sleepy.exited,0);
  }finally{w.child.kill();await rm(data,{recursive:true,force:true});}
});
