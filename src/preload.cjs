const {contextBridge,ipcRenderer}=require('electron');
const listen=(event,handler)=>{const fn=(_e,data)=>handler(data);ipcRenderer.on(event,fn);return()=>ipcRenderer.removeListener(event,fn);};
contextBridge.exposeInMainWorld('pet',{
 boot:()=>ipcRenderer.invoke('boot'),animation:id=>ipcRenderer.invoke('animation',id),usage:()=>ipcRenderer.invoke('usage'),
 shape:rects=>ipcRenderer.send('shape',rects),dragBegin:token=>ipcRenderer.send('drag-begin',token),dragEnd:(token,cancelled)=>ipcRenderer.invoke('drag-end',{token,cancelled}),menu:()=>ipcRenderer.send('menu'),
 onDragState:fn=>listen('drag-state',fn),
 onState:fn=>listen('state',fn),onCursor:fn=>listen('cursor',fn),onSettings:fn=>listen('settings',fn),onPreview:fn=>listen('preview',fn),onPause:fn=>listen('pause',fn),onOpenPanel:fn=>listen('open-panel',fn),onTogglePanel:fn=>listen('toggle-panel',fn)
});
