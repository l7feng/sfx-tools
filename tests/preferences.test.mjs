import test from 'node:test';
import assert from 'node:assert/strict';
import {safePreferences,defaultPreferences} from '../src/preferences.ts';
test('设置读取拒绝损坏值并约束音量，保留明确的关闭布局记忆',()=>{
 assert.deepEqual(safePreferences(null),defaultPreferences);
 assert.deepEqual(safePreferences({density:'bad',startPage:'bad',volume:NaN,waveColor:3,rememberLayout:'false'}),defaultPreferences);
 const value=safePreferences({volume:150,startPage:'music',density:'comfortable',waveColor:'mint',rememberLayout:false});
 assert.equal(value.volume,100);assert.equal(value.startPage,'music');assert.equal(value.rememberLayout,false);assert.equal(value.density,'comfortable');assert.equal(value.waveColor,'blue');assert.equal(safePreferences({volume:-3}).volume,0);
});
test('含元数据默认开启，旧存档缺字段不被静默关掉，显式关闭则保留',()=>{
 assert.equal(defaultPreferences.metadataSearch,true);
 assert.equal(safePreferences({}).metadataSearch,true);
 assert.equal(safePreferences({metadataSearch:false}).metadataSearch,false);
});
