import type { PermissionGate, WebResearch } from '../../../packages/contracts/src/index.js';
/** Research is a separate DonSeTch MCP process, never the interactive profile. */
export class DonSetchResearch implements WebResearch {
  constructor(private gate:PermissionGate){}
  private async call(operation:string,input:string):Promise<never>{
    if(!await this.gate.request({id:crypto.randomUUID(),action:'web.request',resource:JSON.stringify({operation,input}),reason:`Web research: ${operation}`}))throw new Error('Permission denied');
    throw new Error('NOT_IMPLEMENTED: lazy DonSeTch MCP transport');
  }
  search(query:string){return this.call('search',query);}
  fetch(url:string){return this.call('fetch',url);}
  crawl(url:string){return this.call('crawl',url);}
  screenshot(url:string){return this.call('screenshot',url);}
}
