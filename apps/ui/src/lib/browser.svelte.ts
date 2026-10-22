import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { BrowserStatus, BrowserFrame, BrowserInput, BrowserControlAction } from '../../../../packages/contracts/src/browser';

export interface BrowserApproval { id: string; tool: string; arguments: Record<string, unknown>; page?: { url?: string; title?: string } }
export const browser = $state({
  status: { state: 'stopped', owner: 'agent', tabs: [], busy: false } as BrowserStatus,
  frame: null as BrowserFrame | null,
  view: 'closed' as 'closed' | 'side' | 'large',
  busy: false, error: '', approvals: [] as BrowserApproval[],
});
const leaseId = crypto.randomUUID();
let heartbeat: ReturnType<typeof setInterval> | undefined;
let renewing = false;
let initialized = false;
const viewIsOpen = () => browser.view !== 'closed';

export function initBrowser() {
  if (!isTauri() || initialized) return () => {};
  initialized = true;
  let active = true;
  const stops: (() => void)[] = [];
  const subscribe = async <T,>(name: string, handler: (value: T) => void) => {
    const stop = await listen<T>(name, event => { if (active) handler(event.payload); });
    if (active) stops.push(stop); else stop();
  };
  void Promise.all([
    subscribe<{ browser?: BrowserStatus }>('runtime-status', value => { if (value.browser) browser.status = value.browser; }),
    subscribe<BrowserApproval[]>('browser-approvals', value => { browser.approvals = value; }),
    subscribe<BrowserFrame | null>('browser-frame', frame => { if (browser.view !== 'closed') browser.frame = frame; }),
  ]).catch(() => { if (active) browser.error = 'Browser connection unavailable. Reopen Talo to reconnect.'; });
  void invoke<{ browser?: BrowserStatus }>('runtime_status').then(value => { if (active && value.browser) browser.status = value.browser; }).catch(() => {});
  const hide = () => { if (document.hidden) closeBrowser(); };
  document.addEventListener('visibilitychange', hide);
  return () => { active = false; initialized = false; closeBrowser(); stops.forEach(stop => stop()); document.removeEventListener('visibilitychange', hide); };
}

async function renewView() {
  if (renewing || browser.view === 'closed' || !isTauri()) return;
  renewing = true;
  try {
    const status = await invoke<BrowserStatus>('browser_view', { enabled: true, leaseId });
    if (viewIsOpen()) browser.status = status;
  } finally { renewing = false; }
}
export async function openBrowser(view: 'side' | 'large' = 'large') {
  browser.view = view; browser.error = '';
  if (!isTauri()) return;
  browser.busy = true;
  try {
    await renewView();
    if (viewIsOpen() && !heartbeat) heartbeat = setInterval(() => {
      void renewView().catch(() => { browser.error = 'The browser view disconnected. Close and reopen it to reconnect.'; closeBrowser(); });
    }, 2500);
  } catch (e) { browser.error = String(e); } finally { browser.busy = false; }
}
export function closeBrowser() {
  browser.view = 'closed'; browser.frame = null;
  clearInterval(heartbeat); heartbeat = undefined;
  if (isTauri()) void invoke('browser_view', { enabled: false, leaseId }).catch(() => {});
}
export async function controlBrowser(action: BrowserControlAction) {
  if (!isTauri()) { browser.error = 'Open the desktop app to use the browser.'; return; }
  browser.busy = true; browser.error = '';
  if (action === 'stop') { closeBrowser(); }
  try { browser.status = await invoke<BrowserStatus>('browser_control', { action }); }
  catch (e) { browser.error = String(e); } finally { browser.busy = false; }
}
export async function sendBrowserInput(input: Omit<BrowserInput, 'leaseId' | 'tabId'>) {
  if (browser.status.owner !== 'human' || browser.view === 'closed' || !browser.frame || !isTauri()) return;
  try { await invoke('browser_input', { input: { ...input, leaseId, tabId: browser.frame.tabId } }); }
  catch { browser.error = 'The page changed or control was returned to Pi. Reopen the view or take over again.'; }
}
export async function resolveBrowserApproval(id: string, allowed: boolean) {
  browser.busy = true;
  try { await invoke('resolve_permission', { id, allowed }); browser.approvals = browser.approvals.filter(item => item.id !== id); }
  catch (e) { browser.error = String(e); } finally { browser.busy = false; }
}
