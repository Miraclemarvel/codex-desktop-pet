(function(root,factory){
  const api=factory();
  if(typeof module==='object'&&module.exports)module.exports=api;
  else root.petMotion=api;
})(globalThis,function(){
  'use strict';
  const BASE_IDLE='待机呼吸休闲';
  const MODES=Object.freeze({
    still:{label:'静态 · 待机不动',intervalMs:Infinity},
    quiet:{label:'安静 · 约 3 分钟一个小动作',intervalMs:180000},
    normal:{label:'适中 · 约 1 分钟一个动作',intervalMs:60000},
    lively:{label:'活泼 · 连续随机动作',intervalMs:0}
  });
  const SMALL_IDLE=new Set(['东张西望','悠闲哼歌','哈欠连天']);
  function normalizeMode(value){return Object.hasOwn(MODES,value)?value:'quiet';}
  function idlePool(all){return all.filter(a=>!['工作状态','余额','拖拽','转向','移动','点击回应'].includes(a.category)&&!['写代码','轻快记录','吃Token',BASE_IDLE].includes(a.id));}
  function pick(pool,previous,random){const different=pool.filter(a=>a.id!==previous);const choices=different.length?different:pool;return choices[Math.min(choices.length-1,Math.floor(random()*choices.length))];}
  function randomIdle(all,previous,random=Math.random){return pick(idlePool(all),previous,random)??all.find(a=>a.id===BASE_IDLE)??all[0];}
  function createSchedule(mode,now=Date.now()){const value=normalizeMode(mode);return{mode:value,nextAt:now+MODES[value].intervalMs,gesture:false};}
  function nextIdle(schedule,all,previous,now=Date.now(),random=Math.random){
    const base=all.find(a=>a.id===BASE_IDLE)??all[0];
    if(schedule.mode==='still')return base;
    if(schedule.mode==='lively')return randomIdle(all,previous,random);
    if(schedule.gesture){schedule.gesture=false;return base;}
    if(now<schedule.nextAt)return base;
    const pool=schedule.mode==='quiet'?all.filter(a=>SMALL_IDLE.has(a.id)):idlePool(all);
    schedule.nextAt=now+MODES[schedule.mode].intervalMs;
    schedule.gesture=true;
    return pick(pool,previous,random)??base;
  }
  function freezeIdle(mode,effectiveState){return normalizeMode(mode)==='still'&&effectiveState==='idle';}
  return{BASE_IDLE,MODES,normalizeMode,createSchedule,nextIdle,randomIdle,freezeIdle};
});
