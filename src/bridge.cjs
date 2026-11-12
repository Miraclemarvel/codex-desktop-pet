const fs=require('node:fs');const net=require('node:net');const os=require('node:os');const path=require('node:path');
const {randomUUID}=require('node:crypto');const {EventEmitter}=require('node:events');
const {aggregate,classify,normalizeUsage,summarizeSnapshot,applyStatusPatches}=require('./model.cjs');
const PIPE_ROOT='\\\\.\\pipe\\';
class FramedSocket extends EventEmitter {
  constructor(pipe,maxBytes=64*1024*1024){super();this.buffer=Buffer.alloc(0);this.maxBytes=maxBytes;this.socket=net.connect(pipe);this.socket.on('connect',()=>this.emit('connect'));this.socket.on('error',e=>this.emit('fault',e));this.socket.on('close',()=>this.emit('close'));this.socket.on('data',d=>this.read(d));}
  read(d){this.buffer=Buffer.concat([this.buffer,d]);while(this.buffer.length>=4){const len=this.buffer.readUInt32LE();if(!len||len>this.maxBytes){this.emit('fault',Error('Unsupported desktop frame'));this.close();return;}if(this.buffer.length<len+4)return;try{const m=JSON.parse(this.buffer.subarray(4,len+4).toString('utf8'));this.buffer=this.buffer.subarray(len+4);this.emit('message',m);}catch{this.emit('fault',Error('Invalid desktop frame'));this.close();return;}}}
  send(m){if(!this.socket.writable)throw Error('Codex connection closed');const body=Buffer.from(JSON.stringify(m)),header=Buffer.alloc(4);header.writeUInt32LE(body.length);this.socket.write(Buffer.concat([header,body]));}
  close(){this.socket.destroy();this.buffer=Buffer.alloc(0);}
}
function recentThreadId(){
  try{const filename=path.join(process.env.CODEX_HOME||path.join(os.homedir(),'.codex'),'session_index.jsonl');const fd=fs.openSync(filename,'r');try{const size=fs.fstatSync(fd).size,b=Buffer.alloc(Math.min(size,65536));fs.readSync(fd,b,0,b.length,size-b.length);const lines=b.toString('utf8').trim().split('\n');for(let i=lines.length-1;i>=0;i--){try{const r=JSON.parse(lines[i]);if(r.id)return r.id;}catch{}}}finally{fs.closeSync(fd);}}catch{}
  return null;
}
// Compatibility adapter for Codex desktop 26.1106+. Both channels belong to
// the signed-in desktop app. The only tool calls permitted here are read-only.
class DesktopBridge extends EventEmitter {
  constructor(){super();this.threads=new Map();this.following=new Map();this.pending=new Map();this.clientId=null;this.nextId=0;this.connected=false;this.polling=false;this.completedUntil=0;this.contextThreadId=null;this.timer=null;}
  start(){this.tick();this.timer=setInterval(()=>this.tick(),3500);}
  snapshot(){return {...aggregate([...this.threads.values()],this.completedUntil),connected:this.connected};}
  publish(){this.emit('state',this.snapshot());}
  async tick(){if(this.polling)return;this.polling=true;try{
    const names=fs.readdirSync(PIPE_ROOT);const toolName=names.find(n=>n.startsWith('codex-app-tools-'));
    if(!toolName||!names.includes('codex-ipc')){this.disconnect();return;}
    if(this.toolPipeName!==toolName||!this.tools){this.disconnect();this.toolPipeName=toolName;this.connectTools(toolName);this.connectStream();}
    else if(!this.stream)this.connectStream();
    this.contextThreadId=recentThreadId()??this.contextThreadId;
    if(!this.contextThreadId){this.emit('issue','请先在 Codex 中打开一个聊天');return;}
    const result=await this.callTool('list_threads',{limit:50});
    this.connected=true;
    const rows=[...(result.pinnedThreads??[]),...(result.threads??[])].filter(t=>t.kind==='codex');const seen=new Set();
    for(const t of rows){const key=(t.hostId||'local')+':'+t.id;if(seen.has(key))continue;seen.add(key);const old=this.threads.get(key),active=classify(t.status)==='working'||classify(t.status)==='waiting';
      const next={id:t.id,hostId:t.hostId||'local',title:t.title||'Codex 任务',status:t.status};
      if(active&&old?.runtimeStatus)Object.assign(next,{runtimeStatus:old.runtimeStatus,requests:old.requests});
      this.threads.set(key,next);
      if(active&&!this.following.has(key)&&this.clientId)this.follow(key,next);
      if(!active&&old&&classify(old.runtimeStatus??old.status,old.requests)==='working'&&t.status!=='systemError')this.completedUntil=Date.now()+10000;
      if(!active&&this.following.has(key))this.unfollow(key);
    }
    for(const [key]of this.threads)if(!seen.has(key)){this.threads.delete(key);this.unfollow(key);}
    this.publish();
  }catch(e){this.emit('issue','Codex 状态连接暂不可用');if(this.tools?.socket.destroyed)this.disconnect();}finally{this.polling=false;}}
  connectTools(name){const channel=this.tools=new FramedSocket(PIPE_ROOT+name,8*1024*1024);channel.on('message',m=>{const p=this.pending.get(m.id);if(!p)return;this.pending.delete(m.id);clearTimeout(p.timer);m.error?p.reject(Error(m.error.message)):p.resolve(m.result);});channel.on('fault',()=>{});channel.on('close',()=>{if(this.tools===channel){this.tools=null;this.failPending();}});}
  connectStream(){const stream=this.stream=new FramedSocket(PIPE_ROOT+'codex-ipc');stream.on('connect',()=>stream.send({type:'request',requestId:randomUUID(),sourceClientId:'initializing-client',version:0,method:'initialize',params:{clientType:'desktop-pet'}}));stream.on('message',m=>{
    if(m.method==='initialize'&&m.result?.clientId){this.clientId=m.result.clientId;this.following.clear();for(const [key,t]of this.threads)if(classify(t.status)==='working')this.follow(key,t);return;}
    if(m.type==='client-discovery-request'){stream.send({type:'client-discovery-response',requestId:m.requestId,response:{canHandle:false}});return;}
    if(m.type!=='broadcast'||m.method!=='thread-stream-state-changed')return;
    const p=m.params;if(m.targetClientIds&&!m.targetClientIds.includes(this.clientId))return;
    const key=(p.hostId||'local')+':'+p.conversationId,t=this.threads.get(key);if(!t)return;
    const before=classify(t.runtimeStatus??t.status,t.requests),change=p.change;
    if(change?.type==='snapshot'){Object.assign(t,summarizeSnapshot(change.conversationState??{}));t.revision=change.revision;}
    else if(change?.type==='patches'){
      if(t.revision!==change.baseRevision){this.unfollow(key);this.follow(key,t);return;}
      applyStatusPatches(t,change.patches);t.revision=change.revision;
    }
    const after=classify(t.runtimeStatus??t.status,t.requests);if(['working','waiting'].includes(before)&&after==='idle')this.completedUntil=Date.now()+10000;
    this.publish();
  });stream.on('fault',()=>{});stream.on('close',()=>{if(this.stream===stream){this.clientId=null;this.stream=null;this.following.clear();}});}
  follow(key,t){if(!this.clientId||!this.stream)return;this.following.set(key,t);this.stream.send({type:'broadcast',method:'thread-stream-following-changed',sourceClientId:this.clientId,version:1,params:{conversationId:t.id,hostId:t.hostId,following:true}});}
  unfollow(key){const t=this.following.get(key);if(!t)return;this.following.delete(key);try{this.stream?.send({type:'broadcast',method:'thread-stream-following-changed',sourceClientId:this.clientId,version:1,params:{conversationId:t.id,hostId:t.hostId,following:false}});}catch{}}
  request(method,params){return new Promise((resolve,reject)=>{if(!this.tools){reject(Error('请先打开 Codex'));return;}const id=++this.nextId,timer=setTimeout(()=>{this.pending.delete(id);reject(Error('Codex 响应超时'));},15000);this.pending.set(id,{resolve,reject,timer});try{this.tools.send({jsonrpc:'2.0',id,method,params});}catch(e){clearTimeout(timer);this.pending.delete(id);reject(e);}});}
  async callTool(tool,args={}){if(!['get_usage_limits','list_threads'].includes(tool))throw Error('Read-only desktop bridge');const threadId=this.contextThreadId??recentThreadId();if(!threadId)throw Error('请先在 Codex 中打开一个聊天');const result=await this.request('tools/call',{namespace:'codex_app',tool,arguments:args,callerSource:'codex',threadId,callId:'desktop-pet-'+randomUUID(),turnId:'desktop-pet-observer'});if(!result.success)throw Error('Codex 读取失败');const text=result.contentItems?.find(i=>i.type==='inputText')?.text;if(!text)throw Error('Codex 返回格式不兼容');return JSON.parse(text);}
  async usage(){const raw=await this.callTool('get_usage_limits');return normalizeUsage(raw);}
  failPending(){for(const p of this.pending.values()){clearTimeout(p.timer);p.reject(Error('Codex 已断开'));}this.pending.clear();}
  disconnect(){const was=this.connected;this.connected=false;for(const [key]of this.following)this.unfollow(key);this.tools?.close();this.stream?.close();this.tools=null;this.stream=null;this.clientId=null;this.toolPipeName=null;this.failPending();this.threads.clear();this.completedUntil=0;if(was)this.publish();}
  stop(){clearInterval(this.timer);this.disconnect();}
}
module.exports={FramedSocket,DesktopBridge,recentThreadId};
