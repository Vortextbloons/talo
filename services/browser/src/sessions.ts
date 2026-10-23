import { BrowserManager } from './manager.js';
import type { BrowserStatus, BrowserFrame } from '../../../packages/contracts/src/browser.js';


export class BrowserSessions {
  private readonly managers = new Map<string, BrowserManager>();
  private readonly runtime: BrowserManager;
  private viewer?: { sessionId: string; leaseId: string };
  private viewQueue: Promise<void> = Promise.resolve();
  constructor(private readonly root: string, private readonly data: string,
    private readonly state: (sessionId: string, state: BrowserStatus) => void,
    private readonly frame: (sessionId: string, frame: BrowserFrame | null) => void) {
    this.runtime = this.make('primary');
  }
  private make(id: string, runtime?: BrowserManager) {
    const manager = new BrowserManager(this.root, this.data, state => this.state(id, state), frame => this.frame(id, frame), id, runtime);
    this.managers.set(id, manager); return manager;
  }
  async acquire() { await this.runtime.acquire(); }
  get(id = 'primary') {
    if (!/^[a-zA-Z0-9-]{1,80}$/u.test(id)) throw new Error('Invalid browser session.');
    const existing = this.managers.get(id); if (existing) return existing;
    if (this.managers.size >= 128) throw new Error('Browser session limit reached. Restart Talo to release inactive sessions.');
    return this.make(id, this.runtime);
  }
  viewing(sessionId: string, enabled: boolean, leaseId: string) {
    const task = this.viewQueue.catch(() => {}).then(async () => {
      const manager = this.get(sessionId);
      if (enabled && this.viewer && (this.viewer.sessionId !== sessionId || this.viewer.leaseId !== leaseId)) {
        await this.get(this.viewer.sessionId).viewing(false, this.viewer.leaseId);
      }
      await manager.viewing(enabled, leaseId);
      if (enabled) this.viewer = { sessionId, leaseId };
      else if (this.viewer?.sessionId === sessionId && this.viewer.leaseId === leaseId) this.viewer = undefined;
    });
    this.viewQueue = task; return task;
  }
  async forget(sessionId: string) {
    if (sessionId === 'primary') return;
    const manager = this.managers.get(sessionId); if (!manager) return;
    if (this.viewer?.sessionId === sessionId) await this.viewing(sessionId, false, this.viewer.leaseId);
    await manager.dispose(); this.managers.delete(sessionId);
  }
  async dispose() {
    await this.viewQueue.catch(() => {});
    await Promise.all([...this.managers.values()].filter(manager => manager !== this.runtime).map(manager => manager.dispose()));
    await this.runtime.dispose(); this.managers.clear();
  }
}
