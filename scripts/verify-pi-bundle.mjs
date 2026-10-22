import { spawn } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { createInterface } from 'node:readline';
import os from 'node:os';
import path from 'node:path';

// Exercise the shipped executable and dependency graph, rather than the developer's Node.
const data = await mkdtemp(path.join(os.tmpdir(), 'talo-bundle-check-'));
let child;
try {
  child = spawn(path.resolve('vendor/runtime/node/node.exe'),
    [path.resolve('vendor/bundle/workers/pi/src/main.js')], {
      cwd: path.resolve('vendor/bundle'),
      env: { ...process.env, NODE_OPTIONS: '', NODE_PATH: '', TALO_DATA_DIR: data, TALO_IDLE_MS: '120000' },
      windowsHide: true,
      stdio: ['pipe', 'pipe', 'pipe'],
    });
  await new Promise((resolve, reject) => {
    let ready = false;
    let verified = false;
    const timeout = setTimeout(() => reject(new Error('Bundled Pi health check timed out')), 30000);
    const finish = (error) => { clearTimeout(timeout); error ? reject(error) : resolve(); };
    // Diagnostics may contain provider information; do not echo subprocess output.
    child.stderr.resume();
    child.once('error', () => finish(new Error('Unable to launch bundled Node runtime')));
    child.stdin.on('error', () => finish(new Error('Bundled Pi protocol pipe closed')));
    createInterface({ input: child.stdout }).on('line', line => {
      let event;
      try { event = JSON.parse(line); } catch { return; }
      if (event.type === 'ready' && !ready) {
        ready = true;
        child.stdin.write(JSON.stringify({ id: 'bundle-health', method: 'ping' }) + '\n');
      }
      if (event.type === 'response' && event.id === 'bundle-health') {
        if (event.error || !event.result) {
          finish(new Error('Bundled Pi health check failed'));
          return;
        }
        verified = true;
        child.stdin.end(JSON.stringify({ id: 'bundle-shutdown', method: 'shutdown' }) + '\n');
      }
    });
    child.once('exit', code => finish(code === 0 && verified ? undefined : new Error('Bundled Pi exited before completing its health check')));
  });
  console.log('Bundled Node launches Pi and completes its IPC health check.');
} finally {
  if (child?.pid && child.exitCode === null && child.signalCode === null) {
    const exited = new Promise(resolve => child.once('exit', resolve));
    child.kill();
    await exited;
  }
  await rm(data, { recursive: true, force: true });
}
