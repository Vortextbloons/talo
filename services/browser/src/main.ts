import { chromium, type BrowserContext } from 'playwright';
import { createInterface } from 'node:readline';
import path from 'node:path';
import { mkdir } from 'node:fs/promises';
const root=process.env.TALO_VENDOR_DIR;
const data=process.env.TALO_DATA_DIR;
if(!root||!data)throw new Error('Host runtime paths required');
const profile=path.join(data,'browser-profile');await mkdir(profile,{recursive:true});
let context:BrowserContext|undefined;
let stopping=false;
async function stop(){if(stopping)return;stopping=true;await context?.close();process.exit(0);}
process.on('SIGTERM',()=>void stop());process.on('SIGINT',()=>void stop());
const input=createInterface({input:process.stdin});input.on('close',()=>void stop());
input.on('line',line=>{try{if(JSON.parse(line).method==='shutdown')void stop();}catch{/* Ignore malformed lifecycle messages. */}});
context=await chromium.launchPersistentContext(profile,{executablePath:path.join(root,'runtime/chromium/chrome-win64/chrome.exe'),headless:false,viewport:null,args:[
  // Keep an open browser from re-downloading components, phoning home or growing its cache forever.
  '--no-first-run','--no-default-browser-check','--disable-background-networking','--disable-component-update',
  '--disable-client-side-phishing-detection','--disable-default-apps','--disable-sync','--disable-domain-reliability',
  '--disable-breakpad','--metrics-recording-only','--disk-cache-size=134217728','--media-cache-size=67108864'
]});
context.on('close',()=>{if(!stopping)process.exit(0);});
process.stdout.write(JSON.stringify({type:'ready',profile})+'\n');
