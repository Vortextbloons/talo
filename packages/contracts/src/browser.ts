/** Session identifiers are independent of Pi sessions, transcripts and memory scopes.
 * V1 exposes primary only. Isolated sessions are reserved; no sub-agent execution exists. */
export type BrowserSessionId = string;
export type BrowserOwner = 'agent' | 'human' | 'paused';
export type BrowserControlAction = 'open' | 'watch' | 'take_over' | 'return' | 'pause' | 'stop';
export interface BrowserStatus {
  state: 'stopped' | 'starting' | 'connected' | 'crashed';
  owner: BrowserOwner;
  activeTab?: string;
  tabs: { id: string; revision: string; url: string; title: string }[];
  busy: boolean;
  error?: string;
}
export interface BrowserSessionDescriptor { id: BrowserSessionId; kind: 'primary' | 'isolated'; persistent: boolean }
