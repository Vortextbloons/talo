import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import fs from 'node:fs';
import path from 'node:path';
import { runtimeArgs, runtimePath } from '../../scripts/runtime.mjs';

function bundledRuntime() {
  try {
    return runtimePath();
  } catch {
    return process.execPath;
  }
}

/**
 * Integration tests launch the Bun bundle, which is what the installer runs. A previous
 * copy under `vendor/bundle` is ignored so an old package cannot hide a current failure.
 * `bun run verify:bundle` is what boots that packaged copy.
 */
export function workerLocation() {
  const bundled = path.resolve('dist/entries/workers/pi/src/main.js');
  if (!fs.existsSync(bundled)) {
    throw new Error('Build the Pi worker (bun run build:workers) before running its tests');
  }
  return { script: bundled, cwd: path.resolve('.'), runtime: bundledRuntime() };
}

/**
 * Minimal protocol harness for the shipped JSONL worker.
 * It intentionally never logs prompts, assistant text, or credentials.
 */
export function startWorker(data, options = {}) {
  const location = workerLocation();
  const child = spawn(location.runtime, runtimeArgs(location.runtime, location.script), {
    cwd: location.cwd,
    env: {
      ...process.env,
      NODE_OPTIONS: '',
      NODE_PATH: '',
      TALO_DATA_DIR: data,
      TALO_IDLE_MS: String(options.idleMs ?? 120_000),
      ...(options.env ?? {}),
    },
    windowsHide: true,
    stdio: ['pipe', 'pipe', 'pipe'],
  });
  const events = [];
  const waiters = new Set();
  let stderr = '';
  let output = '';
  child.stdout.setEncoding('utf8');
  child.stderr.on('data', chunk => { stderr += chunk.toString(); });
  child.stdout.on('data', chunk => {
    output += chunk.toString();
    let newline;
    while ((newline = output.indexOf('\n')) >= 0) {
      const line = output.slice(0, newline).replace(/\r$/u, '');
      output = output.slice(newline + 1);
      if (!line) continue;
      let event;
      try { event = JSON.parse(line); } catch { continue; }
      events.push(event);
      for (const waiter of [...waiters]) {
        if (waiter.predicate(event, events.length - 1)) {
          waiters.delete(waiter);
          clearTimeout(waiter.timer);
          waiter.resolve(event);
        }
      }
    }
  });
  const exited = new Promise(resolve => child.once('exit', code => resolve(code ?? -1)));
  child.once('error', error => { stderr += `\n${error.message}`; });

  function mark() {
    return events.length;
  }

  function waitFrom(from, predicate, timeoutMs = 30_000) {
    for (let index = from; index < events.length; index += 1) {
      if (predicate(events[index], index)) return Promise.resolve(events[index]);
    }
    return new Promise((resolve, reject) => {
      const waiter = {
        predicate,
        resolve,
        timer: setTimeout(() => {
          waiters.delete(waiter);
          reject(new Error(`Worker timeout: ${stderr.slice(-2000)}`));
        }, timeoutMs),
      };
      waiters.add(waiter);
      void exited.then(code => {
        if (!waiters.has(waiter)) return;
        waiters.delete(waiter);
        clearTimeout(waiter.timer);
        reject(new Error(`Worker exited ${code}: ${stderr.slice(-2000)}`));
      });
    });
  }

  function wait(predicate, timeoutMs) {
    return waitFrom(0, predicate, timeoutMs);
  }

  function waitReady(timeoutMs = 30_000) {
    return wait(event => event.type === 'ready', timeoutMs);
  }

  function send(request) {
    if (child.stdin.destroyed) throw new Error('Worker stdin is closed');
    child.stdin.write(`${JSON.stringify(request)}\n`);
  }

  async function request(id, method, params, timeoutMs = 30_000) {
    send({ id, method, ...(params === undefined ? {} : { params }) });
    const event = await wait(item => item.type === 'response' && item.id === id, timeoutMs);
    if (event.error) {
      const error = new Error(event.error);
      error.response = event;
      throw error;
    }
    return event.result;
  }

  function waitGeneration(conversationId, generationId, states = ['completed', 'failed', 'cancelled'], from = 0, timeoutMs = 30_000) {
    return waitFrom(from, event =>
      event.type === 'chat' &&
      event.event?.type === 'generation' &&
      event.event.conversationId === conversationId &&
      event.event.generationId === generationId &&
      states.includes(event.event.state), timeoutMs);
  }

  function generationEvents(conversationId, generationId) {
    return events
      .filter(event => event.type === 'chat' && event.event?.type === 'generation' &&
        event.event.conversationId === conversationId && event.event.generationId === generationId)
      .map(event => event.event);
  }

  async function close() {
    if (child.exitCode !== null) return child.exitCode;
    try { child.stdin.end(); } catch { /* already closed */ }
    return Promise.race([
      exited,
      new Promise(resolve => setTimeout(() => resolve(child.exitCode), 5000)),
    ]);
  }

  async function kill() {
    if (child.exitCode !== null) return;
    child.kill();
    await exited;
  }

  return {
    child,
    events,
    exited,
    stderr: () => stderr,
    mark,
    wait,
    waitFrom,
    waitReady,
    send,
    request,
    waitGeneration,
    generationEvents,
    close,
    kill,
  };
}
