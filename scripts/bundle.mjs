import { mkdir, readFile, writeFile, cp, readdir, access, rm } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { bundleDonsetch } from './donsetch.mjs';
const destination=path.resolve('vendor/bundle');
await mkdir(destination,{recursive:true});
const entries=[
  ['dist/entries/workers/pi/src/main.js','workers/pi/src/main.js'],
  ['dist/entries/workers/pi/src/transcript.js','workers/pi/src/transcript.js'],
  ['dist/entries/services/browser/src/main.js','services/browser/src/main.js'],
];
async function copyRuntimeCode(){
  for(const [from,to] of entries){
    const target=path.join(destination,to);
    await mkdir(path.dirname(target),{recursive:true});
    await cp(from,target);
  }
}
if(process.argv.includes('--code-only')){
  await copyRuntimeCode();
  process.exit(0);
}
const dependencies={};
for(const file of ['workers/pi/package.json','services/browser/package.json'])Object.assign(dependencies,JSON.parse(await readFile(file,'utf8')).dependencies);
await writeFile(path.join(destination,'package.json'),JSON.stringify({name:'talo-runtime',version:'0.1.0',private:true,type:'module',dependencies},null,2));
// The root lock is authoritative. Copy its fully resolved production graph, including native optional modules.
function parseBunLock(text){return JSON.parse(text.replace(/,(\s*[}\]])/g,'$1'));}
const lock=parseBunLock(await readFile('bun.lock','utf8'));
const packages={};
for(const [key,value] of Object.entries(lock.packages)){
  if(Array.isArray(value)&&String(value[0]).includes('workspace:'))continue;
  packages[key]=value;
}
const bundledLock={lockfileVersion:lock.lockfileVersion,configVersion:lock.configVersion,workspaces:{'':{name:'talo-runtime',dependencies}},packages};
await writeFile(path.join(destination,'bun.lock'),JSON.stringify(bundledLock,null,2));
await rm(path.join(destination,'package-lock.json'),{force:true});
const bunBinary=await access(path.resolve('vendor/runtime/bun/bun.exe')).then(()=>path.resolve('vendor/runtime/bun/bun.exe'),()=>'bun');
execFileSync(bunBinary,['install','--frozen-lockfile','--ignore-scripts','--production'],{cwd:destination,stdio:'inherit',windowsHide:true});
// Strip files the runtime never opens: source maps, type declarations and the Java engine
// that recheck only falls back to off Windows. Markdown stays: pi reads its own README.md
// and CHANGELOG.md at runtime. License and notice files are preserved for the inventory.
async function prune(dir){
  for(const entry of await readdir(dir,{withFileTypes:true})){
    const full=path.join(dir,entry.name);
    if(entry.isDirectory()){await prune(full);continue;}
    if(/^(license|copying|notice)/i.test(entry.name))continue;
    if(/\.(?:map|d\.ts|d\.mts|d\.cts)$/i.test(entry.name))await rm(full,{force:true});
  }
}
await prune(path.join(destination,'node_modules'));
await rm(path.join(destination,'node_modules/recheck-jar'),{recursive:true,force:true});
await copyRuntimeCode();
// Explicit, checksum-verifying installer; no downloads occur on the end user's machine.
await bundleDonsetch(destination);
const binary=path.join(destination,'node_modules/donsetch/binaries/donsetch.exe');
await access(binary);
// Include a machine-readable inventory and complete license files from shipped npm packages.
const notices=[];
async function walk(dir){for(const entry of await readdir(dir,{withFileTypes:true})){if(!entry.isDirectory()||entry.name==='.bin')continue;const full=path.join(dir,entry.name);if(entry.name.startsWith('@')){await walk(full);continue;}try{const p=JSON.parse(await readFile(path.join(full,'package.json'),'utf8'));const licenses=[];for(const file of await readdir(full))if(/^(license|copying|notice)/i.test(file)){try{licenses.push({file,text:await readFile(path.join(full,file),'utf8')});}catch{}}notices.push({name:p.name,version:p.version,license:p.license,repository:p.repository,licenses});}catch{}try{await walk(path.join(full,'node_modules'));}catch{}}}
await walk(path.join(destination,'node_modules'));
await writeFile(path.join(destination,'THIRD-PARTY-LICENSES.json'),JSON.stringify(notices,null,2));
await cp('docs/THIRD-PARTY-NOTICES.md',path.join(destination,'THIRD-PARTY-NOTICES.md'));
execFileSync(process.execPath,['scripts/licenses.mjs'],{stdio:'inherit',windowsHide:true});
console.log(`Prepared ${notices.length} dependency notices and runtime modules.`);
