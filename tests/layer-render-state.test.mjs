import test from 'node:test';
import assert from 'node:assert/strict';
import {renderRecipe,renderRecipeKey,renderMatches,renderReady,renderIsActive,renderProgress} from '../src/layer-render-state.ts';
const clips=[{assetId:'a',offset:0,gain:.5,duration:2},{assetId:'a',offset:3,gain:1,duration:2}];
const completed={job_id:'job',status:'completed',progress:1,rendered_seconds:5,duration:5,output_path:'D:/cache/job.wav',bytes:960044,peak:.6,clipped_samples:0,error:null,clips};
test('渲染快照比对真实编排四字段，重复素材与顺序不被去重',()=>{
 assert.deepEqual(renderRecipe([{...clips[0],path:'untrusted',name:'name'}]),[clips[0]]);
 assert.notEqual(renderRecipeKey(clips),renderRecipeKey([clips[0]]));assert.equal(renderMatches(completed,clips),true);
 for(const patch of [{assetId:'b'},{offset:1},{gain:.7},{duration:1}])assert.equal(renderMatches(completed,[{...clips[0],...patch},clips[1]]),false);
 assert.equal(renderMatches(completed,[clips[1],clips[0]]),false);assert.equal(renderMatches(completed,[]),false);
});
test('仅已完成且匹配当前编排及有效来源的缓存可试听导出',()=>{
 assert.equal(renderReady(completed,clips),true);
 for(const patch of [{status:'running'},{status:'failed'},{status:'cancelled'},{job_id:null},{output_path:null},{source_stale:true}])assert.equal(renderReady({...completed,...patch},clips),false);
 assert.equal(renderReady({...completed,clips:[]},[]),false);assert.equal(renderReady(null,clips),false);
});
test('后台活动状态含取消中，进度异常不产生越界数值',()=>{
 for(const status of ['queued','preparing','running','cancelling'])assert.equal(renderIsActive({status}),true);
 for(const status of ['idle','completed','failed','cancelled'])assert.equal(renderIsActive({status}),false);
 assert.equal(renderIsActive(null),false);assert.equal(renderProgress(NaN),0);assert.equal(renderProgress(Infinity),0);assert.equal(renderProgress(-1),0);assert.equal(renderProgress(2),1);assert.equal(renderProgress(.4),.4);
});
