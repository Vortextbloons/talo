export type ServiceId = 'pi' | 'browser' | 'web';
export type ServiceState = 'stopped' | 'starting' | 'running' | 'failed';
export type SensitiveAction = 'provider.connect' | 'browser.start' | 'web.request' | 'memory.write' | 'extension.load' | 'mcp.connect' | 'task.execute' | 'autostart.change';
export interface PermissionRequest { id: string; action: SensitiveAction; resource: string; reason: string; }
export interface PermissionGate { request(request: PermissionRequest): Promise<boolean>; }
export interface Onboarding { name: string; personality: string; provider: string; model: string; memory: 'off' | 'workspace' | 'global'; complete: boolean; }
export interface RuntimeStatus { database: string; services: Record<ServiceId, ServiceState>; dependencies: Record<string, boolean>; }
export interface AgentRequest { id: string; method: 'ping' | 'session.open' | 'session.prompt' | 'session.abort' | 'shutdown'; params?: Record<string, unknown>; }
export type AgentEvent = { type: 'ready' | 'sleeping' | 'error'; message?: string } | { type: 'response'; id: string; result?: unknown; error?: string } | { type: 'stream'; sessionId: string; event: unknown };
export interface WebResearch { search(query: string): Promise<unknown>; fetch(url: string): Promise<unknown>; crawl(url: string): Promise<unknown>; screenshot(url: string): Promise<unknown>; }
export interface BrowserService { start(): Promise<void>; stop(): Promise<void>; isolatedContext(): Promise<never>; }
export interface MemoryRecord { id: string; scope: 'global' | 'workspace'; workspaceId: string | null; content: string; createdAt: string; }
export interface MemoryStore { search(query: string, workspaceId?: string): Promise<MemoryRecord[]>; put(record: MemoryRecord): Promise<void>; forget(id: string): Promise<void>; }
export interface ContextPolicy { maxTokens: number; memoryScope: 'off' | 'workspace' | 'global'; compactAtRatio: number; }
export interface ScheduledTask { id: string; title: string; dueAt: string; state: 'pending' | 'awaiting_permission' | 'done' | 'failed'; payload: unknown; }
export interface ToolDefinition { id: string; source: 'pi' | 'mcp' | 'web' | 'browser'; action: SensitiveAction; description: string; }
export interface ExtensionManifest { id: string; entrypoint: string; enabled: boolean; }
/** Credentials cross the boundary as opaque handles, never settings or event payloads. */
export interface ProviderConnection { providerId: string; modelId: string; credentialRef: string; }
export interface ProviderManager { listModels(providerId:string):Promise<{id:string;label:string}[]>; connect(connection:ProviderConnection,gate:PermissionGate):Promise<void>; disconnect(providerId:string):Promise<void>; }
export interface LazyMcpConnection { id: string; state: 'disconnected' | 'connecting' | 'connected'; connect(gate: PermissionGate): Promise<void>; disconnect(): Promise<void>; }
