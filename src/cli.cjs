const fs=require('node:fs');
const os=require('node:os');
const path=require('node:path');
const {EventEmitter}=require('node:events');
const {aggregate,normalizeUsage}=require('./model.cjs');
const {readCliUsage}=require('./cli-usage.cjs');
const MAX_READ=1024*1024;

function rolloutFiles(root){
  const found=[];
  function walk(dir,depth){
    if(depth>4)return;
    let entries;try{entries=fs.readdirSync(dir,{withFileTypes:true});}catch{return;}
    for(const e of entries){
      const file=path.join(dir,e.name);
      if(e.isDirectory())walk(file,depth+1);
      else if(e.isFile()&&/^rollout-.*\.jsonl$/.test(e.name)){
        try{found.push({file,mtime:fs.statSync(file).mtimeMs});}catch{}
      }
    }
  }
  walk(root,0);
  return found.sort((a,b)=>b.mtime-a.mtime).slice(0,50);
}
function cliUsage(raw,at){
  const window=w=>w?{usedPercent:w.used_percent,windowDurationMins:w.window_minutes,resetsAt:w.resets_at}:null;
  return {...normalizeUsage({rateLimits:{limitId:raw.limit_id,limitName:raw.limit_name,primary:window(raw.primary),secondary:window(raw.secondary),credits:raw.credits}},at),source:'cli'};
}
// Observe durable CLI events only. No commands, prompts, or transcript text are
// retained. Reads and bootstrap tails are bounded even for very large rollouts.
class CliBridge extends EventEmitter {
  constructor({home=process.env.CODEX_HOME||path.join(os.homedir(),'.codex'),since=Date.now(),executable,usageReader=readCliUsage}={}){
    super();this.root=path.join(home,'sessions');this.since=since;this.files=new Map();this.completedUntil=0;this.cachedUsage=null;this.executable=executable;this.usageReader=usageReader;
  }
  start(){this.tick();this.timer=setInterval(()=>this.tick(),3500);}
  snapshot(){return {...aggregate([...this.files.values()].filter(f=>f.cli).map(f=>({id:f.id,hostId:'cli',title:'Codex CLI',status:f.status})),this.completedUntil),connected:true};}
  consume(f,r,live){
    const p=r.payload;
    if(!p||r.type!=='event_msg'||!f.cli)return;
    const at=Date.parse(r.timestamp);
    if(!Number.isFinite(at))return;
    if(at<this.since)return;
    if(p.type==='token_count'&&p.rate_limits&&(!this.cachedUsage||at>=this.cachedUsage.fetchedAt))this.cachedUsage=cliUsage(p.rate_limits,at);
    if(['task_started','turn_started'].includes(p.type))f.status='active';
    else if(['task_complete','turn_complete'].includes(p.type)){
      if(live&&['active','waitingOnApproval','waitingOnUserInput'].includes(f.status))this.completedUntil=Date.now()+10000;
      f.status='idle';
    }else if(['turn_aborted','shutdown_complete'].includes(p.type))f.status='idle';
    // These events are transient in current CLI releases. Honor them if an
    // older/future release persists them, without inferring approval from silence.
    else if(['exec_approval_request','apply_patch_approval_request'].includes(p.type))f.status='waitingOnApproval';
    else if(p.type==='request_user_input')f.status='waitingOnUserInput';
    else if(['exec_command_begin','patch_apply_begin','mcp_tool_call_begin'].includes(p.type))f.status='active';
    else if(p.type==='error')f.status='systemError';
  }
  read(file,f){
    const fd=fs.openSync(file,'r');
    try{
      const size=fs.fstatSync(fd).size;
      if(size<f.offset)Object.assign(f,{offset:0,initialized:false,cli:false,status:'idle',dropping:false});
      const live=f.initialized;
      if(!f.initialized){
        const header=Buffer.alloc(Math.min(size,MAX_READ));fs.readSync(fd,header,0,header.length,0);
        const end=header.indexOf(10);if(end<0)return;
        let meta;try{meta=JSON.parse(header.subarray(0,end).toString('utf8'));}catch{return;}
        f.cli=meta.type==='session_meta'&&['cli','exec','vscode'].includes(meta.payload?.source);
        f.id=typeof meta.payload?.id==='string'?meta.payload.id:path.basename(file);
        f.initialized=true;
        f.offset=Math.max(end+1,size-MAX_READ);f.dropping=f.offset>end+1;
      }
      if(!f.cli){f.offset=size;return;}
      const buffer=Buffer.alloc(Math.min(MAX_READ,size-f.offset));
      const count=fs.readSync(fd,buffer,0,buffer.length,f.offset);
      let start=0;
      for(let i=0;i<count;i++)if(buffer[i]===10){
        if(!f.dropping){try{this.consume(f,JSON.parse(buffer.subarray(start,i).toString('utf8')),live);}catch{}}
        f.dropping=false;start=i+1;
      }
      if(start===0&&count===MAX_READ){f.offset+=count;f.dropping=true;}
      else f.offset+=start;
    }finally{fs.closeSync(fd);}
  }
  tick(){
    const entries=rolloutFiles(this.root),seen=new Set(entries.map(e=>e.file));
    for(const file of this.files.keys())if(!seen.has(file))this.files.delete(file);
    for(const {file,mtime}of entries){
      // Old sessions must never revive an old task or supply another login's quota.
      let f=this.files.get(file);if(!f){f={offset:0,initialized:false,cli:false,status:'idle',dropping:false};this.files.set(file,f);}
      try{this.read(file,f);}catch{this.files.delete(file);}
      if(mtime<this.since)f.status='idle';
    }
    this.emit('state',this.snapshot());
  }
  async usage(){return this.usageReader({executable:this.executable});}
  disconnect(){this.files.clear();this.completedUntil=0;this.cachedUsage=null;this.since=Date.now();}
  stop(){clearInterval(this.timer);this.disconnect();}
}
module.exports={CliBridge,cliUsage,rolloutFiles};
