import { invoke, isTauri } from '@tauri-apps/api/core';
import { check, type Update } from '@tauri-apps/plugin-updater';


type Phase = 'idle' | 'checking' | 'current' | 'available' | 'preparing' | 'downloading' | 'installing' | 'error';

export const updates = $state({
  phase: 'idle' as Phase,
  version: '',
  notes: '',
  received: 0,
  total: 0,
  error: '',
});


let pending: Update | null = null;

const busy = (): boolean =>
  updates.phase === 'checking' || updates.phase === 'preparing' || updates.phase === 'downloading' || updates.phase === 'installing';

function describe(error: unknown): string {
  const text = error instanceof Error ? error.message : String(error);
  if (/signature/iu.test(text)) return 'This update failed its signature check and was not installed.';
  if (/valid release json|\b(?:401|403|404)\b|not found|platform.*not.*found/iu.test(text)) return 'Update files are unavailable. Try again later or install the latest Talo release manually.';
  if (/network|connect|fetch|dns|timed? ?out|request/iu.test(text)) return 'Could not reach the update server. Check your connection and try again.';
  if (/permission|denied|access is denied/iu.test(text)) return 'Windows blocked the update. Close Talo from the tray and try again.';
  return 'The update could not be installed. You can try again.';
}

export async function checkForUpdates(options: { quiet?: boolean } = {}): Promise<void> {
  if (!isTauri()) {
    if (!options.quiet) {
      updates.phase = 'error';
      updates.error = 'Updates are handled by the desktop app.';
    }
    return;
  }
  if (busy()) return;
  if (options.quiet && updates.phase === 'error') return;
  updates.phase = 'checking';
  updates.error = '';
  try {
    const found = await check({ timeout: 20_000 });
    if (!found) {
      pending = null;
      updates.phase = 'current';
      updates.version = '';
      updates.notes = '';
      return;
    }
    pending = found;
    updates.version = found.version;
    updates.notes = found.body ?? '';
    updates.phase = 'available';
  } catch (error) {
    pending = null;
    updates.phase = 'error';
    updates.error = describe(error);
  }
}






export async function installUpdate(): Promise<void> {
  if (!pending || busy()) return;
  updates.phase = 'downloading';
  updates.error = '';
  updates.received = 0;
  updates.total = 0;
  try {
    await pending.download((event) => {
      switch (event.event) {
        case 'Started':
          updates.total = event.data.contentLength ?? 0;
          break;
        case 'Progress':
          updates.received += event.data.chunkLength;
          break;
        default:
          break;
      }
    });
    updates.phase = 'preparing';
    await invoke('prepare_update');
    updates.phase = 'installing';
    await pending.install();
  } catch (error) {
    updates.phase = 'error';
    updates.error = describe(error);
  }
}
