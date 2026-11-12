const {test}=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),os=require('node:os'),path=require('node:path');
const {CliBridge,cliUsage}=require('../src/cli.cjs');
const since=Date.parse('2026-11-12T00:00:00Z');
const event=(type,extra={},timestamp='2026-11-12T00:00:01.123Z')=>JSON.stringify({timestamp,type:'event_msg',payload:{type,...extra}})+'\n';
function setup(t,source='cli',originator='codex-tui'){
  const home=fs.mkdtempSync(path.join(os.tmpdir(),'codex-pet-cli-'));
  t.after(()=>fs.rmSync(home,{recursive:true,force:true}));
  const root=path.join(home,'sessions','2026','11','12');fs.mkdirSync(root,{recursive:true});
  const file=path.join(root,'rollout-test.jsonl');
  fs.writeFileSync(file,JSON.stringify({type:'session_meta',payload:{id:'test',source,originator,base_instructions:'private instructions'}})+'\n');
  return {bridge:new CliBridge({home,since,usageReader:async()=>{throw Error('fixture offline');}}),file};
}
test('CLI works without desktop pipes and observes live completion and interruption',async t=>{
  const {bridge,file}=setup(t);bridge.tick();
  assert.equal(bridge.snapshot().connected,true);
  fs.appendFileSync(file,event('task_started'));bridge.tick();assert.equal(bridge.snapshot().state,'working');
  fs.appendFileSync(file,event('task_complete',{last_agent_message:'private answer'}));bridge.tick();assert.equal(bridge.snapshot().state,'complete');
  fs.appendFileSync(file,event('turn_started')+event('turn_aborted'));bridge.completedUntil=0;bridge.tick();assert.equal(bridge.snapshot().state,'idle');
  assert.ok(!JSON.stringify([...bridge.files.values()]).includes('private'));
  await assert.rejects(bridge.usage(),/fixture offline/);
});
test('bootstrap never celebrates historical completions or revives tasks predating the owner',t=>{
  const {bridge,file}=setup(t);
  fs.appendFileSync(file,event('task_started',{},'2026-11-11T00:00:00Z'));bridge.tick();assert.equal(bridge.snapshot().state,'idle');
  fs.appendFileSync(file,event('task_complete'));bridge.tick();assert.equal(bridge.snapshot().state,'idle');
  bridge.disconnect();bridge.tick();assert.equal(bridge.completedUntil,0);
});
test('desktop and subagent rollouts are excluded; vscode-originated CLI sessions are observed',t=>{
  for(const source of ['mcp',{subagent:{thread_spawn:{}}}]){
    const {bridge,file}=setup(t,source);fs.appendFileSync(file,event('task_started'));bridge.tick();assert.equal(bridge.snapshot().working,0);
  }
  for(const source of ['exec','vscode']){
    const {bridge,file}=setup(t,source);fs.appendFileSync(file,event('turn_started'));bridge.tick();assert.equal(bridge.snapshot().working,1);
  }
});
test('session metadata distinguishes Desktop and IDE sessions from supported CLI origins',t=>{
  const cases=require('./fixtures/cli-origins.json');
  for(const {metadata,expected}of cases){
    const {bridge,file}=setup(t);
    fs.writeFileSync(file,JSON.stringify({type:'session_meta',payload:{id:'test',...metadata}})+'\n'+event('task_started'));
    bridge.tick();
    assert.equal(bridge.snapshot().working,expected?1:0,JSON.stringify(metadata));
  }
});
test('partial UTF-8 records, malformed JSON, truncation, and deletion recover',t=>{
  const {bridge,file}=setup(t);bridge.tick();
  const line=Buffer.from(event('task_started',{ignored:'私密'})),cut=line.indexOf(Buffer.from('私'))+1;
  fs.appendFileSync(file,line.subarray(0,cut));bridge.tick();assert.equal(bridge.snapshot().state,'idle');
  fs.appendFileSync(file,Buffer.concat([line.subarray(cut),Buffer.from('malformed\n')]));bridge.tick();assert.equal(bridge.snapshot().state,'working');
  fs.writeFileSync(file,JSON.stringify({type:'session_meta',payload:{id:'test',source:'cli'}})+'\n');bridge.tick();assert.equal(bridge.snapshot().state,'idle');
  fs.unlinkSync(file);bridge.tick();assert.equal(bridge.snapshot().working,0);
});
test('large transcript lines are skipped with bounded reads and no retained text',t=>{
  const {bridge,file}=setup(t);bridge.tick();
  fs.appendFileSync(file,'x'.repeat(2*1024*1024)+'\n'+event('task_started'));
  for(let i=0;i<4;i++)bridge.tick();
  assert.equal(bridge.snapshot().state,'working');assert.ok(JSON.stringify([...bridge.files.values()]).length<512);
});
test('quota uses snake_case CLI fields, preserves unknown data and original timestamp',async t=>{
  const {bridge,file}=setup(t);bridge.tick();
  fs.appendFileSync(file,event('token_count',{info:{private:'usage details'},rate_limits:{limit_id:'codex',primary:{used_percent:21,window_minutes:300,resets_at:1800000000},secondary:{used_percent:null}}}));
  bridge.tick();const usage=bridge.cachedUsage;
  assert.deepEqual(usage.buckets[0].windows.map(w=>w.remaining),[79,null]);assert.equal(usage.buckets[0].windows[0].minutes,300);
  assert.equal(usage.fetchedAt,Date.parse('2026-11-12T00:00:01.123Z'));assert.equal(usage.source,'cli');
  fs.appendFileSync(file,event('token_count',{rate_limits:null}));bridge.tick();assert.equal(bridge.cachedUsage.fetchedAt,usage.fetchedAt);
  assert.equal(cliUsage({primary:{used_percent:120}},since).buckets[0].windows[0].remaining,0);
});
test('live quota and reset count do not depend on any matching session log',async()=>{
  const bridge=new CliBridge({home:path.join(os.tmpdir(),'nonexistent-codex-home'),executable:'native-codex.exe',usageReader:async({executable})=>{
    assert.equal(executable,'native-codex.exe');return {buckets:[],resetsAvailable:2};
  }});
  assert.equal((await bridge.usage()).resetsAvailable,2);
});
