const {spawn}=require('node:child_process');
const os=require('node:os');
const {normalizeUsage}=require('./model.cjs');

// A short-lived account reader. It never starts/resumes threads or runs turns.
function readCliUsage({executable=process.env.CODEX_CLI_PATH||'codex.exe',spawnProcess=spawn,timeoutMs=20000}={}){
  return new Promise((resolve,reject)=>{
    let child,buffer=Buffer.alloc(0),settled=false,initialized=false;
    const timer=setTimeout(()=>finish(Error('CLI 额度读取超时')),timeoutMs);
    function finish(error,raw){
      if(settled)return;settled=true;clearTimeout(timer);
      if(child){child.stdin.destroy();child.stdout.destroy();child.kill();}
      error?reject(error):resolve(normalizeUsage(raw));
    }
    function send(message){child.stdin.write(JSON.stringify(message)+'\n');}
    try{
      child=spawnProcess(executable,['app-server','--listen','stdio://'],{windowsHide:true,cwd:os.homedir(),stdio:['pipe','pipe','ignore']});
      child.on('error',()=>finish(Error('无法启动 Codex CLI；请检查 CLI 安装或 CODEX_CLI_PATH')));
      child.on('exit',()=>finish(Error('CLI 额度连接已结束')));
      child.stdin.on('error',()=>finish(Error('CLI 额度连接写入失败')));
      child.stdout.on('error',()=>finish(Error('CLI 额度连接读取失败')));
      child.stdout.on('data',chunk=>{
        buffer=Buffer.concat([buffer,chunk]);
        if(buffer.length>1024*1024)return finish(Error('CLI 额度响应过大'));
        let end;
        while(!settled&&(end=buffer.indexOf(10))>=0){
          const line=buffer.subarray(0,end);buffer=buffer.subarray(end+1);
          let message;try{message=JSON.parse(line.toString('utf8'));}catch{continue;}
          if(![1,2].includes(message.id))continue;
          if(message.error)return finish(Error('CLI 无法读取账号额度；请确认已使用 ChatGPT 账号登录'));
          if(!message.result)continue;
          if(message.id===1&&!initialized){initialized=true;send({method:'initialized'});send({id:2,method:'account/rateLimits/read'});}
          else if(message.id===2&&initialized)finish(null,message.result);
        }
      });
      send({id:1,method:'initialize',params:{clientInfo:{name:'codex_desktop_pet',title:'Codex Desktop Pet',version:'0.4.1'}}});
    }catch{finish(Error('无法启动 Codex CLI 额度读取'));}
  });
}
module.exports={readCliUsage};
