import test from 'node:test';
import assert from 'node:assert/strict';
import {safePreferences,defaultPreferences} from '../src/preferences.ts';
test('设置读取拒绝损坏值并约束音量，保留明确的关闭布局记忆',()=>{
 assert.deepEqual(safePreferences(null),defaultPreferences);
 assert.deepEqual(safePreferences({startPage:'bad',volume:NaN,waveColor:3,rememberLayout:'false'}),defaultPreferences);
 const value=safePreferences({volume:150,startPage:'music',waveColor:'mint',rememberLayout:false});
 assert.equal(value.volume,100);assert.equal(value.startPage,'music');assert.equal(value.rememberLayout,false);assert.equal(value.waveColor,'blue');assert.equal(safePreferences({volume:-3}).volume,0);
});
test('含元数据默认开启，旧存档缺字段不被静默关掉，显式关闭则保留',()=>{
 assert.equal(defaultPreferences.metadataSearch,true);
 assert.equal(safePreferences({}).metadataSearch,true);
 assert.equal(safePreferences({metadataSearch:false}).metadataSearch,false);
});
test('筛选模板缺省回落默认值，自定义值去重去空并忽略未知维度',()=>{
 assert.deepEqual(safePreferences(null).filterTemplates,defaultPreferences.filterTemplates);
 const custom=safePreferences({filterTemplates:{'情绪':[' 紧张 ','紧张','','温暖'],'流派':[],'未知维度':['不应出现']}});
 assert.deepEqual(custom.filterTemplates['情绪'],['紧张','温暖']);
 assert.deepEqual(custom.filterTemplates['流派'],[]);
 assert.equal(custom.filterTemplates['未知维度'],undefined);
 assert.deepEqual(custom.filterTemplates['乐器'],defaultPreferences.filterTemplates['乐器']);
});
