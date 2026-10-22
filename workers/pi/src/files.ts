import path from 'node:path';
import { realpath, lstat } from 'node:fs/promises';
import { createReadToolDefinition, createWriteToolDefinition, createEditToolDefinition } from '@earendil-works/pi-coding-agent';
import type { ToolDefinition } from '@earendil-works/pi-coding-agent';
import type { Authorize } from './mcp.js';

const fold = (value:string) => process.platform === 'win32' ? value.toLowerCase() : value;

/** Whether `target` is a descendant of `root`. Both must already be absolute. */
export function insideDirectory(root:string,target:string):boolean {
  const relative = path.relative(fold(path.resolve(root)),fold(path.resolve(target)));
  return relative !== '' && !relative.startsWith('..') && !path.isAbsolute(relative);
}

export interface FileToolOptions {
  /** Approved skill directories: readable without a permission card, never writable. */
  readRoots?: string[];
  onSkillRead?: (file: string) => void;
}

export function fileTools(cwd:string, authorize:Authorize, options:FileToolOptions = {}):ToolDefinition[] {
  const readRoots = (options.readRoots ?? []).map(root => path.resolve(root));
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
      const approvedSkill = readRoots.some(root => insideDirectory(root,resolved));
      if(approvedSkill) {
        // Approved instructions are read-only here: changes go through the user's review card.
        if(tool.name!=='read')throw new Error('Read approved skills; propose changes through the skill review flow.');
      }
      else if(!await authorize(`files.${tool.name}`,resolved,`${tool.label}: ${resolved}`,signal))throw new Error('File access denied.');
      signal?.throwIfAborted();
      try {if((await lstat(resolved)).isSymbolicLink())throw new Error('File changed after approval.');}catch(e:any){if(e.code!=='ENOENT')throw e;}
      if(await realpath(path.dirname(resolved))!==path.dirname(resolved))throw new Error('File location changed after approval.');
      if(approvedSkill) options.onSkillRead?.(resolved);
      return (tool.execute as any)(id,{...input,path:resolved},signal,update,context);
    }} as ToolDefinition));
}
