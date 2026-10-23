

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
