import {test} from 'node:test';
import assert from 'node:assert/strict';
import {displayPath,filterPage,formatBytes} from '../src/catalog-client.ts';
import type {CatalogAsset} from '../src/catalog-client.ts';

const asset=(name:string,path:string,format='WAV'):CatalogAsset=>({id:name,path,name,format,size:1,modified:'2020-01-01',rate:null,channels:null,duration:null,status:'ready',error:null,favorite:false,tags:[]});

test('displayPath 去掉 Windows 长路径前缀',()=>{
 assert.equal(displayPath('\\\\?\\D:\\Sounds\\a.wav'),'D:\\Sounds\\a.wav');
 assert.equal(displayPath('\\\\?\\UNC\\nas\\share\\a.wav'),'\\\\nas\\share\\a.wav');
 assert.equal(displayPath('D:\\Sounds\\a.wav'),'D:\\Sounds\\a.wav');
});

test('filterPage：多关键词需同时命中，忽略大小写',()=>{
 const items=[asset('Rain Soft.wav','D:\\s\\rain\\soft.wav'),asset('Rain Hard.wav','D:\\s\\rain\\hard.wav'),asset('Wind.wav','D:\\s\\wind.wav')];
 assert.deepEqual(filterPage(items,'rain soft','全部格式').map(item=>item.name),['Rain Soft.wav']);
 assert.equal(filterPage(items,'RAIN','全部格式').length,2);
 assert.equal(filterPage(items,'rain wind','全部格式').length,0,'两个词必须同时出现在同一文件');
});

test('filterPage：格式筛选按大写比较，「全部格式」放行',()=>{
 const items=[asset('a.wav','D:\\s\\a.wav','WAV'),asset('b.mp3','D:\\s\\b.mp3','MP3')];
 assert.deepEqual(filterPage(items,'','WAV').map(item=>item.name),['a.wav']);
 assert.equal(filterPage(items,'','全部格式').length,2);
});

test('formatBytes 三段进制',()=>{
 assert.equal(formatBytes(1023),'1023 B');
 assert.equal(formatBytes(1024),'1.0 KB');
 assert.equal(formatBytes(1048576),'1.0 MB');
});
