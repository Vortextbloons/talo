

export type BrowserSessionId = string;
export type BrowserOwner = 'agent' | 'human' | 'paused';
export type BrowserControlAction = 'open' | 'watch' | 'take_over' | 'return' | 'pause' | 'stop';
export interface BrowserApproval {
  id: string;
  tool: string;
  arguments: Record<string, unknown>;
  page?: { url?: string; title?: string };
  conversationId?: string;
  generationId?: string;
}
export interface BrowserStatus {
  sessionId?: BrowserSessionId;
  state: 'stopped' | 'starting' | 'connected' | 'crashed';
  owner: BrowserOwner;
  activeTab?: string;
  tabs: { id: string; revision: string; url: string; title: string }[];
  busy: boolean;
  error?: string;
}

export interface BrowserFrame { tabId: string; data: string; width: number; height: number }
export interface BrowserInput {
  leaseId: string; tabId: string;
  type: 'mousePressed' | 'mouseReleased' | 'mouseMoved' | 'mouseWheel' | 'keyDown' | 'keyUp' | 'text' | 'navigate' | 'back' | 'forward' | 'reload';
  x?: number; y?: number; button?: 'left' | 'right' | 'middle'; buttons?: number; clickCount?: number;
  deltaX?: number; deltaY?: number; modifiers?: number;
  key?: string; code?: string; keyCode?: number; text?: string; url?: string;
}
