import { invoke, isTauri } from '@tauri-apps/api/core';
import type { ToolsState } from '../../../../packages/contracts/src/tools';
export const tools = $state<{state:ToolsState;error:string;busy:boolean}>({state:{servers:[],tools:[],grants:[],pending:[],errors:[]},error:'',busy:false});
export async function refreshTools(){if(isTauri())try{tools.state=await invoke<ToolsState>('tools_action',{operation:'state'});}catch(e){tools.error=String(e);}}
export async function toolAction(operation:string,payload:unknown){
  tools.busy=true;tools.error='';
  try{await invoke('tools_action',{operation,payload});await refreshTools();return true;}
  catch(e){tools.error=String(e);await refreshTools();return false;}finally{tools.busy=false;}
}
