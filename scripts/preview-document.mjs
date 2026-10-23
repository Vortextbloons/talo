
import { readFile, readdir } from 'node:fs/promises';
const assets=await readdir('apps/ui/dist/assets');
const css=await readFile(`apps/ui/dist/assets/${assets.find(f=>f.endsWith('.css'))}`,'utf8');
const js=await readFile(`apps/ui/dist/assets/${assets.find(f=>f.endsWith('.js'))}`,'utf8');
process.stdout.write(`<!doctype html><html lang="en"><head><meta charset="UTF-8"><title>Talo UI inspection</title><style>${css.replaceAll('100vh','760px')}</style></head><body><div id="app"></div><script type="module">${js.replaceAll('</script','<\\/script')}</script></body></html>`);
