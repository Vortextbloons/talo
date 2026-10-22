// Points the updater at the repository that actually publishes releases.
//
// The checked-in endpoint intentionally carries an OWNER placeholder so nobody ships a build that
// silently checks someone else's repo. Release builds rewrite it from GITHUB_REPOSITORY; the
// change stays in the workspace and is never committed.
import { readFile, writeFile } from 'node:fs/promises';

const repository = process.env.GITHUB_REPOSITORY;
if (!repository || !repository.includes('/')) {
  console.error('GITHUB_REPOSITORY is not set; leaving the updater endpoint untouched.');
  process.exit(0);
}

const file = 'apps/desktop/tauri.conf.json';
const config = JSON.parse(await readFile(file, 'utf8'));
const updater = config.plugins?.updater;
if (!updater) throw new Error(`${file} has no plugins.updater section`);

const endpoint = `https://github.com/${repository}/releases/latest/download/latest.json`;
updater.endpoints = [endpoint];
await writeFile(file, JSON.stringify(config, null, 2) + '\n');
console.log(`Updater endpoint set to ${endpoint}`);
