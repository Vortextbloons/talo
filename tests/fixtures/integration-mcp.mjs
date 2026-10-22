import {createInterface} from 'node:readline';
import {spawn} from 'node:child_process';
import {writeFileSync} from 'node:fs';
if(process.argv[2]==='--spawn-child'){
  const child=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'ignore',windowsHide:true});
  writeFileSync(process.argv[3],String(child.pid));child.unref();
}
const reply=(id,result)=>process.stdout.write(JSON.stringify({jsonrpc:'2.0',id,result})+'\n');
const tools=[{name:'echo',description:'Echo text',inputSchema:{type:'object',properties:{text:{type:'string'},delay:{type:'integer',minimum:0,maximum:5000}},required:['text'],additionalProperties:false}},
{name:'fail',description:'Return a tool error',inputSchema:{type:'object',properties:{}}}];
const timers=new Map();
createInterface({input:process.stdin}).on('line',line=>{
  const r=JSON.parse(line);
  if(r.method==='initialize')reply(r.id,{protocolVersion:r.params.protocolVersion,capabilities:{tools:{}},serverInfo:{name:'talo-integration-fixture',version:'1.0.0'}});
  if(r.method==='tools/list')reply(r.id,{tools});
  if(r.method==='tools/call'){
    const timer=setTimeout(()=>{timers.delete(r.id);reply(r.id,{isError:r.params.name==='fail',content:[{type:'text',text:r.params.arguments.text??'Fixture failure'}]});},r.params.arguments.delay??0);
    timers.set(r.id,timer);
  }
  if(r.method==='notifications/cancelled'){clearTimeout(timers.get(r.params.requestId));timers.delete(r.params.requestId);}
});
process.stdin.on('end',()=>process.exit(0));
