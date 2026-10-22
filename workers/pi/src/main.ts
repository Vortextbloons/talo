import { createInterface } from 'node:readline';
import { mkdir } from 'node:fs/promises';
import path from 'node:path';
import { createAgentSession, DefaultResourceLoader, SessionManager, type AgentSession } from '@earendil-works/pi-coding-agent';
import { isProviderId, parseProviderPreference, providerDescriptor, type AgentRequest, type AgentEvent } from '../../../packages/contracts/src/index.js';
import { readPreference, registryModels, syncModelsJson } from './providers.js';

const data=process.env.TALO_DATA_DIR;
if(!data)throw new Error('TALO_DATA_DIR is required');
const dataDir: string=data;
const cwd=path.join(data,'agent-workspace');
const agentDir=path.join(data,'pi');
const sessionDir=path.join(data,'sessions');
await Promise.all([cwd,agentDir,sessionDir].map(p=>mkdir(p,{recursive:true})));
let session:AgentSession|undefined;
let activeProvider:string|undefined;
let active=0;
let lastActivity=Date.now();
const send=(event:AgentEvent)=>process.stdout.write(JSON.stringify(event)+'\n');
async function shutdown(){if(session){await session.abort();session.dispose();}process.exit(0);}
const idleMs=Number(process.env.TALO_IDLE_MS??300000);
const idle=setInterval(()=>{if(!active && Date.now()-lastActivity>=idleMs){send({type:'sleeping'});void shutdown();}},Math.min(1000,idleMs));
idle.unref();
const input=createInterface({input:process.stdin,crlfDelay:Infinity});
input.on('close',()=>void shutdown());
process.on('SIGTERM',()=>void shutdown());
process.on('SIGINT',()=>void shutdown());
async function handle(request:AgentRequest){
  lastActivity=Date.now();
  try{
    switch(request.method){
      case 'ping':send({type:'response',id:request.id,result:{sdk:'1.4.0',sessionId:session?.sessionId??null}});break;
      case 'shutdown':await shutdown();break;
      case 'session.open': {
        if(session)throw new Error('A session is already open');
        // Provider preferences come from the host's projection; params can override them for tests or a future bridge.
        const preference=parseProviderPreference({...await readPreference(dataDir),...request.params});
        // Pi reads models.json while composing its registry, so write it before creating the session.
        await syncModelsJson(agentDir,preference);
        // Explicit resource isolation: no ambient extensions, skills, MCP, or project instructions.
        const loader=new DefaultResourceLoader({cwd,agentDir,noExtensions:true,disabledBuiltinExtensions:['mcp'],noSkills:true,noPromptTemplates:true,noThemes:true,noContextFiles:true});
        await loader.reload();
        const manager=SessionManager.continueRecent(cwd,sessionDir);
        const result=await createAgentSession({cwd,agentDir,sessionManager:manager,resourceLoader:loader,noTools:'all',tools:[]});
        session=result.session;
        activeProvider=preference.provider;
        session.subscribe(event=>send({type:'stream',sessionId:session!.sessionId,event}));
        send({type:'response',id:request.id,result:{sessionId:session.sessionId,path:manager.getSessionFile(),provider:preference.provider}});break;
      }
      case 'models.list': {
        if(!session)throw new Error('Open a session before listing models');
        const requested=request.params?.provider;
        const preference=await readPreference(dataDir);
        const providerId=typeof requested==='string'?requested:(activeProvider??preference.provider);
        if(!isProviderId(providerId))throw new Error(`Unknown provider: ${providerId}`);
        const descriptor=providerDescriptor(providerId);
        // The bundled Pi catalog supplies endpoints, limits and reasoning metadata for native providers.
        send({type:'response',id:request.id,result:{provider:descriptor.id,name:descriptor.name,native:descriptor.native,models:registryModels(session,descriptor.id)}});break;
      }
      case 'session.prompt':
        // Host permission bridge and secure credentials are a future interface.
        throw new Error('NOT_IMPLEMENTED: provider permission and credential bridge required');
      case 'session.abort':await session?.abort();send({type:'response',id:request.id,result:true});break;
      default:throw new Error('Unknown method');
    }
  }catch(e){send({type:'response',id:request.id,error:e instanceof Error?e.message:String(e)});}
  finally{lastActivity=Date.now();}
}
let queue=Promise.resolve();
input.on('line',line=>{
  if(line.length>1024*1024){send({type:'error',message:'Request exceeds 1 MiB'});return;}
  let request:AgentRequest;
  try{request=JSON.parse(line);if(typeof request.id!=='string'||typeof request.method!=='string')throw new Error('Invalid request');}
  catch{send({type:'error',message:'Invalid protocol request'});return;}
  // Shutdown/abort must not wait behind a future streaming prompt.
  if(request.method==='shutdown'||request.method==='session.abort'){void handle(request);return;}
  active++;queue=queue.then(()=>handle(request)).finally(()=>{active--;});
});
send({type:'ready',message:'Pi worker ready; providers resolve through the bundled registry and tools stay disabled until the permission bridge is connected'});
