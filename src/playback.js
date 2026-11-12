(function(root,factory){const api=factory();if(typeof module==='object'&&module.exports)module.exports=api;else root.petPlayback=api;})(globalThis,function(){
 function clipRange(animation){
  const s=animation.spritesheet,count=Math.max(1,s.frameCount),fps=Math.max(1,s.fps);
  // The supplied videos begin with a pale reference-image fade. Start in
  // the stable-color portion; keep at least 80% of every animation.
  const first=Math.min(Math.round(fps*2),Math.floor(count*.2));
  return{first,last:count-1,count:count-first,fps,durationMs:(count-first)/fps*1000};
 }
 function frameAt(animation,elapsedMs,frozen=false){
  const r=clipRange(animation);
  return frozen?r.first:Math.min(r.last,r.first+Math.max(0,Math.floor(elapsedMs*r.fps/1000)));
 }
 return{clipRange,frameAt};
});
