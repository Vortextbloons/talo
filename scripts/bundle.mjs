import { mkdir, readFile, writeFile, cp, readdir, access } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
const destination=path.resolve('vendor/bundle');
await mkdir(destination,{recursive:true});
const dependencies={};
for(const file of ['workers/pi/package.json','services/browser/package.json'])Object.assign(dependencies,JSON.parse(await readFile(file,'utf8')).dependencies);
await writeFile(path.join(destination,'package.json'),JSON.stringify({name:'talo-runtime',version:'0.1.0',private:true,type:'module',dependencies},null,2));
// The root lock is authoritative. Copy its fully resolved production graph, including native optional modules.
const lock=JSON.parse(await readFile('package-lock.json','utf8'));
const bundledLock={name:'talo-runtime',version:'0.1.0',lockfileVersion:3,requires:true,packages:{'':{name:'talo-runtime',version:'0.1.0',dependencies}}};
for(const [key,value] of Object.entries(lock.packages))if(key.startsWith('node_modules/')&&!value.link&&!value.dev)bundledLock.packages[key]=value;
await writeFile(path.join(destination,'package-lock.json'),JSON.stringify(bundledLock,null,2));
execFileSync('cmd.exe',['/d','/s','/c','npm ci --omit=dev --ignore-scripts --no-audit --no-fund'],{cwd:destination,stdio:'inherit',windowsHide:true});
await cp('dist/workers',path.join(destination,'workers'),{recursive:true});
await cp('dist/services',path.join(destination,'services'),{recursive:true});
await cp('dist/packages',path.join(destination,'packages'),{recursive:true});
// Explicit, checksum-verifying installer; no downloads occur on the end user's machine.
execFileSync(process.execPath,[path.join(destination,'node_modules/donsetch/install.js')],{cwd:destination,stdio:'inherit',windowsHide:true});
const binary=path.join(destination,'node_modules/donsetch/binaries/donsetch.exe');
await access(binary);
const sourceUrl='https://github.com/dondai44423/donsetch/archive/refs/tags/v4.10.1.tar.gz';
const source=await fetch(sourceUrl);if(!source.ok)throw new Error('Unable to bundle DonSeTch corresponding source');
await writeFile(path.join(destination,'donsetch-v4.10.1-source.tar.gz'),Buffer.from(await source.arrayBuffer()));
// Include a machine-readable inventory and complete license files from shipped npm packages.
const notices=[];
async function walk(dir){for(const entry of await readdir(dir,{withFileTypes:true})){if(!entry.isDirectory()||entry.name==='.bin')continue;const full=path.join(dir,entry.name);if(entry.name.startsWith('@')){await walk(full);continue;}try{const p=JSON.parse(await readFile(path.join(full,'package.json'),'utf8'));const licenses=[];for(const file of await readdir(full))if(/^(license|copying|notice)/i.test(file)){try{licenses.push({file,text:await readFile(path.join(full,file),'utf8')});}catch{}}notices.push({name:p.name,version:p.version,license:p.license,repository:p.repository,licenses});}catch{}try{await walk(path.join(full,'node_modules'));}catch{}}}
await walk(path.join(destination,'node_modules'));
await writeFile(path.join(destination,'THIRD-PARTY-LICENSES.json'),JSON.stringify(notices,null,2));
await cp('docs/THIRD-PARTY-NOTICES.md',path.join(destination,'THIRD-PARTY-NOTICES.md'));
execFileSync(process.execPath,['scripts/licenses.mjs'],{stdio:'inherit',windowsHide:true});
console.log(`Prepared ${notices.length} dependency notices and runtime modules.`);
