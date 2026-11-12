const {test}=require('node:test'),assert=require('node:assert/strict');
const {clipRange,frameAt}=require('../src/playback.js');
const clip={spritesheet:{frameCount:120,fps:12}};
test('playback and static idle begin after the pale introductory frames',()=>{
 assert.deepEqual(clipRange(clip),{first:24,last:119,count:96,fps:12,durationMs:8000});
 assert.equal(frameAt(clip,0),24);assert.equal(frameAt(clip,1000),36);
 assert.equal(frameAt(clip,999999,true),24);
});
test('an unfinished switch holds the ending frame instead of wrapping to frame zero',()=>{
 assert.equal(frameAt(clip,8000),119);assert.equal(frameAt(clip,20000),119);
 assert.equal(frameAt(clip,-1),24);
});
test('short clips retain at least eighty percent of their frames',()=>{
 for(const count of [1,5,12,30,60,120]){
  const a={spritesheet:{frameCount:count,fps:12}},r=clipRange(a);
  assert.ok(r.count>=count*.8);assert.ok(r.first>=0);assert.ok(r.last>=r.first);
 }
});
