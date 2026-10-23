import type { ToolDefinition } from '@earendil-works/pi-coding-agent';
import type { ManagedTool } from '../../../packages/contracts/src/tools.js';
import type { Authorize } from './mcp.js';


export class ToolRegistry {
  private entries = new Map<string, ManagedTool>();
  constructor(private authorize: Authorize) {}
  list() {return [...this.entries.values()];}
  wrap(tools: ToolDefinition[], allow?: (name: string) => boolean, authorize: Authorize = this.authorize):ToolDefinition[] {
    if(new Set(tools.map(t=>t.name)).size!==tools.length)throw new Error('Duplicate tool registration.');
    return tools.map(tool=>{
      const category = tool.name.startsWith('web_')?'Web Research':tool.name.startsWith('browser_')?'Browser':tool.name.startsWith('google_')?'Integrations':['mcp','tools','agent'].includes(tool.name)?'Integrations':'Files';
      const permissions = category==='Web Research'?['web.public']:category==='Browser'?['browser.session','browser.action']:category==='Files'?['files.approve']:tool.name==='agent'?['tool.use']:tool.name.startsWith('google_')?['google.read','google.write']:['mcp.call'];

      const timeoutMs = tool.name==='agent'?330_000:tool.name.startsWith('google_')?150_000:category==='Browser'?145_000:65_000;
      const maxResultBytes = tool.name==='agent'?32_768:tool.name.startsWith('google_')?16_384:category==='Browser'?98_304:16_384;
      const descriptor:ManagedTool = {id:tool.name,description:tool.description,category,permissions,availability:'available',timeoutMs,maxResultBytes,resources:[category==='Web Research'?'DonSeTch':category==='Browser'?'Chromium / Playwright / Stagehand':tool.name.startsWith('google_')?'Google Workspace':tool.name==='agent'?'Talo library':category==='Files'?'approved file':'MCP on demand']};
      const previous=this.entries.get(tool.name);
      if(previous && previous.category!==category)throw new Error(`Duplicate tool: ${tool.name}`);
      this.entries.set(tool.name,descriptor);
      return {...tool,execute:async(id,input,signal,update,context)=>{
        signal?.throwIfAborted();

        if(allow&&!allow(tool.name))throw new Error('This workspace does not include that capability. Ask the user to enable it in Workspaces.');
        if(!await authorize('tool.use',tool.name,tool.label,signal))throw new Error('Tool is disabled or permission was denied.');
        const deadline=AbortSignal.any([AbortSignal.timeout(descriptor.timeoutMs),...(signal?[signal]:[])]);
        const result=await tool.execute(id,input,deadline,update,context);
        if(category!=='Files')return result;
        let remaining=descriptor.maxResultBytes;
        return {...result,content:result.content.map(block=>{
          if(block.type!=='text')return {type:'text' as const,text:'Binary file output omitted. Open this file manually.'};
          const text=block.text.slice(0,remaining);remaining=Math.max(0,remaining-text.length);
          return {type:'text' as const,text};
        })};
      }};
    });
  }
}
