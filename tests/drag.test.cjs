const {test}=require('node:test'),assert=require('node:assert/strict');
const {WINDOW_SIZE,constrainWindow,DragSession}=require('../src/drag.cjs');
const area={x:0,y:0,width:1536,height:824};
test('pressing and holding a stationary pet never writes window geometry',()=>{
 const session=new DragSession(1,{x:700,y:70,width:520,height:620},{x:960,y:520});
 for(let i=0;i<1000;i++)assert.equal(session.sample({x:960,y:520},area),null);
 assert.equal(session.moved,false);
 assert.equal(session.sample({x:963,y:524},area),null);
});
test('dragging uses one native coordinate system and always supplies a fixed size',()=>{
 const session=new DragSession(1,{x:700,y:70,width:620,height:1093},{x:960,y:520});
 const sample=session.sample({x:985,y:500},area);
 assert.deepEqual(sample,{started:true,changed:true,bounds:{x:725,y:50,...WINDOW_SIZE}});
 for(let i=0;i<1000;i++)assert.equal(session.sample({x:985,y:500},area),null);
 assert.deepEqual(session.sample({x:980,y:510},area).bounds,{x:720,y:60,...WINDOW_SIZE});
});
test('a drag cannot accumulate position drift when native dimensions change',()=>{
 const session=new DragSession(2,{x:700,y:70,width:520,height:620},{x:960,y:520});
 for(let i=0;i<300;i++){
  const x=i%2?980:990;
  const change=session.sample({x,y:530},area);
  assert.deepEqual(change.bounds,{x:700+x-960,y:80,...WINDOW_SIZE});
 }
});
test('screen edges and monitors with negative coordinates keep the pet in view',()=>{
 const negative={x:-1536,y:-864,width:1536,height:824};
 for(const a of [area,negative]){
  const bottom=constrainWindow({x:1e6,y:1e6},a);
  assert.equal(bottom.y+bottom.height,a.y+a.height);
  const top=constrainWindow({x:-1e6,y:-1e6},a);
  assert.ok(top.y+top.height-260>=a.y);
  assert.ok(top.x+top.width/2>=a.x);
  assert.ok(bottom.x+bottom.width/2<=a.x+a.width);
 }
});
