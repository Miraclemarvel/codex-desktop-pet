const {app,BrowserWindow,ipcMain,Menu,screen}=require('electron');
const {spawn,execFileSync}=require('node:child_process');
const {ownerPid,processAlive}=require('./lifecycle.cjs');
const {MODES,normalizeMode}=require('./motion.js');
const {STYLES,normalize:normalizeAppearance}=require('./appearance.js');
const {WINDOW_SIZE,constrainWindow,DragSession}=require('./drag.cjs');
const fs=require('node:fs'),path=require('node:path');
const {DesktopBridge}=require('./bridge.cjs');
const ROOT=path.resolve(__dirname,'..'),{width:WIDTH,height:HEIGHT}=WINDOW_SIZE;
const dragTesting=process.argv.includes('--drag-smoke');
app.setName('Codex Desktop Pet');app.setAppUserModelId('local.codex.desktop.pet');
app.disableHardwareAcceleration();
const startedAt=Date.now();
const testDataIndex=process.argv.indexOf('--test-user-data');
const isolatedData=testDataIndex>=0?process.argv[testDataIndex+1]:null;
if(isolatedData){fs.mkdirSync(path.resolve(isolatedData),{recursive:true});app.setPath('userData',path.resolve(isolatedData));}
let codexPid=ownerPid(process.argv);
if(!app.requestSingleInstanceLock()){app.quit();}else{app.whenReady().then(start);}
let win,bridge,settings={},settingsPath,animations=[],usage=null,usageError=null,usagePromise=null;
let drag=null,ignore=true,paused=false,ready=false,latest={state:'idle',connected:false,working:0,waiting:0,errors:0,threads:[]};
function send(name,value){if(win&&!win.isDestroyed()&&ready)win.webContents.send(name,value);}
function save(){try{fs.writeFileSync(settingsPath,JSON.stringify(settings,null,2));}catch{}}
function hostPath(){return app.isPackaged?path.join(path.dirname(process.execPath),'CodexPetHost.exe'):path.join(ROOT,'native','CodexPetHost.exe');}
function startHost(){if(isolatedData)return;const exe=hostPath();if(!fs.existsSync(exe))throw Error('CodexPetHost.exe is missing');const child=spawn(exe,[],{windowsHide:true,detached:true,stdio:'ignore'});child.on('error',e=>console.error('host:',e.message));child.unref();}
function stopHost(){const exe=hostPath();if(fs.existsSync(exe))execFileSync(exe,['--stop'],{windowsHide:true,timeout:5000,stdio:'ignore'});}
function setStartup(enabled,launch=true){app.setLoginItemSettings({openAtLogin:enabled,path:hostPath(),args:[]});settings.autoStart=enabled;save();if(launch){if(enabled)startHost();else stopHost();}}
function setMotion(value){settings.motion=normalizeMode(value);save();send('settings',settings);}
function setAppearance(value){settings.appearance=normalizeAppearance(value);save();send('settings',settings);}
function menu(){const cats=[...new Set(animations.map(a=>a.category))];return Menu.buildFromTemplate([
 {label:'Codex 白龙 · GPT 动作包',enabled:false},
 {label:'查看剩余额度',click:()=>{win.showInactive();send('open-panel');}},
 {label:'随机换个动作',click:()=>send('preview',{random:true})},
 {label:paused?'继续动画':'暂停动画',click:()=>{paused=!paused;send('pause',paused);}},
 {label:'动作图鉴 · '+animations.length,submenu:cats.map(category=>({label:category,submenu:animations.filter(a=>a.category===category).map(a=>({label:a.name,click:()=>send('preview',{id:a.id})}))}))},
 {label:'画面效果',submenu:Object.entries(STYLES).map(([value,style])=>({label:style.label,type:'radio',checked:settings.appearance===value,click:()=>setAppearance(value)}))},
 {label:'待机动态',submenu:Object.entries(MODES).map(([value,mode])=>({label:mode.label,type:'radio',checked:settings.motion===value,click:()=>setMotion(value)}))},
 {label:'桌宠大小',submenu:[['小',0.8],['标准',1],['大',1.2]].map(([label,value])=>({label,type:'radio',checked:settings.scale===value,click:()=>{settings.scale=value;save();send('settings',settings);}}))},
 {type:'separator'},
 {label:'随 Codex 启停',type:'checkbox',checked:settings.autoStart===true,click:item=>setStartup(item.checked)},
 {label:'恢复到屏幕内',click:restorePosition},
 {label:'重新连接',click:()=>{bridge.disconnect();bridge.tick();}},
 {label:'退出桌宠（本次 Codex）',click:()=>app.quit()}
 ]);}
