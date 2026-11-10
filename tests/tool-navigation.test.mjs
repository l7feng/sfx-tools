import test from 'node:test';
import assert from 'node:assert/strict';
import {toolPages,findTools,toolArea} from '../src/tool-navigation.ts';

test('工具总览列出全部实际入口，叠层筛选清除后能重新找到其他工具',()=>{
 assert.equal(new Set(toolPages.map(tool=>tool.id)).size,9);
 assert(findTools('叠层').some(tool=>tool.id==='layer'));
 assert.deepEqual(findTools('  '),toolPages);
 for(const id of ['ucs','duplicates','convert','plugins','account'])assert(findTools('').some(tool=>tool.id===id));
});
test('工具搜索支持多词、英文大小写和清晰的零结果',()=>{
 assert(findTools('pro tools').some(tool=>tool.id==='pt'));
 assert(findTools('SOXR 重采样').some(tool=>tool.id==='convert'));
 assert(findTools('波形 合并').some(tool=>tool.id==='duplicates'));
 assert.equal(findTools('不存在的工具名123').length,0);
});

test('工具窗口只保留制作功能，其余归管理窗口',()=>{assert.deepEqual(toolPages.filter(t=>toolArea(t.id)==='production').map(t=>t.id),['convert','plugins','pt']);for(const id of ['account','ucs','duplicates','layer','maintenance','info'])assert.equal(toolArea(id),'library');});
