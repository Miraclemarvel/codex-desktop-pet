const WINDOW_SIZE=Object.freeze({width:520,height:620});
function constrainWindow(point,area,size=WINDOW_SIZE){
 const axis=(value,min,max)=>Math.round(max<min?(min+max)/2:Math.max(min,Math.min(max,Number.isFinite(value)?value:min)));
 return{x:axis(point.x,area.x-100,area.x+area.width-size.width+100),y:axis(point.y,area.y-200,area.y+area.height-size.height),width:size.width,height:size.height};
}
class DragSession{
 constructor(token,bounds,cursor,size=WINDOW_SIZE){this.token=token;this.origin={x:bounds.x,y:bounds.y};this.cursor={...cursor};this.size={...size};this.last={...this.origin};this.moved=false;}
 sample(cursor,area){
  const dx=cursor.x-this.cursor.x,dy=cursor.y-this.cursor.y;
  if(!this.moved&&Math.hypot(dx,dy)<=5)return null;
  const started=!this.moved;this.moved=true;
  const bounds=constrainWindow({x:this.origin.x+dx,y:this.origin.y+dy},area,this.size);
  const changed=bounds.x!==this.last.x||bounds.y!==this.last.y;
  if(!started&&!changed)return null;
  this.last={x:bounds.x,y:bounds.y};
  return{started,changed,bounds};
 }
}
module.exports={WINDOW_SIZE,constrainWindow,DragSession};
