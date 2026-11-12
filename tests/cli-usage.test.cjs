const {test}=require('node:test'),assert=require('node:assert/strict');
const {spawn}=require('node:child_process');
const fs=require('node:fs'),os=require('node:os'),path=require('node:path');
const {readCliUsage}=require('../src/cli-usage.cjs');
function fixture(t,scenario){
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),'codex-pet-account-'));
  t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));
  const file=path.join(dir,'account.cjs');
  fs.writeFileSync(file,`
    const rl=require('node:readline').createInterface({input:process.stdin});
    let initialized=false,ack=false;
    rl.on('line',line=>{
      const m=JSON.parse(line);
      if(m.method==='initialize'&&!initialized){initialized=true;
        if('${scenario}'==='timeout')return;
        if('${scenario}'==='exit')return process.exit(0);
        process.stdout.write(JSON.stringify({id:1,result:{userAgent:'fixture'}})+'\\n');
      }else if(m.method==='initialized'&&initialized){ack=true;
      }else if(m.method==='account/rateLimits/read'&&ack){
        if('${scenario}'==='error')return process.stdout.write(JSON.stringify({id:2,error:{code:-1,message:'not signed in'}})+'\\n');
        if('${scenario}'==='oversize')return process.stdout.write('x'.repeat(1024*1024+1));
        const body=Buffer.from(JSON.stringify({id:2,result:{rateLimits:{limitName:'常规额度',primary:{usedPercent:35,windowDurationMins:10080,resetsAt:1800000000}},rateLimitResetCredits:{availableCount:2}}})+'\\n');
        const cut=body.indexOf(Buffer.from('常'))+1;
        process.stdout.write(body.subarray(0,cut));setTimeout(()=>process.stdout.write(body.subarray(cut)),10);
      }else{process.stderr.write('Unexpected mutating call or handshake order');process.exit(6);}
    });
  `);
  let child;
  return {
    options:{timeoutMs:scenario==='timeout'?250:3000,spawnProcess:(exe,args,options)=>{
      assert.deepEqual(args,['app-server','--listen','stdio://']);assert.equal(options.windowsHide,true);
      child=spawn(process.execPath,[file],options);return child;
    }},
    exited:async()=>{if(child.exitCode===null&&child.signalCode===null)await new Promise(resolve=>child.once('exit',resolve));assert.ok(child.exitCode!==null||child.signalCode!==null);}
  };
}
test('account reader initializes once, reads live quota/reset count, and cleans up its child',async t=>{
  const f=fixture(t,'success'),usage=await readCliUsage(f.options);
  assert.equal(usage.buckets[0].name,'常规额度');assert.equal(usage.buckets[0].windows[0].remaining,65);
  assert.equal(usage.resetsAvailable,2);assert.ok(Date.now()-usage.fetchedAt<2000);await f.exited();
});
for(const scenario of ['error','timeout','exit','oversize'])test('account reader handles '+scenario+' and cleans up its child',async t=>{
  const f=fixture(t,scenario);await assert.rejects(readCliUsage(f.options));await f.exited();
});