async function start(){
 app.setAccessibilitySupportEnabled(true);
 settingsPath=path.join(app.getPath('userData'),'settings.json');fs.mkdirSync(path.dirname(settingsPath),{recursive:true});try{settings=JSON.parse(fs.readFileSync(settingsPath,'utf8'));}catch{}
 settings.scale=[0.8,1,1.2].includes(settings.scale)?settings.scale:1;
 settings.autoStart=settings.autoStart!==false;
 settings.motion=normalizeMode(settings.motion);settings.appearance=normalizeAppearance(settings.appearance);save();
 if(process.argv.includes('--quit')){app.quit();return;}
 if(process.argv.includes('--install-config')){setStartup(true,false);app.quit();return;}
 if(process.argv.includes('--disable-autostart')){setStartup(false);app.quit();return;}
 const upstream=JSON.parse(fs.readFileSync(path.join(ROOT,'assets/upstream-manifest.json'))),extra=JSON.parse(fs.readFileSync(path.join(ROOT,'assets/upstream-actions.json')));
 animations=[...new Map([...upstream.animations,...extra.animations].map(a=>[a.id,a])).values()];
 const area=screen.getPrimaryDisplay().workArea;
 let pos=settings.position??{x:area.x+area.width-WIDTH+80,y:area.y+area.height-HEIGHT};pos=clamp(pos);
 win=new BrowserWindow({width:WIDTH,height:HEIGHT,x:pos.x,y:pos.y,transparent:true,frame:false,thickFrame:false,hasShadow:false,resizable:false,minWidth:WIDTH,maxWidth:WIDTH,minHeight:HEIGHT,maxHeight:HEIGHT,skipTaskbar:true,alwaysOnTop:true,show:false,backgroundColor:'#00000000',title:'Codex 白龙桌宠',webPreferences:{preload:path.join(__dirname,'preload.cjs'),contextIsolation:true,nodeIntegration:false,sandbox:true,backgroundThrottling:false}});
 moveWindow(pos);
 win.webContents.on('console-message',(_event,_level,message)=>console.log('renderer:',message));
 win.webContents.on('did-fail-load',(_event,code,description)=>console.error('load-failed:',code,description));
 win.setAlwaysOnTop(true,'pop-up-menu');win.setIgnoreMouseEvents(false);ignore=false;
 win.setShape([{x:220,y:575,width:90,height:35}]);
 win.on('show',()=>win.setAlwaysOnTop(true,'pop-up-menu'));
 win.webContents.setWindowOpenHandler(()=>({action:'deny'}));win.webContents.on('will-navigate',e=>e.preventDefault());
 win.loadFile(path.join(__dirname,'index.html'));
 win.on('closed',()=>{win=null;app.quit();});
 win.on('blur',()=>{if(drag)finishDrag(drag.token,true);});
 bridge=new DesktopBridge();bridge.on('state',s=>{latest=s;send('state',s);if(s.connected&&!isolatedData){if(!win.isVisible())win.showInactive();if(!win.isAlwaysOnTop())win.setAlwaysOnTop(true,'pop-up-menu');}else if(!s.connected)win.hide();});bridge.on('issue',m=>send('connection-issue',m));bridge.start();
 setInterval(()=>{if(win&&drag&&!dragTesting)sampleDrag(screen.getCursorScreenPoint());},16);
 setInterval(()=>{if(latest.state==='complete'){latest=bridge.snapshot();send('state',latest);}},1000);
 if(process.argv.includes('--install'))setStartup(true);
 if(settings.autoStart&&!codexPid&&!isolatedData&&!process.argv.includes('--smoke'))startHost();
 setInterval(()=>{if(codexPid){if(!processAlive(codexPid))app.quit();}else if(!latest.connected&&Date.now()-startedAt>15000)app.quit();},1000);
 if(process.argv.includes('--smoke'))setTimeout(smoke,7500);
 if(process.argv.includes('--diagnose'))setTimeout(diagnose,6000);
 if(process.argv.includes('--motion-smoke'))setTimeout(motionSmoke,6000);
 if(dragTesting)setTimeout(dragSmoke,5000);
 if(process.argv.includes('--appearance-smoke'))setTimeout(appearanceSmoke,5000);
 if(process.argv.includes('--transition-smoke'))setTimeout(transitionSmoke,5000);
}
function clamp(p){const area=screen.getDisplayNearestPoint({x:Math.round(p.x+WIDTH/2),y:Math.round(p.y+HEIGHT-150)}).workArea;return constrainWindow(p,area);}
function moveWindow(bounds){win.setBounds({x:bounds.x,y:bounds.y,width:WIDTH,height:HEIGHT},false);}
function restorePosition(){finishDrag(drag?.token,true);const area=screen.getPrimaryDisplay().workArea;const p=constrainWindow({x:area.x+area.width-WIDTH-24,y:area.y+area.height-HEIGHT-12},area);moveWindow(p);settings.position={x:p.x,y:p.y};save();}
function beginDrag(token,cursor=screen.getCursorScreenPoint()){
 if(!win||!Number.isSafeInteger(token)||token<=0)return;
 if(drag)finishDrag(drag.token,true);
 drag=new DragSession(token,win.getBounds(),cursor);
}
function sampleDrag(cursor){
 if(!drag||!win)return null;
 const change=drag.sample(cursor,screen.getDisplayNearestPoint(cursor).workArea);
 if(!change)return null;
 if(change.started){win.setShape([{x:0,y:0,width:WIDTH,height:HEIGHT}]);send('drag-state',{token:drag.token,active:true});}
 if(change.changed)moveWindow(change.bounds);
 return change;
}
function finishDrag(token,cancelled=false,cursor=screen.getCursorScreenPoint()){
 if(!drag||drag.token!==token||!win)return{moved:false,stale:true};
 if(!cancelled)sampleDrag(cursor);
 const session=drag;drag=null;
 if(session.moved){const b=win.getBounds();settings.position={x:b.x,y:b.y};save();}
 send('drag-state',{token,active:false});
 return{moved:session.moved};
}
ipcMain.handle('boot',()=>{ready=true;return{animations,state:latest,settings,usage,version:app.getVersion()};});
ipcMain.handle('animation',(_e,id)=>{const a=animations.find(a=>a.id===id);if(!a)throw Error('Unknown animation');return 'data:image/webp;base64,'+fs.readFileSync(path.join(ROOT,'assets',a.sources.webp)).toString('base64');});
ipcMain.handle('usage',async()=>{if(usagePromise)return usagePromise;usagePromise=(async()=>{try{usage=await bridge.usage();usageError=null;return{usage,error:null};}catch(e){usageError=e.message;return{usage,error:usageError};}finally{usagePromise=null;}})();return usagePromise;});
ipcMain.on('shape',(_e,rects)=>{if(!win||drag?.moved||!Array.isArray(rects)||rects.length>1000)return;const safe=rects.filter(r=>[r.x,r.y,r.width,r.height].every(Number.isFinite)&&r.width>0&&r.height>0).map(r=>({x:Math.round(r.x),y:Math.round(r.y),width:Math.ceil(r.width),height:Math.ceil(r.height)}));if(safe.length)win.setShape(safe);});
ipcMain.on('drag-begin',(_e,token)=>beginDrag(token));
ipcMain.handle('drag-end',(_e,{token,cancelled}={})=>finishDrag(token,cancelled===true));
ipcMain.on('menu',()=>menu().popup({window:win}));
ipcMain.on('hide-panel',()=>{});
app.on('second-instance',(_event,argv)=>{if(argv.includes('--disable-autostart')){setStartup(false);app.quit();return;}if(argv.includes('--quit')){app.quit();return;}if(argv.includes('--install-config')){setStartup(true,false);return;}const nextOwner=ownerPid(argv);if(nextOwner){codexPid=nextOwner;return;}if(argv.includes('--restore-position')){restorePosition();return;}if(argv.includes('--diagnose')){diagnose();return;}if(argv.includes('--install'))setStartup(true);if(latest.connected){win?.showInactive();send('open-panel');}});
app.on('before-quit',()=>bridge?.stop());
app.on('window-all-closed',()=>app.quit());
async function smoke(){try{await win.webContents.executeJavaScript('window.petTest.openPanel()');await new Promise(r=>setTimeout(r,2500));const dir=process.env.PET_EVIDENCE_DIR||path.join(ROOT,'outputs');fs.mkdirSync(dir,{recursive:true});const page=await win.capturePage();fs.writeFileSync(path.join(dir,'desktop-pet-smoke.png'),page.toPNG());const ui=await win.webContents.executeJavaScript('window.petTest.inspect()');fs.writeFileSync(path.join(dir,'smoke.json'),JSON.stringify({state:latest,usage,ui,animationCount:animations.length,autoStart:app.getLoginItemSettings({path:hostPath(),args:[]}).openAtLogin,tray:false,codexPid,versions:process.versions},null,2));}catch(e){console.error(e);process.exitCode=1;}finally{app.quit();}}
async function diagnose(){try{const dir=process.env.PET_EVIDENCE_DIR||path.join(ROOT,'outputs');fs.mkdirSync(dir,{recursive:true});fs.writeFileSync(path.join(dir,'diagnose.json'),JSON.stringify({state:latest,ui:await win.webContents.executeJavaScript('window.petTest.inspect()'),visible:win.isVisible(),minimized:win.isMinimized(),alwaysOnTop:win.isAlwaysOnTop(),tray:false,codexPid,ignore,opacity:win.getOpacity(),bounds:win.getBounds()},null,2));fs.writeFileSync(path.join(dir,'diagnose.png'),(await win.capturePage()).toPNG());}catch(e){console.error(e);}}

