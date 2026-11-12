const {test}=require('node:test'),assert=require('node:assert/strict');
const {classify,aggregate,normalizeUsage,chooseAnimation,summarizeSnapshot,applyStatusPatches}=require('../src/model.cjs');
test('pending user decisions outrank other concurrent work',()=>{
 const result=aggregate([{id:'a',status:'active'},{id:'b',runtimeStatus:{type:'active',activeFlags:['waitingOnApproval']}}]);
 assert.equal(result.state,'waiting');assert.equal(result.working,1);assert.equal(result.waiting,1);
 assert.equal(classify({type:'idle'},[true]),'waiting');
});
test('completion is temporary and does not replace a running or blocked task',()=>{
 assert.equal(aggregate([],110,100).state,'complete');assert.equal(aggregate([],110,120).state,'idle');
 assert.equal(aggregate([{status:'active'}],110,100).state,'working');
 assert.equal(aggregate([{status:'systemError'}],110,100).state,'error');
});
test('multi-bucket quota preserves unavailable data and clamps remaining percent',()=>{
 const r=normalizeUsage({rateLimits:{primary:{usedPercent:100}},rateLimitsByLimitId:{a:{limitId:'a',primary:{usedPercent:21,windowDurationMins:10080,resetsAt:100},secondary:{usedPercent:null}},b:{limitId:'b',primary:{usedPercent:120},secondary:{usedPercent:-5}}}},999);
 assert.equal(r.fetchedAt,999);assert.deepEqual(r.buckets[0].windows.map(w=>w.remaining),[79,null]);assert.deepEqual(r.buckets[1].windows.map(w=>w.remaining),[0,100]);assert.deepEqual(normalizeUsage(null).buckets,[]);
});
test('status shadow discards transcript and applies approval patches without storing requests',()=>{
 const s=summarizeSnapshot({threadRuntimeStatus:{type:'active',activeFlags:[]},requests:[],title:'A',turns:[{content:'private'}],latestThreadSettings:{private:'private'}});
 applyStatusPatches(s,[{op:'add',path:['requests',0],value:{params:{prompt:'private'}}},{op:'replace',path:['turns',0,'content'],value:'private'}]);assert.equal(classify(s.runtimeStatus,s.requests),'waiting');assert.ok(!JSON.stringify(s).includes('private'));
 applyStatusPatches(s,[{op:'remove',path:['requests',0]},{op:'replace',path:['threadRuntimeStatus','activeFlags'],value:['waitingOnUserInput']}]);assert.equal(classify(s.runtimeStatus,s.requests),'waiting');
 applyStatusPatches(s,[{op:'remove',path:['threadRuntimeStatus','activeFlags',0]},{op:'replace',path:['threadRuntimeStatus','type'],value:'idle'}]);assert.equal(classify(s.runtimeStatus,s.requests),'idle');
});
test('all animation references exist and state selections avoid repeats',()=>{
 const fs=require('node:fs'),path=require('node:path');const root=path.resolve(__dirname,'..');const all=JSON.parse(fs.readFileSync(path.join(root,'assets/upstream-actions.json'))).animations;
 assert.equal(all.length,92);for(const a of all){assert.ok(fs.existsSync(path.join(root,'assets',a.sources.webp)),a.id);assert.ok(a.spritesheet.frameCount<=a.spritesheet.rows*a.spritesheet.columns);}
 for(const state of ['working','waiting','error','complete','idle','drag','click']){const a=chooseAnimation(all,state,null,()=>0);assert.ok(a);if(state!=='drag')assert.notEqual(chooseAnimation(all,state,a.id,()=>0).id,a.id);}
});
