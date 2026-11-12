const {test}=require('node:test'),assert=require('node:assert/strict');
const {ownerPid,processAlive}=require('../src/lifecycle.cjs');
test('only a positive integer can bind a Codex owner process',()=>{
 assert.equal(ownerPid(['app','--codex-pid','29528']),29528);
 for(const value of ['-1','0','1.5','invalid','Infinity'])assert.equal(ownerPid(['--codex-pid',value]),null);
 assert.equal(ownerPid([]),null);
});
test('a missing owner exits the pet while access-denied probes do not cause false exits',()=>{
 assert.equal(processAlive(29528,()=>{}),true);
 assert.equal(processAlive(29528,()=>{throw Object.assign(Error(),{code:'ESRCH'});}),false);
 assert.equal(processAlive(29528,()=>{throw Object.assign(Error(),{code:'EPERM'});}),true);
 assert.equal(processAlive(null,()=>{throw Error('must not probe');}),false);
});
