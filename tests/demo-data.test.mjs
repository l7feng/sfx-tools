import test from 'node:test';
import assert from 'node:assert/strict';
import {assets,filterAssets,timeLabel} from '../src/demo-data.ts';
test('分类、格式和多个搜索词共同生效',()=>{
 assert.deepEqual(filterAssets(assets,'森林 雨','自然环境','WAV',new Set()).map(a=>a.id),[1]);
 assert.equal(filterAssets(assets,'雨','自然环境','MP3',new Set()).length,0);
});
test('搜索英文文件名不区分大小写，空格不改变结果',()=>{
 assert.deepEqual(filterAssets(assets,'  pineforest  ','全部素材','全部格式',new Set()).map(a=>a.id),[1]);
});
test('收藏范围与格式交叉过滤，取消收藏即时影响结果',()=>{
 const favorites=new Set([1,5,9]);
 assert.deepEqual(filterAssets(assets,'','我的收藏','AIFF',favorites).map(a=>a.id),[5]);
 favorites.delete(5);
 assert.equal(filterAssets(assets,'','我的收藏','AIFF',favorites).length,0);
});
test('空结果不会改写演示源集合',()=>{
 const before=JSON.stringify(assets);
 assert.equal(filterAssets(assets,'不存在的素材','全部素材','全部格式',new Set()).length,0);
 assert.equal(JSON.stringify(assets),before);
});
test('时间读数正确跨越分钟边界',()=>{
 assert.equal(timeLabel(59.99),'01:00.0');
 assert.equal(timeLabel(68.2),'01:08.2');
 assert.equal(timeLabel(0),'00:00.0');
});
