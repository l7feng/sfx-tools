import test from 'node:test';
import assert from 'node:assert/strict';
import {searchFolder} from '../src/search-scope.ts';
test('目录中默认搜索全库，显式切回目录，清空查询恢复原位置',()=>{
 const folder='D:\\audio\\forest';
 assert.equal(searchFolder(folder,' rain ','global'),null);
 assert.equal(searchFolder(folder,'rain','folder'),folder);
 assert.equal(searchFolder(folder,'  ','global'),folder);
 assert.equal(searchFolder(null,'rain','folder'),null);
});
