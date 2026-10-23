export type ToolCategory = 'Web Research' | 'Browser' | 'Files' | 'Memory' | 'Integrations';
export interface ManagedTool {
  id: string; category: ToolCategory; description: string; permissions: string[];
  availability: 'available' | 'disabled' | 'on-demand';
  timeoutMs: number; maxResultBytes: number; resources: string[];
}
export interface McpServerDefinition {
  id: string; transport: 'stdio' | 'http'; command?: string; args?: string[]; url?: string;
  enabled: boolean; trusted: boolean; idleMs?: number;

  credential?: string;
}
export interface ToolPermission {
  id: string; action: string; scope: string; summary: string;
  conversationId?: string; generationId?: string;
}
export interface ToolsState {
  servers: (Omit<McpServerDefinition, 'credential'> & { hasCredential?: boolean; state?: string; tools?: ManagedTool[]; error?: string })[];
  tools: ManagedTool[]; grants: ToolPermission[]; pending: ToolPermission[];
  errors: { server: string; message: string; at: string }[];
}
