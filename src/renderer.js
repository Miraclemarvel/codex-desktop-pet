/* global pet, petMotion, petAppearance, petPlayback */
'use strict';
const canvas=document.getElementById('sprite'),ctx=canvas.getContext('2d',{willReadFrequently:true}),panel=document.getElementById('panel'),badge=document.getElementById('badge'),wrap=document.getElementById('pet-wrap');
let all=[],state={state:'idle',working:0,threads:[]},settings={},bitmap=null,anim=null,frame=-1,started=0,loading=0,paused=false,holdUntil=0,selectionState='',pointer=null,dragging=false,panelOpen=false,usage=null,error=null,busy=false,animationError=null;
const mask=document.createElement('canvas');mask.width=160;mask.height=90;const maskCtx=mask.getContext('2d',{willReadFrequently:true});
let idleSchedule=petMotion.createSchedule('quiet');
let stateOverride=null;
let canvasDpr=0;
let pendingAnimation=null;
const interactions=[];function rememberInteraction(type){interactions.push(type);if(interactions.length>8)interactions.shift();}
const pools={working:['写代码','忙碌点按','思考冒泡','轻快记录','吃Token'],waiting:['原地踱步张望','东张西望','思考冒泡'],complete:['雀跃庆祝','开心跃动','元气挥手'],error:['垂头叹气冒汗','被吓一跳'],drag:['被鼠标拖拽悬空反馈'],click:['元气挥手','害羞惊讶','挠痒咯咯笑','开心跃动']};
function choose(mode){if(mode==='idle')return petMotion.nextIdle(idleSchedule,all,anim?.id);let options=pools[mode]?all.filter(a=>pools[mode].includes(a.id)):all.filter(a=>!['工作状态','余额','拖拽','转向','移动','点击回应'].includes(a.category)&&!['写代码','轻快记录','吃Token'].includes(a.id));let diff=options.filter(a=>a.id!==anim?.id);if(diff.length)options=diff;return options[Math.floor(Math.random()*options.length)]??all[0];}
function play(a,force=false){
 if(!a)return Promise.resolve();
 if(pendingAnimation?.id===a.id&&!force)return pendingAnimation.promise;
 if(a.id===anim?.id&&bitmap&&!force){if(pendingAnimation){loading++;pendingAnimation=null;}return Promise.resolve();}
 const sequence=++loading;
 const promise=(async()=>{try{
  const data=await pet.animation(a.id),binary=atob(data.slice(data.indexOf(',')+1)),bytes=new Uint8Array(binary.length);
  for(let i=0;i<binary.length;i++)bytes[i]=binary.charCodeAt(i);
  const target=petAppearance.sheetSize(a.spritesheet,canvas.width);
  const next=await createImageBitmap(new Blob([bytes],{type:'image/webp'}),{resizeWidth:target.width,resizeHeight:target.height,resizeQuality:'high'});
  if(sequence!==loading){next.close();return;}
  bitmap?.close();bitmap=next;anim=a;frame=-1;started=performance.now();animationError=null;
  canvas.setAttribute('aria-label',a.name+'，点击查看剩余额度，拖动调整位置');
 }catch(e){animationError=String(e);if(!bitmap)setTimeout(()=>play(all.find(a=>a.id==='待机呼吸休闲')??all[0]),1000);}
 finally{if(pendingAnimation?.sequence===sequence)pendingAnimation=null;}})();
 pendingAnimation={id:a.id,sequence,promise};return promise;
}
function effectiveMode(){return dragging?'drag':Date.now()<holdUntil?selectionState:state.state;}
function updateState(s){if(stateOverride)s=stateOverride;const before=state.state;state=s;document.body.dataset.state=s.state;const labels={working:`正在工作${s.working>1?' · '+s.working+' 项':''}`,waiting:`等你确认${s.waiting>1?' · '+s.waiting+' 项':''}`,error:'遇到问题了',complete:'完成啦 ✧',idle:'陪你待机'};document.getElementById('status').textContent=s.connected?labels[s.state]:'等待 Codex';if(before!==s.state){if(s.state==='idle')idleSchedule=petMotion.createSchedule(settings.motion);if(s.state==='waiting'||s.state==='error')holdUntil=0;if(Date.now()>=holdUntil)play(choose(s.state));}}
function draw(now){requestAnimationFrame(draw);renderFrame(now);}
function renderFrame(now){
 if(!bitmap||!anim)return;
 const s=anim.spritesheet,range=petPlayback.clipRange(anim),elapsed=now-started;
 if(!paused&&elapsed>=range.durationMs){
  if(pendingAnimation)return;
  const next=choose(effectiveMode());
  if(next.id!==anim.id){play(next);return;}
  started=now;
 }
 const frozen=petMotion.freezeIdle(settings.motion,effectiveMode());
 const index=paused?Math.max(range.first,frame):petPlayback.frameAt(anim,now-started,frozen);
 if(index===frame)return;frame=index;
 const fw=bitmap.width/s.columns,fh=bitmap.height/s.rows;
 ctx.clearRect(0,0,canvas.width,canvas.height);ctx.drawImage(bitmap,(index%s.columns)*fw,Math.floor(index/s.columns)*fh,fw,fh,0,0,canvas.width,canvas.height);updateShape();
}
function setScale(s){
 const prior=settings.motion;settings={...s,motion:petMotion.normalizeMode(s.motion),appearance:petAppearance.normalize(s.appearance)};
 const w=480*(s.scale??1),bottom=60+w*360/640;
 wrap.style.width=w+'px';wrap.style.height=w*360/640+'px';panel.style.bottom=bottom+'px';panel.style.maxHeight=Math.max(120,window.innerHeight-bottom-12)+'px';
 canvasDpr=window.devicePixelRatio||1;const resolution=petAppearance.resolution(w,canvasDpr),resized=canvas.width!==resolution.width||canvas.height!==resolution.height;
 if(resized){canvas.width=resolution.width;canvas.height=resolution.height;ctx.imageSmoothingEnabled=true;ctx.imageSmoothingQuality='high';frame=-1;}
 canvas.style.filter=petAppearance.STYLES[settings.appearance].filter;
 if(prior!==settings.motion){idleSchedule=petMotion.createSchedule(settings.motion);if(bitmap&&effectiveMode()==='idle'){holdUntil=0;play(choose('idle'),resized);}}
 else if(resized&&bitmap&&anim)play(anim,true);
 if(bitmap)renderFrame(performance.now());updateShape();
}
function playInteraction(a){if(!a)return;selectionState='click';holdUntil=Date.now()+petPlayback.clipRange(a).durationMs;play(a);}
function preview(p){playInteraction(p.id?all.find(a=>a.id===p.id):petMotion.randomIdle(all,anim?.id));}
async function openPanel(){panelOpen=true;panel.hidden=false;renderUsage();updateShape();playInteraction(choose('click'));await refresh();}
function closePanel(){panelOpen=false;panel.hidden=true;updateShape();}
async function refresh(){if(busy)return;busy=true;document.getElementById('refresh').disabled=true;renderUsage();const result=await pet.usage();usage=result.usage;error=result.error;busy=false;document.getElementById('refresh').disabled=false;renderUsage();if(panelOpen&&usage&&!error){const min=Math.min(...usage.buckets.flatMap(b=>b.windows.map(w=>w.remaining).filter(n=>n!==null)));if(Number.isFinite(min)){const id=min<=0?'分文不剩':min<=10?'袋空如洗':min<=30?'数钱皱眉':min<=60?'钱袋如常':'钱袋满溢';playInteraction(all.find(a=>a.id===id)??choose('click'));}}}
function element(tag,className,text){const el=document.createElement(tag);if(className)el.className=className;if(text!==undefined)el.textContent=text;return el;}
function label(minutes){return minutes===10080?'每周额度':minutes===300?'5 小时额度':minutes===1440?'每日额度':minutes>=60&&minutes%60===0?`${minutes/60} 小时额度`:minutes?`${minutes} 分钟额度`:'使用额度';}
function dateText(seconds){if(!seconds)return '重置时间暂不可用';const d=new Date(seconds*1000);if(!Number.isFinite(d.getTime()))return '重置时间暂不可用';return d.toLocaleString('zh-CN',{month:'numeric',day:'numeric',hour:'2-digit',minute:'2-digit',hour12:false})+' 重置';}
function renderUsage(){const root=document.getElementById('quota');root.replaceChildren();if(!usage){root.append(element('p','hint'+(error?' error-text':''),busy?'读取中…':error?'读取失败，点刷新重试':'点刷新查看额度'));}else{
  if(error)root.append(element('p','hint error-text','刷新失败 · 显示上次数据'));
  if(usage.source==='cli')root.append(element('p','hint','CLI 日志快照 · '+new Date(usage.fetchedAt).toLocaleString('zh-CN')));
  for(const b of usage.buckets){const bucket=element('div','quota-bucket');if(usage.buckets.length>1)bucket.append(element('div','bucket-title',b.name));for(const w of b.windows){const row=element('div','quota-row'+(w.remaining!==null&&w.remaining<=0?' empty':w.remaining!==null&&w.remaining<=20?' low':'')),top=element('div','quota-top');top.append(element('span','quota-label',label(w.minutes)));const n=element('span','quota-number',w.remaining===null?'—':String(Math.round(w.remaining*10)/10));n.append(element('small','',w.remaining===null?'暂不可用':'% 剩余'));top.append(n);const track=element('div','track'),fill=element('div','fill');fill.style.width=(w.remaining??0)+'%';track.append(fill);row.append(top,track,element('div','reset',dateText(w.resetsAt)));bucket.append(row);}if(!b.windows.length)bucket.append(element('p','hint',b.credits?.unlimited?'无限额度':'额度暂不可用'));root.append(bucket);}
  if(!usage.buckets.length)root.append(element('p','hint','额度暂不可用'));
 }
 updateShape();
}
function updateShape(){const rects=[],r=canvas.getBoundingClientRect();maskCtx.clearRect(0,0,160,90);maskCtx.drawImage(canvas,0,0,160,90);const pixels=maskCtx.getImageData(0,0,160,90).data;for(let y=0;y<90;y++){let min=160,max=-1;for(let x=0;x<160;x++)if(pixels[(y*160+x)*4+3]>30){min=Math.min(min,x);max=x;}if(max>=min)rects.push({x:r.x+min*r.width/160-1,y:r.y+y*r.height/90-1,width:(max-min+1)*r.width/160+2,height:r.height/90+2});}for(const el of panelOpen?[panel,badge]:[badge]){const b=el.getBoundingClientRect();rects.push({x:b.x-10,y:b.y-10,width:b.width+20,height:b.height+20});}pet.shape(rects);}
let gestureToken=0;
canvas.addEventListener('pointerdown',e=>{if(e.button!==0)return;rememberInteraction('down');const token=++gestureToken;pointer={id:e.pointerId,token};pet.dragBegin(token);canvas.setPointerCapture(e.pointerId);});
pet.onDragState(({token,active})=>{if(token!==gestureToken)return;dragging=active;canvas.classList.toggle('dragging',active);if(active){rememberInteraction('drag');play(choose('drag'));}else updateShape();});
async function endPointer(e){if(!pointer)return;rememberInteraction(e.type);const current=pointer;pointer=null;let result;try{result=await pet.dragEnd(current.token,e.type==='pointercancel');}catch{result={stale:true};}if(current.token!==gestureToken)return;dragging=false;canvas.classList.remove('dragging');if(result.moved){holdUntil=0;play(choose(state.state));}else if(!result.stale&&e.type!=='pointercancel'){panelOpen?closePanel():openPanel();}updateShape();}
canvas.addEventListener('pointerup',endPointer);canvas.addEventListener('pointercancel',endPointer);canvas.addEventListener('lostpointercapture',e=>{if(pointer)endPointer({...e,type:'pointercancel'});});
canvas.addEventListener('keydown',e=>{if(e.key==='Enter'||e.key===' '){e.preventDefault();panelOpen?closePanel():openPanel();}});
badge.addEventListener('click',()=>panelOpen?closePanel():openPanel());document.getElementById('close').onclick=closePanel;document.getElementById('refresh').onclick=refresh;
document.addEventListener('contextmenu',e=>{e.preventDefault();pet.menu();});document.addEventListener('keydown',e=>{if(e.key==='Escape'){if(pointer)endPointer({type:'pointercancel'});closePanel();}});window.addEventListener('blur',()=>{if(pointer)endPointer({type:'pointercancel'});});
pet.onState(updateState);pet.onSettings(setScale);pet.onPreview(preview);pet.onPause(value=>{paused=value;if(!paused){started=performance.now();frame=-1;}});pet.onOpenPanel(openPanel);pet.onTogglePanel(()=>panelOpen?closePanel():openPanel());
setInterval(()=>{if(panelOpen)refresh();},60000);setInterval(()=>{if(canvasDpr&&canvasDpr!==(window.devicePixelRatio||1))setScale(settings);},1000);window.addEventListener('resize',()=>setScale(settings));
pet.boot().then(data=>{all=data.animations;setScale(data.settings);updateState(data.state);play(data.state.connected?choose(data.state.state):all.find(a=>a.id==='待机呼吸休闲'));requestAnimationFrame(draw);});
async function probeIdleMotion(mode){stateOverride={state:'idle',connected:true,working:0,waiting:0,errors:0,threads:[]};holdUntil=0;paused=false;updateState(stateOverride);setScale({...settings,motion:mode});idleSchedule=petMotion.createSchedule(mode);await play(choose('idle'));started=performance.now();frame=-1;renderFrame(started);const first=frame;renderFrame(started+400);return{mode:settings.motion,animation:anim?.id,firstFrame:first,lastFrame:frame,startFrame:petPlayback.clipRange(anim).first,nextGestureInMs:idleSchedule.nextAt-Date.now(),static:petMotion.freezeIdle(settings.motion,effectiveMode())};}
async function probeAppearance(value){stateOverride={state:'idle',connected:true,working:0,waiting:0,errors:0,threads:[]};holdUntil=0;paused=false;setScale({...settings,appearance:value});updateState(stateOverride);await play(all.find(a=>a.id==='待机呼吸休闲'));started=performance.now();frame=-1;renderFrame(started);paused=true;const output=document.createElement('canvas');output.width=canvas.width;output.height=canvas.height;const paint=output.getContext('2d');paint.filter=petAppearance.STYLES[settings.appearance].filter;paint.drawImage(canvas,0,0);return{appearance:settings.appearance,filter:getComputedStyle(canvas).filter,backing:{width:canvas.width,height:canvas.height},dpr:window.devicePixelRatio,cssWidth:canvas.getBoundingClientRect().width,animation:anim?.id,image:output.toDataURL('image/png')};}
function captureSourceFrame(index){const output=document.createElement('canvas');output.width=canvas.width;output.height=canvas.height;const paint=output.getContext('2d');paint.imageSmoothingQuality='high';paint.filter=petAppearance.STYLES[settings.appearance].filter;const s=anim.spritesheet,fw=bitmap.width/s.columns,fh=bitmap.height/s.rows;paint.drawImage(bitmap,(index%s.columns)*fw,Math.floor(index/s.columns)*fh,fw,fh,0,0,output.width,output.height);return output.toDataURL('image/png');}
async function probeColorTransitions(){
 const check=(value,message)=>{if(!value)throw Error(message);};
 stateOverride={state:'idle',connected:true,working:0,waiting:0,errors:0,threads:[]};holdUntil=0;paused=false;updateState(stateOverride);
 const idle=all.find(a=>a.id==='待机呼吸休闲'),work=all.find(a=>a.id==='忙碌点按');
 await play(work,true);started=performance.now();frame=-1;renderFrame(started+3000);
 const action=captureSourceFrame(frame),before={id:anim.id,frame,pixels:canvas.toDataURL()};
 const pending=play(idle,true);renderFrame(started+petPlayback.clipRange(anim).durationMs+100);
 check(anim.id===before.id&&frame===before.frame,'The old clip rewound during loading');
 check(canvas.toDataURL()===before.pixels,'The displayed pixels changed before the new clip was ready');
 await pending;renderFrame(started);check(frame===petPlayback.clipRange(idle).first,'The new clip began with a pale intro frame');
 const legacy=captureSourceFrame(0),fixed=captureSourceFrame(frame);
 const startFrame=frame;renderFrame(started+petPlayback.clipRange(idle).durationMs+100);check(frame===startFrame,'Idle loop returned to its pale intro');
 const stale=play(work,true);await play(idle);await stale;check(anim.id===idle.id&&!pendingAnimation,'A cancelled decode replaced the idle clip');
 const forced=play(idle,true),deduped=play(idle);check(forced===deduped,'A repeated request cancelled the same decode');await deduped;
 const cases=[];
 for(let i=0;i<3;i++){for(const clip of [work,idle]){await play(clip,true);renderFrame(started);check(frame>=24,'A switch displayed a pale leading frame');cases.push({animation:anim.id,frame,filter:getComputedStyle(canvas).filter});}}
 const staticIndex=petPlayback.frameAt(idle,0,true);check(staticIndex===24,'Static idle uses the pale reference frame');
 return{passed:true,startFrame,heldPreviousPixels:true,idleLoopStable:true,cancelledDecodeDiscarded:true,repeatedLoadDeduplicated:true,staticIndex,cases,images:{action,legacy,fixed}};
}
window.petTest={openPanel,closePanel,preview,probeIdleMotion,probeAppearance,probeColorTransitions,inspect:()=>({animation:anim?.id,frame,loadingAnimation:pendingAnimation?.id??null,motion:settings.motion,appearance:settings.appearance,dragging,pointerActive:!!pointer,animationError,panelOpen,interactions:[...interactions],quotaText:document.getElementById('quota').textContent,state:state.state,canvasReady:!!bitmap,rect:panel.getBoundingClientRect().toJSON()})};
