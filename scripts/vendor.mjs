import { mkdir, readFile, writeFile, access, cp, readdir, rm } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
if(process.platform!=='win32'||process.arch!=='x64')throw new Error('Packaging currently targets Windows x64 only');
const runtime=path.resolve('vendor/runtime');await mkdir(runtime,{recursive:true});
const bunVersion='1.4.3';
const base=`https://github.com/oven-sh/bun/releases/download/bun-v${bunVersion}`;
const name='bun-windows-x64.zip';
const exists=async p=>access(p).then(()=>true,()=>false);
async function download(url){const r=await fetch(url);if(!r.ok)throw new Error(`${r.status}: ${url}`);return Buffer.from(await r.arrayBuffer());}
if(!await exists(path.join(runtime,'bun/bun.exe'))){
  const sums=(await download(`${base}/SHASUMS256.txt`)).toString();
  const expected=sums.split('\n').map(line=>line.trim().split(/\s+/)).find(fields=>fields.at(-1)===name)?.at(0);if(!expected)throw new Error('Bun checksum missing');
  const zip=await download(`${base}/${name}`);
  if(createHash('sha256').update(zip).digest('hex')!==expected)throw new Error('Bun checksum mismatch');
  await mkdir('vendor/downloads',{recursive:true});await writeFile(`vendor/downloads/${name}`,zip);
  execFileSync('powershell.exe',['-NoProfile','-Command',`Expand-Archive -LiteralPath 'vendor/downloads/${name}' -DestinationPath 'vendor/downloads/bun' -Force`],{stdio:'inherit',windowsHide:true});
  await cp('vendor/downloads/bun/bun-windows-x64',path.join(runtime,'bun'),{recursive:true});
}
const browserInfo=JSON.parse(await readFile('node_modules/playwright-core/browsers.json','utf8')).browsers.find(b=>b.name==='chromium');
if(!await exists(path.join(runtime,'chromium/chrome-win64/chrome.exe'))){
  // Playwright owns the revision and download URLs; package-lock pins the installer.
  execFileSync(process.execPath,['node_modules/playwright/cli.js','install','chromium','--no-shell'],{stdio:'inherit',windowsHide:true,env:{...process.env,PLAYWRIGHT_BROWSERS_PATH:path.join(runtime,'playwright')}});
  await cp(path.join(runtime,'playwright',`chromium-${browserInfo.revision}`),path.join(runtime,'chromium'),{recursive:true});
}
await mkdir('vendor',{recursive:true});
// Playwright's download cache is only a staging copy; the shipped tree is runtime/chromium.
await rm(path.join(runtime,'playwright'),{recursive:true,force:true});
// Prune what a sandboxed headless session never loads: 219 unused locale packs and the
// Chrome installer/updater helpers. chrome.exe is launched directly and never updates itself.
const chrome=path.join(runtime,'chromium/chrome-win64');
if(await exists(chrome)){
  for(const entry of await readdir(path.join(chrome,'locales')))if(entry!=='en-US.pak')await rm(path.join(chrome,'locales',entry),{force:true});
  for(const name of ['setup.exe','chrome_pwa_launcher.exe','notification_helper.exe','elevated_tracing_service.exe','chrome_proxy.exe'])await rm(path.join(chrome,name),{force:true});
}
if(await exists(path.join(runtime,'node/node.exe')))console.log('Note: vendor/runtime/node is unused now; the app prefers Bun and the installer ships only runtime/bun.');
await writeFile('vendor/runtime-manifest.json',JSON.stringify({platform:'win32-x64',bun:bunVersion,playwright:'1.64.1',chromium:browserInfo.browserVersion,chromiumRevision:browserInfo.revision,donsetch:'4.10.1'},null,2)+'\n');
console.log('Bundled Bun and Chromium are ready.');
