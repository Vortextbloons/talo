// Sets the version everywhere it is written down, so a tag, the installer and the update
// manifest agree. Usage: node scripts/version.mjs 0.1.1
import { readFile, writeFile } from 'node:fs/promises';

const version = process.argv[2];
if (!version || !/^\d+\.\d+\.\d+$/u.test(version)) {
  console.error('Usage: node scripts/version.mjs <major.minor.patch>');
  process.exit(1);
}

const json = async (file, apply) => {
  const value = JSON.parse(await readFile(file, 'utf8'));
  apply(value);
  await writeFile(file, JSON.stringify(value, null, 2) + '\n');
};

await json('package.json', value => { value.version = version; });
await json('apps/ui/package.json', value => { value.version = version; });
await json('apps/desktop/tauri.conf.json', value => { value.version = version; });

// Cargo's version is a plain `version = "x.y.z"` line inside [package].
const manifest = 'apps/desktop/Cargo.toml';
const text = await readFile(manifest, 'utf8');
const next = text.replace(/^version = "[^"]*"$/mu, `version = "${version}"`);
if (next === text) throw new Error('Could not find the [package] version in Cargo.toml');
await writeFile(manifest, next);

console.log(`Version set to ${version}. Commit, then tag v${version}.`);
