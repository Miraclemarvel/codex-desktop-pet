const {app,BrowserWindow,screen}=require('electron');
const fs=require('node:fs'),path=require('node:path');
app.disableHardwareAcceleration();
app.on('window-all-closed',()=>{});
app.whenReady().then(async()=>{
 const result=[];
 try{
  for(const fixed of [false,true]){
   const win=new BrowserWindow({width:520,height:620,x:100,y:80,show:false,frame:false,transparent:true,resizable:false,hasShadow:false,...fixed?{thickFrame:false,minWidth:520,maxWidth:520,minHeight:620,maxHeight:620}:{}});
   const initial=win.getBounds();
   let maxHeight=initial.height;
   for(let i=0;i<180;i++){
    const x=100+(i%2?12:28),y=80+(i%2?8:18);
    if(fixed)win.setBounds({x,y,width:520,height:620},false);
    else win.setPosition(x,y,false);
    maxHeight=Math.max(maxHeight,win.getBounds().height);
    if(i%30===0)await new Promise(r=>setTimeout(r,10));
   }
   result.push({fixed,initial,after:win.getBounds(),maxHeight});win.destroy();
  }
  const dir=process.env.PET_EVIDENCE_DIR||path.join(__dirname,'../outputs/drag-qa');fs.mkdirSync(dir,{recursive:true});fs.writeFileSync(path.join(dir,'window-growth-repro.json'),JSON.stringify({displays:screen.getAllDisplays().map(d=>({scaleFactor:d.scaleFactor,bounds:d.bounds,workArea:d.workArea})),result},null,2));
 }finally{app.quit();}
});
