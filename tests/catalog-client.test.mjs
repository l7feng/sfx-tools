import test from 'node:test';
import assert from 'node:assert/strict';
import {filterPage,resumable,formatBytes,displayPath} from '../src/catalog-client.ts';
const items=[{id:'stable-1',name:'雨夜.wav',path:'D:/素材/森林/雨夜.wav',format:'WAV',duration:null,rate:48000,channels:2},{id:'stable-2',name:'Rain.mp3',path:'D:/素材/城市/Rain.mp3',format:'MP3',duration:1.1,rate:44100,channels:1}];
test('真实路径、名称和格式组合过滤，不改写未知元数据',()=>{
 const selected=filterPage(items,'森林 雨','WAV');
 assert.deepEqual(selected.map(a=>a.id),['stable-1']);
 assert.equal(selected[0].duration,null);
 assert.equal(filterPage(items,'rain','MP3')[0].id,'stable-2');
 assert.equal(filterPage(items,'城市','WAV').length,0);
});
test('仅已结束且未完成的任务可继续，运行任务不能重复恢复',()=>{
 for(const status of ['cancelled','interrupted','failed','completed_with_errors'])assert.equal(resumable({status}),true);
 for(const status of ['queued','running','cancelling','completed'])assert.equal(resumable({status}),false);
});
test('文件大小格式不把小文件显示成零MB',()=>{
 assert.equal(formatBytes(44),'44 B');
 assert.equal(formatBytes(1024),'1.0 KB');
 assert.equal(formatBytes(1048576),'1.0 MB');
});
test('显示路径隐藏Windows扩展前缀但保留UNC共享路径',()=>{
 assert.equal(displayPath(String.raw`\\?\D:\素材\雨`),String.raw`D:\素材\雨`);
 assert.equal(displayPath(String.raw`\\?\UNC\server\share\素材`),String.raw`\\server\share\素材`);
 assert.equal(displayPath('D:/素材/雨'),'D:/素材/雨');
});
