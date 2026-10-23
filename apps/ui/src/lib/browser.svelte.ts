import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { BrowserStatus, BrowserFrame, BrowserInput, BrowserControlAction } from '../../../../packages/contracts/src/browser';

export interface BrowserApproval { id: string; tool: string; arguments: Record<string, unknown>; page?: { url?: string; title?: string } }
const stopped = (sessionId: string): BrowserStatus => ({sessionId, state: 'stopped', owner: 'agent', tabs: [], busy: false});
export const browser = $state({
  sessionId: 'primary', sessions: {} as Record<string, BrowserStatus>,
  status: stopped('primary'), frame: null as BrowserFrame | null,
  view: 'closed' as 'closed' | 'side' | 'large',
  busy: false, error: '', approvals: [] as BrowserApproval[],
});
let leaseId = crypto.randomUUID();
let heartbeat: ReturnType<typeof setInterval> | undefined;
const renewing = new Set<string>();
let initialized = false;
let workspaceVisible = false;
let hiddenPreview = false;
let approvalRevision = 0;
const viewIsOpen = () => browser.view !== 'closed';

function updateStatus(status: BrowserStatus) {
  if (status.sessionId && status.sessionId !== browser.sessionId) return;
  if (status.state === 'stopped' && browser.busy) return;
  browser.status = status;
  if (status.state === 'crashed' || status.state === 'stopped') {
    if (browser.view !== 'closed') closeBrowser(false);
    return;
  }
  if (workspaceVisible && !document.hidden && !hiddenPreview && status.state === 'connected' && browser.view === 'closed') void openBrowser('side');
}

