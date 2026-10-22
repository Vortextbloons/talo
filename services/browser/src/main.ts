import { createInterface } from 'node:readline';
import { BrowserSessions } from './sessions.js';
const root = process.env.TALO_VENDOR_DIR;
const data = process.env.TALO_DATA_DIR;
if (!root || !data) throw new Error('Host runtime paths required');
const send = (value: unknown) => process.stdout.write(JSON.stringify(value) + '\n');
const sessions = new BrowserSessions(root, data,
  (sessionId, state) => send({ type: 'browser_state', sessionId, state }),
  (sessionId, frame) => send({ type: 'browser_frame', sessionId, frame }));
const requests = new Map<string, AbortController>();
let stopping = false;
async function stop() {
  if (stopping) return;
  stopping = true;
  for (const controller of requests.values()) controller.abort();
  await sessions.dispose(); process.exit(0);
}
await sessions.acquire();
const input = createInterface({ input: process.stdin });
input.on('close', () => void stop());
input.on('line', line => {
  if (line.length > 64 * 1024 || stopping) return;
  let value: any; try { value = JSON.parse(line); } catch { return; }
  if (value.method === 'shutdown') { void stop(); return; }
  if (value.method === 'cancel') { requests.get(String(value.params?.requestId))?.abort(); return; }
  const id = String(value.id ?? ''); if (!id || requests.has(id)) return;
  if (requests.size >= 32) { send({ id, error: { message: 'Browser is busy. Try again shortly.' } }); return; }
  const controller = new AbortController(); requests.set(id, controller);
  void (async () => {
    try {
      let result;
      const sessionId = String(value.params?.sessionId ?? 'primary');
      if (value.method === 'forget') { await sessions.forget(sessionId); send({id, result: true}); return; }
      const manager = sessions.get(sessionId);
      if (value.method === 'state') result = await manager.state();
      else if (value.method === 'view') { await sessions.viewing(sessionId, value.params?.enabled === true, String(value.params?.leaseId)); result = await manager.state(); }
      else if (value.method === 'input') { await manager.input(value.params); result = true; }
      else if (value.method === 'control') {
        const action = value.params?.action;
        if (action === 'stop') { await manager.stop(); result = await manager.state(); }
        else if (action === 'watch' || action === 'open') result = await manager.execute({ name: 'browser_handoff', arguments: { action: 'watch' } });
        else if (['take_over', 'return', 'pause'].includes(action)) result = await manager.handoff(action === 'return' ? 'agent' : action === 'pause' ? 'paused' : 'human');
        else throw new Error('Unknown browser control.');
      } else if (value.method === 'tools/call') result = await manager.execute(value.params, controller.signal);
      else throw new Error('Unknown browser request.');
      send({ id, result });
    } catch (error) { send({ id, error: { message: value.params?.name === 'browser_ai' ? 'Browser AI failed. Use a supported configured provider and retry.' : error instanceof Error ? error.message.slice(0, 500) : 'Browser operation failed.' } }); }
    finally { requests.delete(id); }
  })();
});
process.on('SIGTERM', () => void stop()); process.on('SIGINT', () => void stop());
process.on('uncaughtException', () => void stop()); process.on('unhandledRejection', () => void stop());
send({ type: 'ready' });
