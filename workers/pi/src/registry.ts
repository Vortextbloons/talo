import type { PermissionGate, ToolDefinition } from '../../../packages/contracts/src/index.js';
/** Single dispatch boundary for future native, web, browser and MCP tools. */
export class ToolRegistry {
  private tools = new Map<string, {definition: ToolDefinition; execute: (input: unknown) => Promise<unknown>}>();
  constructor(private gate: PermissionGate) {}
  register(definition: ToolDefinition, execute: (input: unknown) => Promise<unknown>) {
    if(this.tools.has(definition.id)) throw new Error(`Duplicate tool: ${definition.id}`);
    this.tools.set(definition.id,{definition,execute});
  }
  list(){return [...this.tools.values()].map(t=>t.definition);}
  async execute(id:string,input:unknown){
    const tool=this.tools.get(id);if(!tool)throw new Error('Unknown tool');
    const allowed=await this.gate.request({id:crypto.randomUUID(),action:tool.definition.action,resource:JSON.stringify(input),reason:tool.definition.description});
    if(!allowed)throw new Error('Permission denied');
    return tool.execute(input);
  }
}