async function motionSmoke(){try{const result=[];for(const value of Object.keys(MODES)){setMotion(value);await new Promise(r=>setTimeout(r,60));const ui=await win.webContents.executeJavaScript('window.petTest.probeIdleMotion('+JSON.stringify(value)+')');const saved=JSON.parse(fs.readFileSync(settingsPath,'utf8'));if(saved.motion!==value||ui.mode!==value)throw Error('Motion preference did not persist: '+value);if(value==='still'?(ui.firstFrame!==ui.startFrame||ui.lastFrame!==ui.startFrame):ui.lastFrame<=ui.firstFrame)throw Error('Unexpected frame behavior: '+value);if(['still','quiet','normal'].includes(value)&&ui.animation!=='待机呼吸休闲')throw Error('Expected breathing idle: '+value);result.push({...ui,savedMotion:saved.motion});}const dir=process.env.PET_EVIDENCE_DIR||path.join(ROOT,'outputs');fs.mkdirSync(dir,{recursive:true});fs.writeFileSync(path.join(dir,'motion-smoke.json'),JSON.stringify({result,menu:menu().items.find(i=>i.label==='待机动态').submenu.items.map(i=>({label:i.label,checked:i.checked})),tray:false,codexPid},null,2));}catch(e){console.error(e);process.exitCode=1;}finally{app.quit();}}

