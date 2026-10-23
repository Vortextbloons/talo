import { spawn } from 'node:child_process';
import { mkdtemp, rm, readFile } from 'node:fs/promises';
import { createInterface } from 'node:readline';
import os from 'node:os';
import path from 'node:path';
import { release, sha256 } from './donsetch.mjs';
import { runtimeArgs, runtimePath } from './runtime.mjs';

for (const [name, expected] of Object.entries(release.files)) {
  if (sha256(await readFile(path.join('vendor/bundle/node_modules/donsetch/binaries', name))) !== expected) throw new Error(`Bundled DonSeTch checksum mismatch: ${name}`);
}
for (const model of release.models) {
  if (sha256(await readFile(path.join('vendor/bundle/research-models/rerank', model.name))) !== model.sha256) throw new Error(`Bundled model checksum mismatch: ${model.name}`);
}
if (sha256(await readFile('vendor/downloads/donsetch-source.tar.gz')) !== release.sourceSha256) throw new Error('Downloaded DonSeTch source checksum mismatch');
const sourceOffer = await readFile('vendor/bundle/DONSETCH-SOURCE.txt', 'utf8');
if (!sourceOffer.includes(release.sourceSha256) || !sourceOffer.includes(`donsetch-v${release.version}-source.tar.gz`)) throw new Error('Bundled DonSeTch source offer is incomplete');


const data = await mkdtemp(path.join(os.tmpdir(), 'talo-bundle-check-'));
let child;
try {
  const runtime = runtimePath();
  child = spawn(runtime,
    runtimeArgs(runtime, path.resolve('vendor/bundle/workers/pi/src/main.js')), {
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

    child.stderr.resume();
    child.once('error', () => finish(new Error('Unable to launch bundled runtime')));
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
        child.stdin.write(JSON.stringify({ id: 'bundle-session', method: 'session.open', params: { conversationId: 'bundle-check' } }) + '\n');
      }
      if (event.type === 'response' && event.id === 'bundle-session') {
        if (event.error) {
          finish(new Error('Bundled Pi could not open a session for tool discovery'));
          return;
        }
        child.stdin.write(JSON.stringify({ id: 'bundle-tools', method: 'tools.status' }) + '\n');
      }
      if (event.type === 'response' && event.id === 'bundle-tools') {
        const googleTools = event.result?.tools?.filter(tool => tool.id.startsWith('google_')) ?? [];
        if (event.error || googleTools.length !== 13) {
          finish(new Error('Bundled Pi is missing the Google Workspace tools; refresh the worker bundle'));
          return;
        }
        verified = true;
        child.stdin.end(JSON.stringify({ id: 'bundle-shutdown', method: 'shutdown' }) + '\n');
      }
    });
    child.once('exit', code => finish(code === 0 && verified ? undefined : new Error('Bundled Pi exited before completing its health check')));
  });
  console.log('Bundled runtime launches Pi, completes its IPC health check, and exposes Google Workspace tools.');
} finally {
  if (child?.pid && child.exitCode === null && child.signalCode === null) {
    const exited = new Promise(resolve => child.once('exit', resolve));
    child.kill();
    await exited;
  }
  await rm(data, { recursive: true, force: true });
}
