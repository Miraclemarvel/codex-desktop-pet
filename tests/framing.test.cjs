const {test}=require('node:test'),assert=require('node:assert/strict'),net=require('node:net'),os=require('node:os'),path=require('node:path'),{randomUUID}=require('node:crypto');
const {FramedSocket,DesktopBridge}=require('../src/bridge.cjs');
test('desktop protocol decodes fragmented and coalesced frames',async()=>{
 const endpoint=process.platform==='win32'?'\\\\.\\pipe\\pet-test-'+randomUUID():path.join(os.tmpdir(),randomUUID()+'.sock');
 const payloads=[{message:'额度 80%'},{type:'broadcast',params:{active:true}}];
 const frame=m=>{const d=Buffer.from(JSON.stringify(m)),h=Buffer.alloc(4);h.writeUInt32LE(d.length);return Buffer.concat([h,d]);};
 const server=net.createServer(s=>{const d=Buffer.concat(payloads.map(frame));s.write(d.subarray(0,2));setTimeout(()=>s.write(d.subarray(2,9)),5);setTimeout(()=>s.end(d.subarray(9)),10);});
 await new Promise(r=>server.listen(endpoint,r));const client=new FramedSocket(endpoint);const got=[];await new Promise((resolve,reject)=>{client.on('message',m=>got.push(m));client.on('close',resolve);client.on('fault',reject);});assert.deepEqual(got,payloads);await new Promise(r=>server.close(r));
});
test('bridge refuses mutating app tools',async()=>{const b=new DesktopBridge();await assert.rejects(b.callTool('delete_thread',{}),/Read-only/);await assert.rejects(b.callTool('automation_update',{}),/Read-only/);});