async function dragSmoke(){
 const result={displays:screen.getAllDisplays().map(d=>({scaleFactor:d.scaleFactor,workArea:d.workArea})),cases:[]};
 const assert=require('node:assert/strict');
 const wait=ms=>new Promise(r=>setTimeout(r,ms));
 // GetBounds can round one DIP at 125%/150%; it must never accumulate.
 const sizeIsFixed=()=>{const b=win.getBounds();assert.ok(Math.abs(b.width-WIDTH)<=1,JSON.stringify(b));assert.ok(Math.abs(b.height-HEIGHT)<=1,JSON.stringify(b));return b;};
 try{
  const area=screen.getPrimaryDisplay().workArea;
  moveWindow(constrainWindow({x:area.x+120,y:area.y+40},area));await wait(30);
  const before=sizeIsFixed(),cursor={x:before.x+WIDTH/2,y:before.y+400};
  beginDrag(101,cursor);let writes=0;
  for(let i=0;i<128;i++){if(sampleDrag(cursor)?.changed)writes++;await wait(16);}
  assert.deepEqual(win.getBounds(),before);assert.equal(writes,0);
  assert.deepEqual(finishDrag(101,false,cursor),{moved:false});
  result.cases.push({case:'hold-still',samples:128,writes,before,after:win.getBounds()});
  beginDrag(102,cursor);
  let last=cursor;
  for(let i=0;i<240;i++){
   last={x:cursor.x+(i%2?28:12),y:cursor.y+(i%2?18:8)};
   sampleDrag(last);sizeIsFixed();
   if(i%12===0)await wait(10);
  }
  const moved=sizeIsFixed();assert.deepEqual(finishDrag(102,false,last),{moved:true});
  result.cases.push({case:'repeated-drag',samples:240,bounds:moved});
  for(const point of [{x:-1000000,y:-1000000},{x:1000000,y:1000000}]){
   beginDrag(103,cursor);sampleDrag(point);
   const b=sizeIsFixed(),work=screen.getDisplayNearestPoint(point).workArea;
   assert.ok(b.y+b.height<=work.y+work.height+1);
   assert.ok(b.y+b.height-260>=work.y);
   const finish=finishDrag(103,true,point);assert.equal(finish.moved,true);
   assert.equal(drag,null);
   result.cases.push({case:'edge-and-cancel',point,bounds:b,workArea:work});
  }
  beginDrag(104,cursor);assert.equal(finishDrag(103,true,cursor).stale,true);assert.equal(drag.token,104);finishDrag(104,true,cursor);
  result.cases.push({case:'stale-end-ignored',passed:true});
  restorePosition();sizeIsFixed();result.passed=true;
 }catch(e){result.passed=false;result.error=e.stack;console.error(e);process.exitCode=1;}
 finally{
  const dir=process.env.PET_EVIDENCE_DIR||path.join(ROOT,'outputs');fs.mkdirSync(dir,{recursive:true});fs.writeFileSync(path.join(dir,'drag-smoke.json'),JSON.stringify(result,null,2));app.quit();
 }
}