export function selectBrowserSession(sessionId: string) {
  if (browser.sessionId === sessionId) return;
  closeBrowser(false);
  leaseId = crypto.randomUUID(); browser.sessionId = sessionId;
  browser.error = ''; browser.busy = false; hiddenPreview = false;
  updateStatus(browser.sessions[sessionId] ?? stopped(sessionId));
}
export function setBrowserWorkspaceVisible(visible: boolean) {
  workspaceVisible = visible; hiddenPreview = false;
  if (!visible) closeBrowser(false);
  else updateStatus(browser.sessions[browser.sessionId] ?? browser.status);
}
export function applyRuntimeStatus(value: {browser?: BrowserStatus; browserSessions?: Record<string, BrowserStatus>}) {
  browser.sessions = value.browserSessions ?? {};
  updateStatus(browser.sessions[browser.sessionId] ?? (browser.sessionId === 'primary' && value.browser ? value.browser : stopped(browser.sessionId)));
}
export function applyBrowserApprovals(approvals: BrowserApproval[]) {
  browser.approvals = approvals;
  approvalRevision += 1;
}
export function browserApprovalRevision(): number { return approvalRevision; }
export function initBrowser(initialStatus?: {browser?: BrowserStatus; browserSessions?: Record<string, BrowserStatus>} | null) {
  if (!isTauri() || initialized) return () => {};
  initialized = true;
  let active = true;
  const stops: (() => void)[] = [];
  const subscribe = async <T,>(name: string, handler: (value: T) => void) => {
    const stop = await listen<T>(name, event => { if (active) handler(event.payload); });
    if (active) stops.push(stop); else stop();
  };
  void Promise.all([
    subscribe('runtime-status', applyRuntimeStatus),
    subscribe<BrowserApproval[]>('browser-approvals', applyBrowserApprovals),
    subscribe<{sessionId: string; frame: BrowserFrame | null}>('browser-frame', value => {
      if (value.sessionId === browser.sessionId && browser.view !== 'closed') browser.frame = value.frame;
    }),
  ]).catch(() => { if (active) browser.error = 'Browser connection unavailable. Reopen Talo to reconnect.'; });
  if (initialStatus) applyRuntimeStatus(initialStatus);
  else if (initialStatus === undefined) void invoke<Parameters<typeof applyRuntimeStatus>[0]>('runtime_status').then(value => { if (active) applyRuntimeStatus(value); }).catch(() => {});
  const visibility = () => { if (document.hidden) closeBrowser(false); else if (workspaceVisible) updateStatus(browser.status); };
  document.addEventListener('visibilitychange', visibility);
  return () => { active = false; initialized = false; closeBrowser(false); stops.forEach(stop => stop()); document.removeEventListener('visibilitychange', visibility); };
}
async function renewView() {
  const sessionId = browser.sessionId, lease = leaseId;
  if (renewing.has(lease) || browser.view === 'closed' || !isTauri()) return;
  renewing.add(lease);
  try {
    const status = await invoke<BrowserStatus>('browser_view', {enabled: true, leaseId: lease, sessionId});
    if (leaseId === lease && viewIsOpen()) browser.status = status;
  } finally { renewing.delete(lease); }
}
export async function openBrowser(view: 'side' | 'large' = 'large') {
  browser.view = view; browser.error = ''; hiddenPreview = false;
  if (!isTauri()) return;
  const lease = leaseId;
  browser.busy = true;
  try {
    await renewView();
    if (lease === leaseId && viewIsOpen() && !heartbeat) heartbeat = setInterval(() => {
      const currentLease = leaseId;
      void renewView().catch(() => {
        if (leaseId !== currentLease) return;
        browser.error = 'The browser view disconnected. Open it again to reconnect.'; closeBrowser();
      });
    }, 2500);
  } catch { if (lease === leaseId) { browser.error = 'The browser could not connect. Open it again to retry.'; closeBrowser(); } }
  finally { if (lease === leaseId) browser.busy = false; }
}
export function closeBrowser(suppressPreview = true) {
  const sessionId = browser.sessionId, lease = leaseId;
  browser.view = 'closed'; browser.frame = null;
  hiddenPreview = suppressPreview;
  clearInterval(heartbeat); heartbeat = undefined;
  if (isTauri()) void invoke('browser_view', {enabled: false, leaseId: lease, sessionId}).catch(() => {});
}
export async function controlBrowser(action: BrowserControlAction) {
  if (!isTauri()) { browser.error = 'Open the desktop app to use the browser.'; return; }
  const sessionId = browser.sessionId;
  browser.busy = true; browser.error = '';
  if (action === 'stop') closeBrowser();
  try { const status = await invoke<BrowserStatus>('browser_control', {action, sessionId}); if (browser.sessionId === sessionId) browser.status = status; }
  catch (e) { if (browser.sessionId === sessionId) browser.error = String(e); }
  finally { if (browser.sessionId === sessionId) browser.busy = false; }
}
export async function sendBrowserInput(input: Omit<BrowserInput, 'leaseId' | 'tabId'>) {
  if (browser.status.owner !== 'human' || browser.view === 'closed' || !browser.frame || !isTauri()) return;
  const sessionId = browser.sessionId;
  try { await invoke('browser_input', {input: {...input, sessionId, leaseId, tabId: browser.frame.tabId}}); }
  catch { if (browser.sessionId === sessionId) browser.error = 'The page changed or control was returned. Take over again to interact.'; }
}

export async function openBrowserSource(url: string) {
  const parsed = new URL(url);
  if (!['http:', 'https:'].includes(parsed.protocol) || parsed.username || parsed.password) throw new Error('Only public HTTP(S) sources can be opened.');
  if (!isTauri()) throw new Error('Open the desktop app to view this source.');
  const sessionId = browser.sessionId;
  await openBrowser('large');
  if (browser.sessionId !== sessionId || !viewIsOpen() || browser.error) throw new Error('The browser could not open. Try again.');
  await controlBrowser('take_over');
  if (browser.sessionId !== sessionId || browser.status.owner !== 'human' || browser.error) throw new Error('The browser could not take control. Try again.');

  await openBrowser('large');
  if (browser.sessionId !== sessionId || !browser.status.activeTab || browser.error) throw new Error('The browser page could not open. Try again.');

  await invoke('browser_input', {input: {type: 'navigate', url: parsed.href, sessionId, leaseId, tabId: browser.status.activeTab}});
}
export async function resolveBrowserApproval(id: string, allowed: boolean) {
  browser.busy = true;
  try { await invoke('resolve_permission', {id, allowed}); browser.approvals = browser.approvals.filter(item => item.id !== id); }
  catch (e) { browser.error = String(e); } finally { browser.busy = false; }
}
