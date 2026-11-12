const net=require('node:net'),{randomUUID}=require('node:crypto');
const s=net.connect('\\\\.\\pipe\\codex-ipc');let buf=Buffer.alloc(0),clientId='initializing-client';
function send(m){let d=Buffer.from(JSON.stringify(m)),h=Buffer.alloc(4);h.writeUInt32LE(d.length);s.write(Buffer.concat([h,d]));}
s.on('error',e=>console.log(e.message));
s.on('connect',()=>send({type:'request',requestId:randomUUID(),sourceClientId:clientId,version:0,method:'initialize',params:{clientType:'desktop-pet'}}));
s.on('data',d=>{buf=Buffer.concat([buf,d]);while(buf.length>=4){let n=buf.readUInt32LE();if(buf.length<n+4)return;let m=JSON.parse(buf.subarray(4,n+4));buf=buf.subarray(n+4);if(m.method==='initialize'&&m.result?.clientId){clientId=m.result.clientId;console.log('connected');send({type:'broadcast',method:'thread-stream-following-changed',sourceClientId:clientId,version:1,params:{conversationId:process.env.CODEX_THREAD_ID,hostId:'local',following:true}});}
if(m.method==='thread-stream-state-changed'){let c=m.params.change;console.log('change',c.type);let state=c.conversationState;if(state)console.log(JSON.stringify(Object.fromEntries(Object.entries(state).filter(([k,v])=>!/(turns|messages|input|prompt|history|content|items)/i.test(k)).map(([k,v])=>[k,typeof v==='object'?JSON.stringify(v).slice(0,1000):v]))));if(c.patches)console.log(JSON.stringify(c.patches.map(p=>({...p,value:typeof p.value==='object'?'[object]':p.value}))).slice(0,1500));}
}});
setTimeout(()=>{if(clientId!=='initializing-client')send({type:'broadcast',method:'thread-stream-following-changed',sourceClientId:clientId,version:1,params:{conversationId:process.env.CODEX_THREAD_ID,hostId:'local',following:false}});s.end();},20000);
