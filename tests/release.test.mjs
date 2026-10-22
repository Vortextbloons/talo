// Release gate: fails when installer inputs, licensing coverage or wired scripts are broken.
// Vendor-dependent checks skip cleanly when `npm run vendor` has not been run yet.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {existsSync} from 'node:fs';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '..');
const read = relative => readFile(path.join(root, relative), 'utf8');
const vendored = existsSync(path.join(root, 'vendor', 'runtime-manifest.json'));

test('installer config: NSIS target, brand and icons resolve', async () => {
  const config = JSON.parse(await read('apps/desktop/tauri.conf.json'));
  assert.equal(config.productName, 'Talo');
  assert.equal(config.bundle.active, true);
  assert.ok(config.bundle.targets.includes('nsis'), 'NSIS must remain a bundle target');
  for (const icon of config.bundle.icon) {
    assert.ok(existsSync(path.join(root, 'apps/desktop', icon)), `missing bundle icon ${icon}`);
  }
  // The tray icon is embedded at compile time from the same set.
  assert.ok(existsSync(path.join(root, 'apps/desktop/icons/icon.png')), 'tray icon source missing');
});

test('vendored runtime resources referenced by the installer exist', async () => {
  if (!vendored) return; // npm run vendor has not run on this checkout yet
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
  // Bun's MIT license must travel with the binary once vendored.
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