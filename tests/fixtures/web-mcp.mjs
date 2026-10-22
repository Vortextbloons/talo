import { createInterface } from 'node:readline';
import { spawn } from 'node:child_process';
const reply = (id, result) => process.stdout.write(JSON.stringify({jsonrpc:'2.0',id,result})+'\n');
createInterface({input:process.stdin}).on('line', line => {
  const value = JSON.parse(line);
  if (value.method === 'initialize') reply(value.id, {protocolVersion:'2025-03-26',capabilities:{tools:{}}});
  if (value.method === 'tools/call') {
    if (value.params.name === 'spawn-child') {
      const child = spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'ignore',windowsHide:true});
      child.unref(); reply(value.id, {pid:child.pid}); return;
    }
    if (value.params.name === 'crash') process.exit(3);
    else if (value.params.name !== 'hang') reply(value.id, {content:[{type:'text',text:'Fixture evidence https://example.com/'}]});
  }
  if (value.method === 'notifications/cancelled') reply(value.params.requestId, {isError:true,content:[{type:'text',text:'Cancelled'}]});
});
