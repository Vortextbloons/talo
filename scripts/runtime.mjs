

import fs from 'node:fs';
import path from 'node:path';

const CANDIDATES = ['runtime/bun/bun.exe', 'runtime/node/node.exe'];


export function runtimePath(root = path.resolve('vendor')) {
  for (const candidate of CANDIDATES) {
    const file = path.resolve(root, candidate);
    if (fs.existsSync(file)) return file;
  }
  throw new Error('Bundled runtime is missing; run bun run vendor');
}


export function runtimeArgs(file, script) {
  return file.includes('bun') ? ['--no-install', script] : [script];
}
