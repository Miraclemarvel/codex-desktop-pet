import {spawn} from 'node:child_process';
import {createInterface} from 'node:readline';
const bin=process.env.CODEX_BIN || 'codex.exe';
const p=spawn(bin,['app-server','proxy'],{windowsHide:true,stdio:['pipe','pipe','pipe']});
let next=0;const pending=new Map();
const request=(method,params={})=>new Promise((resolve,reject)=>{const id=++next;const t=setTimeout(()=>{pending.delete(id);reject(new Error(method+' timeout'));},12000);pending.set(id,m=>{clearTimeout(t);m.error?reject(new Error(JSON.stringify(m.error))):resolve(m.result)});p.stdin.write(JSON.stringify({id,method,params})+'\n')});
createInterface({input:p.stdout}).on('line',line=>{try{const m=JSON.parse(line);if(pending.has(m.id)){pending.get(m.id)(m);pending.delete(m.id)}}catch{}});
p.stderr.on('data',d=>process.stderr.write(d));
p.on('error',e=>console.error(e.message));
try{
 console.log('initialize',await request('initialize',{clientInfo:{name:'codex_desktop_pet',title:'Codex Desktop Pet',version:'0.1.0'},capabilities:{experimentalApi:true}}));
 p.stdin.write(JSON.stringify({method:'initialized',params:{}})+'\n');
 for(const method of ['thread/loaded/list','account/rateLimits/read']){
  try{console.log(method,JSON.stringify(await request(method)))}catch(e){console.error(e.message)}
 }
 try{const data=await request('thread/list',{limit:5,sortKey:'updated_at'});console.log('threads',JSON.stringify(data.data?.map(t=>({id:t.id,status:t.status,source:t.source}))))}catch(e){console.error(e.message)}
}catch(e){console.error(e.message);process.exitCode=1}finally{p.stdin.end();p.kill()}
