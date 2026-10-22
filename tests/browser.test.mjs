import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtemp, rm, access } from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
test('bundled Chromium starts with a dedicated profile and shuts down through host pipe',{timeout:30000},async()=>{
  const data=await mkdtemp(path.join(os.tmpdir(),'talo-browser-'));
  const child=spawn(path.resolve('vendor/runtime/node/node.exe'),[path.resolve('vendor/bundle/services/browser/src/main.js')],{env:{...process.env,TALO_DATA_DIR:data,TALO_VENDOR_DIR:path.resolve('vendor')},windowsHide:true});
  let output='';let errors='';child.stdout.on('data',d=>output+=d);child.stderr.on('data',d=>errors+=d);
  const exited=new Promise(resolve=>child.on('exit',code=>resolve(code)));
  try{
    const until=Date.now()+20000;while(!output.includes('"ready"')&&Date.now()<until&&child.exitCode===null)await new Promise(r=>setTimeout(r,50));
    assert.ok(output.includes('"ready"'),errors);await access(path.join(data,'browser-profile','Default'));
    child.stdin.write('{"id":"stop","method":"shutdown"}\n');assert.equal(await exited,0);
  }finally{child.kill();await exited;await rm(data,{recursive:true,force:true,maxRetries:20,retryDelay:100});}
});
