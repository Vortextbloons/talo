// Locates the packaged JavaScript runtime used by Talo's stdio services. Bun ships in the
// installer; a vendored Node runtime is still accepted so older checkouts keep working.
import fs from 'node:fs';
import path from 'node:path';

const CANDIDATES = ['runtime/bun/bun.exe', 'runtime/node/node.exe'];

/** `root` is the vendor directory that carries `runtime/` and `bundle/`. */
export function runtimePath(root = path.resolve('vendor')) {
  for (const candidate of CANDIDATES) {
    const file = path.resolve(root, candidate);
    if (fs.existsSync(file)) return file;
  }
  throw new Error('Bundled runtime is missing; run bun run vendor');
}

/** Bun must never resolve a missing module by installing it inside a packaged app. */
export function runtimeArgs(file, script) {
  return file.includes('bun') ? ['--no-install', script] : [script];
}
