const {test}=require('node:test'),assert=require('node:assert/strict');
const {BASE_IDLE,normalizeMode,createSchedule,nextIdle,freezeIdle,randomIdle}=require('../src/motion.js');
const all=[{id:BASE_IDLE,category:'待机'},{id:'东张西望',category:'小动作'},{id:'悠闲哼歌',category:'小动作'},{id:'三球抛接',category:'才艺表演'},{id:'写代码',category:'工作职业'}];
test('default quiet idle breathes for three minutes, performs one small gesture, then rests',()=>{
 assert.equal(normalizeMode(undefined),'quiet');const s=createSchedule(undefined,0);
 assert.equal(nextIdle(s,all,null,0).id,BASE_IDLE);
 assert.equal(nextIdle(s,all,BASE_IDLE,179999).id,BASE_IDLE);
 assert.equal(nextIdle(s,all,BASE_IDLE,180000,()=>0).id,'东张西望');
 assert.equal(nextIdle(s,all,'东张西望',190000).id,BASE_IDLE);
 assert.equal(nextIdle(s,all,BASE_IDLE,359999).id,BASE_IDLE);
});
test('static setting applies only to idle and permits direct interactions and task feedback',()=>{
 const s=createSchedule('still',0);assert.equal(nextIdle(s,all,null,10000000).id,BASE_IDLE);
 assert.equal(freezeIdle('still','idle'),true);
 for(const state of ['working','waiting','complete','error','click','drag'])assert.equal(freezeIdle('still',state),false);
 assert.equal(freezeIdle('quiet','idle'),false);
});
test('normal and lively rates differ; changing the preference resets the idle interval',()=>{
 const normal=createSchedule('normal',0);assert.equal(nextIdle(normal,all,null,59999).id,BASE_IDLE);assert.notEqual(nextIdle(normal,all,BASE_IDLE,60000,()=>0).id,BASE_IDLE);
 const lively=createSchedule('lively',0);const first=nextIdle(lively,all,null,0,()=>0);assert.notEqual(first.id,BASE_IDLE);assert.notEqual(nextIdle(lively,all,first.id,10000,()=>0).id,first.id);
 const quiet=createSchedule('quiet',100000);assert.equal(nextIdle(quiet,all,null,100000).id,BASE_IDLE);assert.equal(quiet.nextAt,280000);
});
test('missed intervals do not replay a burst of idle gestures; manual random selection stays available',()=>{
 const s=createSchedule('quiet',0);assert.notEqual(nextIdle(s,all,null,999999,()=>0).id,BASE_IDLE);assert.equal(s.nextAt,1179999);
 assert.equal(nextIdle(s,all,null,1009999).id,BASE_IDLE);assert.equal(nextIdle(s,all,null,1019999).id,BASE_IDLE);
 assert.notEqual(randomIdle(all,BASE_IDLE,()=>0).id,'写代码');assert.notEqual(randomIdle(all,BASE_IDLE,()=>0).id,BASE_IDLE);
});
