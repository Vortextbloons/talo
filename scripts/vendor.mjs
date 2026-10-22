import { mkdir, readFile, writeFile, access, cp } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
if(process.platform!=='win32'||process.arch!=='x64')throw new Error('Packaging currently targets Windows x64 only');
const runtime=path.resolve('vendor/runtime');await mkdir(runtime,{recursive:true});
const nodeVersion='24.22.1';
const base=`https://nodejs.org/dist/v${nodeVersion}`;
const name=`node-v${nodeVersion}-win-x64.zip`;
const exists=async p=>access(p).then(()=>true,()=>false);
async function download(url){const r=await fetch(url);if(!r.ok)throw new Error(`${r.status}: ${url}`);return Buffer.from(await r.arrayBuffer());}
if(!await exists(path.join(runtime,'node/node.exe'))){
  const sums=(await download(`${base}/SHASUMS256.txt`)).toString();
  const expected=sums.split('\n').find(l=>l.endsWith(`  ${name}`))?.split(/\s+/)[0];if(!expected)throw new Error('Node checksum missing');
  const zip=await download(`${base}/${name}`);
  if(createHash('sha256').update(zip).digest('hex')!==expected)throw new Error('Node checksum mismatch');
  await mkdir('vendor/downloads',{recursive:true});await writeFile(`vendor/downloads/${name}`,zip);
  execFileSync('powershell.exe',['-NoProfile','-Command',`Expand-Archive -LiteralPath 'vendor/downloads/${name}' -DestinationPath 'vendor/downloads/node' -Force`],{stdio:'inherit',windowsHide:true});
  await cp(`vendor/downloads/node/node-v${nodeVersion}-win-x64`,path.join(runtime,'node'),{recursive:true});
}
const browserInfo=JSON.parse(await readFile('node_modules/playwright-core/browsers.json','utf8')).browsers.find(b=>b.name==='chromium');
if(!await exists(path.join(runtime,'chromium/chrome-win64/chrome.exe'))){
  // Playwright owns the revision and download URLs; package-lock pins the installer.
  execFileSync(process.execPath,['node_modules/playwright/cli.js','install','chromium','--no-shell'],{stdio:'inherit',windowsHide:true,env:{...process.env,PLAYWRIGHT_BROWSERS_PATH:path.join(runtime,'playwright')}});
  await cp(path.join(runtime,'playwright',`chromium-${browserInfo.revision}`),path.join(runtime,'chromium'),{recursive:true});
}
await mkdir('vendor',{recursive:true});
await writeFile('vendor/runtime-manifest.json',JSON.stringify({platform:'win32-x64',node:nodeVersion,playwright:'1.64.1',chromium:browserInfo.browserVersion,chromiumRevision:browserInfo.revision,donsetch:'4.10.1'},null,2)+'\n');
console.log('Bundled Node and Chromium are ready.');
