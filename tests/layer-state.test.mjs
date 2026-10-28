import test from 'node:test';
import assert from 'node:assert/strict';
import {appendLayer,safeLayer,updateLayer,pruneLayerState,refreshLayerMetadata,LAYER_LIMIT} from '../src/layer-state.ts';
const asset=(id,duration=2)=>({id,name:id,path:`D:/audio/${id}.wav`,duration,status:'ready'});
let sequence=0;const id=()=>`clip-${++sequence}`;
test('叠放重复素材保持独立片段，散布从当前编排尾部连续追加',()=>{
 const first=appendLayer([], [asset('a'),asset('a',3)],false,id);assert.equal(first.added,2);assert.notEqual(first.clips[0].id,first.clips[1].id);assert.deepEqual(first.clips.map(c=>c.offset),[0,0]);
 const next=appendLayer(first.clips,[asset('b',4),asset('c',5)],true,id);assert.deepEqual(next.clips.map(c=>c.offset),[0,0,3,7]);assert.equal(next.clips.at(-1).offset+next.clips.at(-1).duration,12);
});
test('损坏配置和不支持时长不伪造片段，限制数量和编排总长',()=>{
 assert.deepEqual(safeLayer(null),[]);assert.deepEqual(safeLayer([null,{},asset('x')]),[]);
 const result=appendLayer([], [asset('bad',null),asset('negative',-1),asset('long',3601),asset('good',3600),asset('overflow',1)],true,id);assert.equal(result.added,1);assert.equal(result.skipped,4);
 const many=appendLayer([],Array.from({length:250},(_,i)=>asset(String(i))),false,id);assert.equal(many.clips.length,LAYER_LIMIT);assert.equal(many.skipped,50);
});
test('编辑时间与增益约束有限范围，配置往返保留可编辑身份',()=>{
 const clip=appendLayer([],[asset('a',20)],false,id).clips[0];
 const edited=updateLayer([clip],clip.id,{offset:99999,gain:-1});assert.equal(edited[0].offset,3580);assert.equal(edited[0].gain,0);
 assert.deepEqual(safeLayer(JSON.parse(JSON.stringify(edited))),edited);
 const invalid=updateLayer(edited,clip.id,{offset:NaN,gain:Infinity});assert.equal(invalid[0].offset,0);assert.equal(invalid[0].gain,1);
 assert.equal(safeLayer([clip,clip]).length,1);
});

test('出库清除全部同ID片段，Windows目录大小写、扩展前缀和边界正确',()=>{
 const clips=appendLayer([],[{...asset('a'),path:'D:/Foo/a.wav'},{...asset('b'),path:'d:/foo/deep/b.wav'},{...asset('c'),path:'D:/foo2/c.wav'},asset('a')],false,id).clips;
 assert.equal(pruneLayerState(clips,{ids:['a']}).length,2);
 const remaining=pruneLayerState(clips,{folder:String.raw`\\?\D:\FOO`});assert.deepEqual(remaining.map(c=>c.assetId),['c','a']);
 assert.equal(pruneLayerState(clips,{folder:''}).length,clips.length);
 assert.equal(pruneLayerState(clips,{folder:'D:/'}).length,0);
 const network=[{...clips[0],path:String.raw`\\?\UNC\server\Share\foo\a.wav`},{...clips[1],path:String.raw`\\server\share\foo2\b.wav`}];
 assert.equal(pruneLayerState(network,{folder:String.raw`\\SERVER\SHARE\foo`}).length,1);
});
test('重命名移动刷新叠层显示元数据，保留稳定ID、编排起点和增益',()=>{
 const original=updateLayer(appendLayer([],[asset('a'),asset('a')],false,id).clips,'no-change',{});
 original[0].offset=8;original[0].gain=.6;
 const refreshed=refreshLayerMetadata(original,[{id:'a',name:'new.wav',path:'D:/new/new.wav'}]);
 assert.equal(refreshed.length,2);for(const clip of refreshed){assert.equal(clip.name,'new.wav');assert.equal(clip.path,'D:/new/new.wav');}
 assert.equal(refreshed[0].id,original[0].id);assert.equal(refreshed[0].offset,8);assert.equal(refreshed[0].gain,.6);assert.equal(original[0].name,'a');
});
