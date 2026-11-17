import {test} from 'node:test';
import assert from 'node:assert/strict';
import {safeLayout,defaultLayout,clamp} from '../src/workbench-state.ts';

test('空存档等于默认布局',()=>{
 const layout=safeLayout({});
 assert.deepEqual(layout,defaultLayout);
});

test('越界的侧栏/检查器/波形会被夹回范围，而不是按原值渲染',()=>{
 const layout=safeLayout({sidebar:900,inspector:10,wave:999});
 assert.equal(layout.sidebar,420);
 assert.equal(layout.inspector,210);
 assert.equal(layout.wave,300);
});

test('NaN / 字符串等脏值回落默认值，不产生 NaN 宽度',()=>{
 const layout=safeLayout({sidebar:'wide',inspector:Number.NaN,wave:null});
 assert.equal(layout.sidebar,220);
 assert.equal(layout.inspector,260);
 assert.equal(layout.wave,110);
 for(const width of layout.columns) assert.ok(Number.isFinite(width),`列宽不能是 NaN：${width}`);
});

test('列宽逐项夹取：太窄抬到下限、太宽压到上限',()=>{
 const layout=safeLayout({columns:[1,9999,50,50,50,50,50,50,50,50,50,50]});
 assert.equal(layout.columns[0],24,'首列下限 24');
 assert.equal(layout.columns[1],600,'名称列上限 600');
 assert.equal(layout.columns[2],50,'其余列 36–300 之间保持原值');
});

test('旧版列序（含已删除的状态列）整体回到新默认列序',()=>{
 const layout=safeLayout({columnOrder:['name','wave','duration','rate','format','status']});
 assert.deepEqual(layout.columnOrder,defaultLayout.columnOrder);
});

test('用户自定义的新列序被保留，缺的列补到末尾',()=>{
 const layout=safeLayout({columnOrder:['playlist','name','duration']});
 assert.deepEqual(layout.columnOrder.slice(0,3),['playlist','name','duration']);
 assert.equal(new Set(layout.columnOrder).size,defaultLayout.columnOrder.length,'补全后不应有重复列');
});

test('所有列都被隐藏时至少留下名称列',()=>{
 const all=defaultLayout.columnOrder;
 const layout=safeLayout({hiddenColumns:all});
 assert.ok(!layout.hiddenColumns.includes('name'),'名称列必须可见');
});

test('clamp 边界',()=>{
 assert.equal(clamp(5,1,3),3);
 assert.equal(clamp(-1,1,3),1);
});
