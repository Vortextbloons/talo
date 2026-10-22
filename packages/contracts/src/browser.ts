/** Session identifiers are independent of Pi sessions, transcripts and memory scopes.
 * Each chat owns its tabs and control state. Logins are shared through one persistent profile. */
export type BrowserSessionId = string;
export type BrowserOwner = 'agent' | 'human' | 'paused';
export type BrowserControlAction = 'open' | 'watch' | 'take_over' | 'return' | 'pause' | 'stop';
export interface BrowserStatus {
  sessionId?: BrowserSessionId;
  state: 'stopped' | 'starting' | 'connected' | 'crashed';
  owner: BrowserOwner;
  activeTab?: string;
  tabs: { id: string; revision: string; url: string; title: string }[];
  busy: boolean;
  error?: string;
}
/** Private UI stream. Never persisted, attached to chat, or supplied to a model. */
export interface BrowserFrame { tabId: string; data: string; width: number; height: number }
export interface BrowserInput {
  leaseId: string; tabId: string;
  type: 'mousePressed' | 'mouseReleased' | 'mouseMoved' | 'mouseWheel' | 'keyDown' | 'keyUp' | 'text' | 'navigate' | 'back' | 'forward' | 'reload';
  x?: number; y?: number; button?: 'left' | 'right' | 'middle'; buttons?: number; clickCount?: number;
  deltaX?: number; deltaY?: number; modifiers?: number;
  key?: string; code?: string; keyCode?: number; text?: string; url?: string;
}
