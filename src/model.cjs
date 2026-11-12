const ACTIVE = new Set(['active','running','inProgress']);
const WAITING = new Set(['waitingOnApproval','waitingOnUserInput','needsInput','waiting_for_input']);
function classify(status, requests=[]) {
  const type=typeof status==='string'?status:status?.type;
  if(requests.length || status?.activeFlags?.some(f=>WAITING.has(f)) || WAITING.has(type))return 'waiting';
  if(['systemError','failed','blocked','error'].includes(type))return 'error';
  return ACTIVE.has(type)?'working':'idle';
}
function aggregate(threads, completedUntil=0, now=Date.now()) {
  const states=threads.map(t=>({...t,state:classify(t.runtimeStatus??t.status,t.requests)}));
  const waiting=states.filter(t=>t.state==='waiting'),errors=states.filter(t=>t.state==='error'),working=states.filter(t=>t.state==='working');
  const state=waiting.length?'waiting':errors.length?'error':working.length?'working':completedUntil>now?'complete':'idle';
  return {state,working:working.length,waiting:waiting.length,errors:errors.length,threads:states.filter(t=>t.state!=='idle').map(({id,hostId,title,state})=>({id,hostId,title,state}))};
}
function normalizeUsage(raw, now=Date.now()) {
  const byId=raw?.rateLimitsByLimitId;
  const buckets=byId&&Object.keys(byId).length?Object.values(byId):raw?.rateLimits?[raw.rateLimits]:[];
  return {fetchedAt:now,buckets:buckets.map(b=>({id:b.limitId??'codex',name:b.limitName??'Codex',windows:['primary','secondary'].flatMap(key=>{
    const w=b[key];if(!w)return [];
    return [{key,remaining:typeof w.usedPercent==='number'&&Number.isFinite(w.usedPercent)?Math.max(0,Math.min(100,100-w.usedPercent)):null,minutes:w.windowDurationMins??null,resetsAt:w.resetsAt??null}];
  }),credits:b.credits?{unlimited:b.credits.unlimited===true,balance:b.credits.balance??null}:null})),resetsAvailable:raw?.rateLimitResetCredits?.availableCount??null};
}
const POOLS={
 working:['写代码','忙碌点按','思考冒泡','轻快记录','吃Token'],
 waiting:['原地踱步张望','东张西望','思考冒泡'],
 complete:['雀跃庆祝','开心跃动','元气挥手'],
 error:['垂头叹气冒汗','被吓一跳'],
 click:['元气挥手','开心跃动','害羞惊讶','挠痒咯咯笑'],
 drag:['被鼠标拖拽悬空反馈']
};
function chooseAnimation(all,state,previous,random=Math.random) {
  const ids=POOLS[state];
  let pool=ids?all.filter(a=>ids.includes(a.id)):all.filter(a=>!['工作状态','余额','拖拽','转向','移动','点击回应'].includes(a.category)&&a.id!=='写代码'&&a.id!=='轻快记录'&&a.id!=='吃Token');
  if(!pool.length)pool=all;
  const different=pool.filter(a=>a.id!==previous);if(different.length)pool=different;
  return pool[Math.min(pool.length-1,Math.floor(random()*pool.length))];
}
// Keep only the fields needed by the pet. Prompts, messages and tool arguments
// received through the desktop stream are never kept in the shadow state.
function summarizeSnapshot(state) {
  return {runtimeStatus:state.threadRuntimeStatus??state.status,requests:Array.isArray(state.requests)?state.requests.map(()=>true):[],title:state.title??''};
}
function applyStatusPatches(shadow,patches) {
  for(const p of patches??[]) {
    const path=Array.isArray(p.path)?p.path:typeof p.path==='string'?p.path.split('/').filter(Boolean):[];
    const key=path[0];
    if(key==='threadRuntimeStatus'||key==='status'){
      if(path.length===1)shadow.runtimeStatus=p.value;
      else if(path[1]==='type')shadow.runtimeStatus={...(typeof shadow.runtimeStatus==='object'?shadow.runtimeStatus:{}),type:p.value};
      else if(path[1]==='activeFlags'){
        const current=typeof shadow.runtimeStatus==='object'?shadow.runtimeStatus:{type:'active'};
        if(path.length===2)current.activeFlags=p.value??[];
        else {current.activeFlags=[...(current.activeFlags??[])];if(p.op==='remove')current.activeFlags.splice(Number(path[2]),1);else current.activeFlags[Number(path[2])]=p.value;}
        shadow.runtimeStatus=current;
      }
    } else if(key==='requests') {
      if(path.length===1)shadow.requests=Array.isArray(p.value)?p.value.map(()=>true):[];
      else if(path.length===2){shadow.requests??=[];const at=path[1]==='-'?shadow.requests.length:Number(path[1]);if(p.op==='remove')shadow.requests.splice(at,1);else if(p.op==='add')shadow.requests.splice(at,0,true);else shadow.requests[at]=true;}
    } else if(key==='title'&&path.length===1)shadow.title=typeof p.value==='string'?p.value:'';
  }
  return shadow;
}
module.exports={classify,aggregate,normalizeUsage,chooseAnimation,summarizeSnapshot,applyStatusPatches};
