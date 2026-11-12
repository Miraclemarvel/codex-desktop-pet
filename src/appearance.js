(function(root,factory){const api=factory();if(typeof module==='object'&&module.exports)module.exports=api;else root.petAppearance=api;})(globalThis,function(){
 const STYLES=Object.freeze({
  original:{label:'原色',filter:'none'},
  clear:{label:'清晰（默认）',filter:'contrast(1.14) saturate(1.12)'},
  vivid:{label:'鲜明',filter:'contrast(1.23) saturate(1.23)'}
 });
 function normalize(value){return Object.hasOwn(STYLES,value)?value:'clear';}
 function resolution(cssWidth,dpr){const width=Math.max(1,Math.round(cssWidth*dpr));return{width,height:Math.max(1,Math.round(cssWidth*360/640*dpr))};}
 function sheetSize(s,canvasWidth){const step=Math.min(40,Math.max(20,Math.round(canvasWidth/16)));return{width:step*16*s.columns,height:step*9*s.rows};}
 return{STYLES,normalize,resolution,sheetSize};
});
