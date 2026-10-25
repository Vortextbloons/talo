

import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile, mkdtemp, mkdir, writeFile, rm} from 'node:fs/promises';
import {execFileSync} from 'node:child_process';
import {tmpdir} from 'node:os';
import {existsSync} from 'node:fs';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '..');
const read = relative => readFile(path.join(root, relative), 'utf8');
const vendored = existsSync(path.join(root, 'vendor', 'runtime-manifest.json'));

test('version script keeps the Cargo lockfile usable by locked release builds', async () => {
  const directory = await mkdtemp(path.join(tmpdir(), 'talo-version-'));
  try {
    await mkdir(path.join(directory, 'apps/ui'), {recursive: true});
    await mkdir(path.join(directory, 'apps/desktop/src'), {recursive: true});
    for (const file of ['package.json', 'apps/ui/package.json', 'apps/desktop/tauri.conf.json']) {
      await writeFile(path.join(directory, file), JSON.stringify({version: '0.4.0'}));
    }
    const manifest = path.join(directory, 'apps/desktop/Cargo.toml');
    await writeFile(manifest, '[package]\nname = "talo"\nversion = "0.4.0"\nedition = "2021"\n');
    await writeFile(path.join(directory, 'apps/desktop/src/main.rs'), 'fn main() {}\n');
    execFileSync('cargo', ['generate-lockfile', '--offline', '--manifest-path', manifest]);
    execFileSync(process.execPath, [path.join(root, 'scripts/version.mjs'), '0.4.1'], {cwd: directory});
    const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--locked', '--offline', '--format-version', '1', '--manifest-path', manifest], {encoding: 'utf8'}));
    assert.equal(metadata.packages.find(pkg => pkg.name === 'talo').version, '0.4.1');
  } finally {
    await rm(directory, {recursive: true, force: true});
  }
});

test('installer config: NSIS target, brand and icons resolve', async () => {
  const config = JSON.parse(await read('apps/desktop/tauri.conf.json'));
  assert.equal(config.productName, 'Talo');
  assert.equal(config.bundle.active, true);
  assert.ok(config.bundle.targets.includes('nsis'), 'NSIS must remain a bundle target');
  for (const icon of config.bundle.icon) {
    assert.ok(existsSync(path.join(root, 'apps/desktop', icon)), `missing bundle icon ${icon}`);
  }

  assert.ok(existsSync(path.join(root, 'apps/desktop/icons/icon.png')), 'tray icon source missing');
});

test('vendored runtime resources referenced by the installer exist', async () => {
  if (!vendored) return;
  const config = JSON.parse(await read('apps/desktop/tauri.conf.json'));
  for (const resource of Object.keys(config.bundle.resources ?? {})) {
    const source = path.join(root, 'apps/desktop', resource.replace(/\/+$/, ''));
    assert.ok(existsSync(source), `bundle resource missing: ${resource}`);
  }
});

test('third-party notices cover every vendored runtime component', async () => {
  const notices = await read('docs/THIRD-PARTY-NOTICES.md');
  const required = ['Tauri', 'Svelte', 'Pi coding agent', 'DonSeTch', 'Playwright', 'Chromium', 'Bun', 'rusqlite', 'SQLite'];
  for (const component of required) {
    assert.ok(notices.includes(component), `THIRD-PARTY-NOTICES.md is missing ${component}`);
  }

  if (vendored) {
    assert.ok(
      existsSync(path.join(root, 'vendor/runtime/bun/LICENSE.md')) || existsSync(path.join(root, 'vendor/runtime/bun/LICENSE')),
      'bundled Bun ships without its LICENSE',
    );
  }
  assert.ok(existsSync(path.join(root, 'docs/DONSETCH-REDISTRIBUTION.md')), 'DonSeTch AGPL obligations doc missing');
});

test('package scripts reference scripts and docs that exist', async () => {
  const pkg = JSON.parse(await read('package.json'));
  const referenced = new Set();
  for (const command of Object.values(pkg.scripts ?? {})) {
    for (const match of String(command).matchAll(/scripts\/[\w.-]+\.mjs/g)) referenced.add(match[0]);
  }
  assert.ok(referenced.has('scripts/vendor.mjs'), 'vendor script unexpectedly unwired');
  for (const file of referenced) {
    assert.ok(existsSync(path.join(root, file)), `package.json references missing ${file}`);
  }
  const readme = await read('README.md');
  for (const match of readme.matchAll(/\]\((docs\/[^)#]+\.md)\)/g)) {
    assert.ok(existsSync(path.join(root, match[1])), `README links missing ${match[1]}`);
  }
});
