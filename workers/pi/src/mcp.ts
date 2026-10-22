import type { PermissionGate } from '../../../packages/contracts/src/index.js';
/** Lazy host-managed adapter. No server is connected during worker startup. */
export async function loadMcpAdapter(gate:PermissionGate,server:string){
  if (/donsetch|\bweb_(search|fetch|crawl|screenshot)\b/iu.test(server)) throw new Error('Built-in web research is already registered by Talo.');
  if(!await gate.request({id:crypto.randomUUID(),action:'mcp.connect',resource:server,reason:'Connect an MCP server'}))throw new Error('Permission denied');
  // Loaded via Pi's extension loader (TS support), never evaluated as a native Node TS module.
  return {package:'pi-mcp-adapter',entrypoint:'pi-mcp-adapter/host-managed',server,state:'disconnected' as const,
    connect:async():Promise<never>=>{throw new Error('NOT_IMPLEMENTED: host-owned transport and per-call approval broker');}};
}
