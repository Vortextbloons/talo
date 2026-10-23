// First-party worker code is one file per process. Packages stay external so Pi's dynamic
// imports, jiti extensions, Playwright, and native addons keep resolving from node_modules.
const result = await Bun.build({
  entrypoints: [
    'workers/pi/src/main.ts',
    'workers/pi/src/transcript.ts',
    'services/browser/src/main.ts',
  ],
  outdir: 'dist/entries',
  target: 'bun',
  packages: 'external',
  format: 'esm',
});
if (!result.success) {
  for (const log of result.logs) console.error(log);
  process.exit(1);
}
console.log(`Bundled ${result.outputs.length} worker entries.`);
