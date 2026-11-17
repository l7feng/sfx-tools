import {test} from 'node:test';
import assert from 'node:assert/strict';
import {treeClick} from '../src/tree-click.ts';

test('叶子目录点击只选中，不做展开',()=>{
 assert.equal(treeClick({hasChild:false,open:false,selected:false}),'select');
 assert.equal(treeClick({hasChild:false,open:true,selected:true}),'select');
});

test('点没选中过的折叠目录：选中并展开',()=>{
 assert.equal(treeClick({hasChild:true,open:false,selected:false}),'select-open');
});

test('点没选中过的已展开目录：只选中，不把它收回去',()=>{
 assert.equal(treeClick({hasChild:true,open:true,selected:false}),'select');
});

test('点已选中的目录：只切换展开状态',()=>{
 assert.equal(treeClick({hasChild:true,open:true,selected:true}),'toggle');
 assert.equal(treeClick({hasChild:true,open:false,selected:true}),'toggle');
});
