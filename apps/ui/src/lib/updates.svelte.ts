import { invoke, isTauri } from '@tauri-apps/api/core';
import { check, type Update } from '@tauri-apps/plugin-updater';

/** The signed manifest endpoint lives in tauri.conf.json; this store only drives the UI. */
type Phase = 'idle' | 'checking' | 'current' | 'available' | 'preparing' | 'downloading' | 'installing' | 'error';

export const updates = $state({
  phase: 'idle' as Phase,
  notes: '',
  received: 0,
  total: 0,
  error: '',
});

/** The plugin object is deliberately kept out of reactive state. */
let pending: Update | null = null;

const busy = (): boolean =>
  updates.phase === 'checking' || updates.phase === 'preparing' || updates.phase === 'downloading' || updates.phase === 'installing';

function describe(error: unknown): string {
  const text = error instanceof Error ? error.message : String(error);
  if (/network|connect|fetch|dns|timed? ?out|request/iu.test(text)) return 'Could not reach the update server. Check your connection and try again.';
  if (/signature/iu.test(text)) return 'This update failed its signature check and was not installed.';
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
      updates.notes = '';
      return;
    }
    pending = found;
    updates.notes = found.body ?? '';
    updates.phase = 'available';
  } catch (error) {
    pending = null;
    updates.phase = 'error';
    updates.error = describe(error);
  }
}

/**
 * Stops the bundled runtimes, downloads the signed installer and hands it to Windows.
 * On Windows the plugin then exits into the NSIS installer, which relaunches Talo; the
 * code after the install call only runs on platforms that need an explicit relaunch.
 */
export async function installUpdate(): Promise<void> {
  if (!pending || busy()) return;
  updates.phase = 'preparing';
  updates.error = '';
  updates.received = 0;
  updates.total = 0;
  try {
    // The worker, browser and research processes hold files the installer replaces.
    await invoke('prepare_update');
    updates.phase = 'downloading';
    await pending.downloadAndInstall((event) => {
      switch (event.event) {
        case 'Started':
          updates.total = event.data.contentLength ?? 0;
          break;
        case 'Progress':
          updates.received += event.data.chunkLength;
          break;
        case 'Finished':
          updates.phase = 'installing';
          break;
        default:
          break;
      }
    });
    updates.phase = 'installing';
  } catch (error) {
    updates.phase = 'error';
    updates.error = describe(error);
  }
}
