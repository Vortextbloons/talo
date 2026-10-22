import test from 'node:test';
import assert from 'node:assert/strict';
import {Type} from 'typebox';
import {McpManager,validateServers} from '../dist/workers/pi/src/mcp.js';
import {ManagedToolsExtension} from '../dist/workers/pi/src/tools-extension.js';
import {ToolRegistry} from '../dist/workers/pi/src/registry.js';

function group(id,tools){return {id,tools,category:id==='talo-research'?'Web Research':'Browser',description:id,timeoutMs:145000,maxResultBytes:98304,resources:['existing service']};}
function tool(name,execute){return {name,label:name,description:name,parameters:Type.Object({text:Type.String()}),execute};}

test('managed Pi extension lazily exposes grouped MCP schemas and preserves native permission checks',async()=>{
  const calls=[],checks=[];let enabled=true;
  const gate=async(action,scope)=>{checks.push({action,scope});return action==='tool.use'&&enabled;};
  const registry=new ToolRegistry(gate), external=new McpManager(gate);
  const web=registry.wrap([tool('web_search',async(_id,args)=>{calls.push(args);return {content:[{type:'text',text:args.text}],details:{}};})]);
  const extension=new ManagedToolsExtension([group('talo-research',web)],external,gate);
  try{
    const registered=[];extension.factory({registerTool:t=>registered.push(t)});
    assert.deepEqual(registered.map(t=>t.name),['tools']);
    const list=await extension.tool.execute('list',{operation:'list'},undefined);
    assert.equal(JSON.parse(list.content[0].text)[0].state,'disconnected');assert.equal(calls.length,0);
    const discover=await extension.tool.execute('discover',{operation:'discover',server:'talo-research'},undefined);
    assert.equal(JSON.parse(discover.content[0].text).tools[0].inputSchema,undefined);
    const schema=await extension.tool.execute('schema',{operation:'discover',server:'talo-research',tool:'web_search'},undefined);
    assert.equal(JSON.parse(schema.content[0].text).tools[0].inputSchema.type,'object');
    const result=await extension.tool.execute('call',{operation:'call',server:'talo-research',tool:'web_search',arguments:{text:'source evidence'}},undefined);
    assert.match(result.content[0].text,/source evidence/);
    assert.equal(calls.length,1);assert.ok(checks.some(c=>c.scope==='web_search'));
    assert.ok(checks.every(c=>c.action==='tool.use'),'bundled research needs no redundant mcp.call grant');
    enabled=false;
    await assert.rejects(extension.tool.execute('disabled',{operation:'call',server:'talo-research',tool:'web_search',arguments:{text:'blocked'}},undefined),/disabled/);
    assert.equal(calls.length,1);
    assert.throws(()=>validateServers([{id:'talo-research',transport:'stdio',command:'node',args:[],enabled:true,trusted:true}]),/already registered/);
  }finally{await extension.close();await external.close();}
});

test('browser MCP calls remain session-local, retain approval broker, and propagate cancellation',async()=>{
  const external=new McpManager(async()=>true);
  const seen=[];
  const make=(session)=>new ManagedToolsExtension([group('talo-browser',[tool('browser_interact',async(_id,args,signal)=>{
    seen.push({session,text:args.text});
    if(args.text==='cancel')await new Promise((resolve,reject)=>signal.addEventListener('abort',()=>reject(new Error('cancelled')),{once:true}));
    if(args.text==='deny')throw new Error('Browser action denied by existing host approval.');
    return {content:[{type:'text',text:session}],details:{}};
  })])],external,async()=>true);
  const first=make('provider-A'),second=make('provider-B');
  const call=(extension,text,signal)=>extension.tool.execute('call',{operation:'call',server:'talo-browser',tool:'browser_interact',arguments:{text}},signal);
  try{
    const results=await Promise.all([call(first,'first'),call(second,'second')]);
    assert.match(results[0].content[0].text,/provider-A/);assert.match(results[1].content[0].text,/provider-B/);
    const denied=await call(first,'deny');assert.equal(denied.details.error,'tool_error');
    const controller=new AbortController();const pending=call(first,'cancel',controller.signal);
    while(!seen.some(s=>s.text==='cancel'))await new Promise(resolve=>setTimeout(resolve,5));
    controller.abort();const cancelled=await pending.catch(()=>null);
    assert.ok(cancelled===null||cancelled.details.error);
  }finally{await first.close();await second.close();await external.close();}
});
