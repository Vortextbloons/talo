import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {McpManager,validateServers} from '../dist/workers/pi/src/mcp.js';
import {mkdtemp,rm,readFile} from 'node:fs/promises';
import os from 'node:os';
import http from 'node:http';
import {startWorker} from './helpers/worker.mjs';
import {startMockProvider} from './helpers/mock-provider.mjs';
import {fileTools} from '../dist/workers/pi/src/files.js';
import {ToolRegistry} from '../dist/workers/pi/src/registry.js';
const definition={id:'fixture',transport:'stdio',command:process.execPath,args:[path.resolve('tests/fixtures/integration-mcp.mjs')],enabled:true,trusted:true,idleMs:1000};

test('Pi files require approval for canonical paths and registry rejects duplicates',async()=>{
  const data=await mkdtemp(path.join(os.tmpdir(),'talo-files-'));
  try{
    const scopes=[];let allowed=false;
    const gate=async(action,scope)=>{scopes.push({action,scope});return allowed;};
    const registry=new ToolRegistry(async()=>true);
    const native=fileTools(data,gate);const registered=registry.wrap(native);
    assert.throws(()=>registry.wrap([native[0],native[0]]),/Duplicate/);
    await assert.rejects(registered.find(t=>t.name==='write').execute('denied',{path:'file.txt',content:'private'},undefined),/denied/);
    allowed=true;
    await registered.find(t=>t.name==='write').execute('approved',{path:'file.txt',content:'approved'},undefined);
    assert.equal(await readFile(path.join(data,'file.txt'),'utf8'),'approved');
    assert.equal(scopes[0].action,'files.write');assert.ok(path.isAbsolute(scopes[0].scope));
    assert.deepEqual(registered.map(t=>t.name),['read','write','edit']);
  }finally{await rm(data,{recursive:true,force:true});}
});

test('real adapter discovery, schema validation, exact permissions, bounded results, idle reconnect and cancellation',async()=>{
  const approvals=[];let allowed=true;
  const m=new McpManager(async(action,scope)=>{approvals.push({action,scope});return allowed;});
  try{
    await m.configure([definition]);
    assert.equal(m.status().servers[0].state,'disconnected');
    const tools=await m.discover('fixture');
    assert.deepEqual(tools.tools.map(t=>t.name),['echo','fail']);
    assert.equal(tools.tools[0].inputSchema,undefined);
    assert.equal((await m.discover('fixture',undefined,'echo')).tools[0].inputSchema.type,'object');
    await assert.rejects(m.execute('fixture','echo',{text:7}),/schema/);
    assert.equal(approvals.length,0);
    let result=await m.execute('fixture','echo',{text:'Hello MCP'});
    assert.match(result.content[0].text,/Hello MCP/);
    assert.deepEqual(JSON.parse(approvals[0].scope),{server:'fixture',tool:'echo',arguments:{text:'Hello MCP'}});
    allowed=false;result=await m.execute('fixture','echo',{text:'must not run'});
    assert.equal(result.details.error,'approval_denied');
    allowed=true;result=await m.execute('fixture','echo',{text:'x'.repeat(20000)});
    assert.ok(result.content[0].text.length<12500);
    const controller=new AbortController();
    const running=m.execute('fixture','echo',{text:'cancelled',delay:4000},controller.signal);
    await delay(100);controller.abort();await running.catch(()=>{});
    await m.cancel('fixture');
    assert.equal(m.status().servers[0].state,'failed');
    await m.discover('fixture');
    const slow=m.execute('fixture','echo',{text:'active',delay:1800});
    await delay(1400);assert.equal(m.status().servers[0].state,'connected');
    await slow;await delay(1700);
    assert.equal(m.status().servers[0].state,'disconnected');
    assert.match((await m.execute('fixture','echo',{text:'reconnected'})).content[0].text,/reconnected/);
  }finally{await m.close();}
});
test('untrusted definitions, duplicates, built-in aliases and credential URLs fail closed',async()=>{
  assert.throws(()=>validateServers([definition,definition]),/unique/);
  assert.throws(()=>validateServers([definition,{...definition,id:'second'}]),/already configured/);
  assert.throws(()=>validateServers([{...definition,id:'donsetch'}]),/registered/);
  assert.throws(()=>validateServers([{...definition,command:'npx',args:['playwright-mcp']}]),/arguments/);
  assert.throws(()=>validateServers([{...definition,transport:'http',url:'https://secret@example.com/mcp'}]),/HTTPS/);
  const m=new McpManager(async()=>true);
  try{await m.configure([{...definition,trusted:false}]);await assert.rejects(m.discover('fixture'),/trust/);}finally{await m.close();}
});

