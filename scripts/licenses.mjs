import { execFileSync } from 'node:child_process';
import { readFile, readdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
const metadata=JSON.parse(execFileSync('cargo',['metadata','--locked','--format-version','1','--manifest-path','apps/desktop/Cargo.toml'],{encoding:'utf8',maxBuffer:32*1024*1024,windowsHide:true}));


const texts=[];const ids=new Map();
const intern=text=>{let id=ids.get(text);if(id===undefined){id=texts.length;texts.push(text);ids.set(text,id);}return id;};
const notices=[];
for(const p of metadata.packages){
  if(!p.source)continue;
  const dir=path.dirname(p.manifest_path);const files=[];
  for(const name of await readdir(dir))if(/^(license|copying|notice)/i.test(name)){try{files.push({file:name,text:intern(await readFile(path.join(dir,name),'utf8'))});}catch{}}
  notices.push({name:p.name,version:p.version,license:p.license,repository:p.repository,source:p.source,files});
}
await writeFile('vendor/bundle/RUST-THIRD-PARTY-LICENSES.json',JSON.stringify({texts,files:notices},null,2));
console.log(`Collected ${notices.length} Rust dependency notices (${texts.length} unique license texts).`);