async function appearanceSmoke(){const dir=process.env.PET_EVIDENCE_DIR||path.join(ROOT,'outputs');fs.mkdirSync(dir,{recursive:true});const result=[];try{for(const value of Object.keys(STYLES)){setAppearance(value);const ui=await win.webContents.executeJavaScript('window.petTest.probeAppearance('+JSON.stringify(value)+')');const image=ui.image;delete ui.image;fs.writeFileSync(path.join(dir,value+'.png'),Buffer.from(image.split(',')[1],'base64'));result.push(ui);}fs.writeFileSync(path.join(dir,'appearance-smoke.json'),JSON.stringify({result,tray:false},null,2));}catch(e){console.error(e);process.exitCode=1;}finally{app.quit();}}

async function transitionSmoke(){const dir=process.env.PET_EVIDENCE_DIR||path.join(ROOT,'outputs');fs.mkdirSync(dir,{recursive:true});try{const result=await win.webContents.executeJavaScript('window.petTest.probeColorTransitions()');for(const [name,image]of Object.entries(result.images))fs.writeFileSync(path.join(dir,name+'.png'),Buffer.from(image.split(',')[1],'base64'));delete result.images;fs.writeFileSync(path.join(dir,'transition-smoke.json'),JSON.stringify(result,null,2));}catch(e){fs.writeFileSync(path.join(dir,'transition-smoke.json'),JSON.stringify({passed:false,error:String(e)},null,2));console.error(e);process.exitCode=1;}finally{app.quit();}}