test('Pi model executes real MCP through the single guarded proxy', {timeout:40000},async()=>{
  const data=await mkdtemp(path.join(os.tmpdir(),'talo-tools-'));
  const provider=await startMockProvider({mode:'mcp-tool'});
  const worker=startWorker(data);
  try{
    await worker.waitReady();
    await worker.request('configure','tools.configure',{servers:[definition]});
    const opened=await worker.request('open','session.open',{conversationId:'tools',sessionPath:null,provider:'custom',model:'mock-model',apiKey:provider.state.key,baseUrl:provider.url});
    await worker.request('prompt','session.prompt',{conversationId:'tools',generationId:'mcp-generation',text:'Use the fixture MCP echo tool'});
    const approval=await worker.wait(e=>e.type==='tools'&&e.message?.method==='permission');
    assert.equal(approval.message.params.action,'mcp.call');
    assert.deepEqual(JSON.parse(approval.message.params.scope),{server:'fixture',tool:'echo',arguments:{text:'Pi executed real MCP'}});
    worker.send({type:'tools_response',message:{id:approval.message.id,result:{allowed:true}}});
    const terminal=await worker.waitGeneration('tools','mcp-generation');assert.equal(terminal.event.state,'completed');
    const messages=provider.state.requests.filter(r=>r.url.endsWith('/chat/completions'));
    assert.ok(messages[1].body.messages.some(m=>m.role==='tool'&&m.content.includes('Pi executed real MCP')));
    const names=messages[0].body.tools.map(t=>t.function.name);
    assert.equal(new Set(names).size,names.length);assert.ok(names.includes('mcp'));assert.ok(!names.includes('fixture_echo'));assert.ok(!names.includes('bash'));
    assert.match(await readFile(opened.path,'utf8'),/Pi executed real MCP/);
    await worker.request('shutdown','shutdown');assert.equal(await worker.exited,0);
  }finally{await worker.kill();await provider.close();await rm(data,{recursive:true,force:true});}
});

test('remote HTTP MCP uses private Authorization header and never follows redirects',{timeout:20000},async()=>{
  const requests=[];const credential='talo-fixture-private-token';
  const server=http.createServer((req,res)=>{
    let body='';req.on('data',c=>body+=c);req.on('end',()=>{
      requests.push(req.headers.authorization);
      if(req.url==='/redirect'){res.writeHead(307,{location:'/stolen'});res.end();return;}
      if(req.method!=='POST'){res.writeHead(405);res.end();return;}
      const r=JSON.parse(body);
      if(r.id===undefined){res.writeHead(202);res.end();return;}
      let result;
      if(r.method==='initialize')result={protocolVersion:r.params.protocolVersion,serverInfo:{name:'http-fixture',version:'1'},capabilities:{tools:{}}};
      if(r.method==='tools/list')result={tools:[{name:'echo',description:'Echo',inputSchema:{type:'object',properties:{text:{type:'string'}},required:['text']}}]};
      if(r.method==='tools/call')result={content:[{type:'text',text:`${r.params.arguments.text} ${credential}`}]};
      res.writeHead(200,{'content-type':'application/json'});res.end(JSON.stringify({jsonrpc:'2.0',id:r.id,result}));
    });
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const url=`http://127.0.0.1:${server.address().port}`;
  const m=new McpManager(async()=>true);
  try{
    await m.configure([{id:'remote',transport:'http',url:`${url}/mcp`,credential,enabled:true,trusted:true}]);
    const result=await m.execute('remote','echo',{text:'Remote MCP works'});
    assert.match(result.content[0].text,/Remote MCP works/);assert.ok(!JSON.stringify(result).includes(credential));assert.ok(!JSON.stringify(m.status()).includes(credential));
    assert.ok(requests.every(a=>a===`Bearer ${credential}`));
    await m.configure([{id:'redirect',transport:'http',url:`${url}/redirect`,credential,enabled:true,trusted:true}]);
    await assert.rejects(m.discover('redirect'),/failed/);
  }finally{await m.close();await new Promise(resolve=>server.close(resolve));}
});
