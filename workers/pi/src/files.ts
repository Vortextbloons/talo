import path from 'node:path';
import { realpath, lstat } from 'node:fs/promises';
import { createReadToolDefinition, createWriteToolDefinition, createEditToolDefinition } from '@earendil-works/pi-coding-agent';
import type { ToolDefinition } from '@earendil-works/pi-coding-agent';
import type { Authorize } from './mcp.js';

export function fileTools(cwd:string, authorize:Authorize):ToolDefinition[] {
  return [createReadToolDefinition(cwd),createWriteToolDefinition(cwd),createEditToolDefinition(cwd)].map(tool=>({...tool,
    async execute(id:string,input:any,signal:any,update:any,context:any) {
      if(typeof input.path!=='string'||input.path.length>4096)throw new Error('Invalid file path.');
      const requested=path.resolve(cwd,input.path);
      let resolved:string;
      try {resolved=await realpath(requested);} catch(error:any) {
        if(tool.name!=='write'||error.code!=='ENOENT')throw new Error('File is unavailable.');
        // Only an existing parent is accepted. Never create a tree through an unchecked link.
        resolved=path.join(await realpath(path.dirname(requested)),path.basename(requested));
      }
      if(!await authorize(`files.${tool.name}`,resolved,`${tool.label}: ${resolved}`,signal))throw new Error('File access denied.');
      signal?.throwIfAborted();
      try {if((await lstat(resolved)).isSymbolicLink())throw new Error('File changed after approval.');}catch(e:any){if(e.code!=='ENOENT')throw e;}
      if(await realpath(path.dirname(resolved))!==path.dirname(resolved))throw new Error('File location changed after approval.');
      return (tool.execute as any)(id,{...input,path:resolved},signal,update,context);
    }} as ToolDefinition));
}
