export * from './providers.js';
export * from './chat.js';
export * from './agent.js';
export * from './browser.js';
export * from './tools.js';
export type ServiceId = 'pi' | 'browser' | 'web';
export type ServiceState = 'stopped' | 'starting' | 'running' | 'failed';
export type SensitiveAction = 'provider.connect' | 'browser.start' | 'web.request' | 'memory.write' | 'extension.load' | 'mcp.connect' | 'task.execute' | 'autostart.change';
export interface Onboarding { name: string; /** User-chosen assistant identity, separate from the user name. */ agentName?: string; personality: string; provider: string; model: string; /** Custom OpenAI-compatible endpoint. Never a credential. */ baseUrl?: string; /** Declare reasoning support for the custom model so Pi exposes thinking levels. */ reasoning?: boolean; /** Show the model's thinking above each reply in Chat. */ showReasoning?: boolean; memory: 'off' | 'workspace' | 'global'; complete: boolean; /** Auto lets Talo pick models and thinking; Manual keeps the user's picks. */ routing?: 'auto' | 'manual'; /** Cost focus Talo routes with when routing is auto. */ performance?: 'economy' | 'recommended'; }
export interface RuntimeStatus { database: string; services: Record<ServiceId, ServiceState>; dependencies: Record<string, boolean>; browser?: import('./browser.js').BrowserStatus; browserSessions?: Record<string, import('./browser.js').BrowserStatus>; }
export interface MemoryRecord { id: string; scope: 'global' | 'workspace'; workspaceId: string | null; content: string; createdAt: string; }
export interface MemoryStore { search(query: string, workspaceId?: string): Promise<MemoryRecord[]>; put(record: MemoryRecord): Promise<void>; forget(id: string): Promise<void>; }
export interface ScheduledTask { id: string; title: string; dueAt: string; state: 'pending' | 'awaiting_permission' | 'done' | 'failed'; payload: unknown; }
export interface ToolDefinition { id: string; source: 'pi' | 'mcp' | 'web' | 'browser'; action: SensitiveAction; description: string; }
export interface ExtensionManifest { id: string; entrypoint: string; enabled: boolean; }
