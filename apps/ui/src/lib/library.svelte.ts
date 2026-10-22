import { invoke, isTauri } from '@tauri-apps/api/core';
import { chat, openConversation } from './chat.svelte';
import type { ConversationInfo } from '../../../../packages/contracts/src/chat';
export interface Workspace {
  id: string; name: string; folder: string | null; archived: boolean;
  /** Bundle fields. Absent on payloads from older hosts. */
  instructions?: string; pinned?: string[]; memoryScope?: 'inherit' | 'workspace' | 'off';
  capabilities?: ('web' | 'browser' | 'files')[]; agentOwned?: boolean; draft?: boolean; createdAt?: string;
}
export interface Memory { id: string; scope: 'global' | 'workspace'; workspaceId: string | null; content: string; source: string; personal: boolean; approved: boolean; supersededBy: string | null; createdAt: string; updatedAt: string }
export interface HistoryHit { conversationId: string; title: string; text: string; archived: boolean }
interface LibraryState { workspaces: Workspace[]; activeWorkspace: string; memoryEnabled: boolean; conversations: ConversationInfo[] }
export const library = $state({ workspaces: [{id:'general',name:'General',folder:null,archived:false}] as Workspace[], activeWorkspace: 'general', memoryEnabled: true, error: '', busy: false });
export async function libraryCall<T>(operation: string, payload: Record<string, unknown> = {}): Promise<T> {
  if (!isTauri()) throw new Error('Open the desktop app to manage your library.');
  return invoke<T>('library_action', { operation, payload });
}
export async function refreshLibrary(): Promise<void> {
  if (!isTauri()) return;
  try { apply(await libraryCall<LibraryState>('state')); } catch (e) { library.error=String(e); }
}
function apply(state: LibraryState): void {
  library.workspaces=state.workspaces; library.activeWorkspace=state.activeWorkspace; library.memoryEnabled=state.memoryEnabled;
  chat.conversations=state.conversations;
  if(chat.active) {
    const info=state.conversations.find(c=>c.id===chat.active?.conversation.id);
    if(!info || info.workspaceId!==state.activeWorkspace || info.archived) chat.active=null;
    else chat.active.conversation=info;
  }
}
export async function mutateLibrary(operation: string, payload: Record<string, unknown> = {}): Promise<boolean> {
  library.busy=true; library.error='';
  try { apply(await libraryCall<LibraryState>(operation,payload)); return true; }
  catch(e) { library.error=String(e); return false; }
  finally { library.busy=false; }
}
export async function switchWorkspace(id: string): Promise<void> {
  if(await mutateLibrary('workspace_switch',{id})) {
    const first=chat.conversations.find(c=>c.workspaceId===id && !c.archived);
    if(first) await openConversation(first.id);
  }
}
