import test from 'node:test';
import assert from 'node:assert/strict';
import {toolPages,findTools,toolArea} from '../src/tool-navigation.ts';

test('工具总览列出全部实际入口，叠层筛选清除后能重新找到其他工具',()=>{
 assert.equal(new Set(toolPages.map(tool=>tool.id)).size,11);
 assert(findTools('叠层').some(tool=>tool.id==='layer'));
 assert.deepEqual(findTools('  '),toolPages);
 for(const id of ['acoustic','ucs','duplicates','convert','plugins','account'])assert(findTools('').some(tool=>tool.id===id));
});
test('工具搜索支持多词、英文大小写和清晰的零结果',()=>{
 assert(findTools('pro tools').some(tool=>tool.id==='smart'));
 assert(findTools('SOXR 重采样').some(tool=>tool.id==='convert'));
 assert(findTools('离线 模型').some(tool=>tool.id==='acoustic'));
 assert.equal(findTools('不存在的工具名123').length,0);
});

test('音频工具页只保留制作功能，账号模型管理移到库内',()=>{assert.deepEqual(toolPages.filter(t=>toolArea(t.id)==='production').map(t=>t.id),['convert','plugins','pt']);for(const id of ['account','acoustic','ucs','duplicates'])assert.equal(toolArea(id),'library');});
