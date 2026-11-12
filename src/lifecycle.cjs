function ownerPid(args){const at=args.indexOf('--codex-pid'),value=at<0?NaN:Number(args[at+1]);return Number.isSafeInteger(value)&&value>0?value:null;}
function processAlive(pid,probe=(value)=>process.kill(value,0)){
 if(!Number.isSafeInteger(pid)||pid<=0)return false;
 try{probe(pid);return true;}catch(e){return e.code!=='ESRCH';}
}
module.exports={ownerPid,processAlive};
