import test from 'node:test';
import assert from 'node:assert/strict';
import {safeLayout,selectIds,visibleColumns,moveColumn,toggleColumn,resetColumns,columnDefinitions} from '../src/workbench-state.ts';
test('损坏或过期布局限制宽度并保留合法折叠偏好',()=>{
 const value=safeLayout({sidebar:-500,inspector:Infinity,wave:900,columns:[null,1],details:false});
 assert.equal(value.sidebar,150);assert.equal(value.inspector,260);assert.equal(value.wave,300);assert.equal(value.columns[0],280);assert.equal(value.columns[1],120);assert.equal(value.details,false);assert.deepEqual(safeLayout(null),safeLayout({}));
});
test('多选支持反选、双向范围与Ctrl Shift累加且不选入其他页',()=>{
 const ids=['a','b','c','d'];assert.deepEqual(selectIds(ids,['a'],'a',null,true,false),[]);assert.deepEqual(selectIds(ids,[],'a','c',false,true),['a','b','c']);assert.deepEqual(selectIds(ids,['d'],'b','a',true,true),['d','a','b']);assert.deepEqual(selectIds(ids,['a'],'other',null,false,false),['a']);
});

test('旧五列布局迁移保留尺寸并插入波形列，四侧折叠状态可恢复',()=>{
 const value=safeLayout({columns:[420,80,90,70,100],topCollapsed:true,leftCollapsed:true,bottomCollapsed:true,details:false});
 assert.deepEqual(value.columns,[420,210,80,90,70,100]);assert.equal(value.topCollapsed,true);assert.equal(value.leftCollapsed,true);assert.equal(value.bottomCollapsed,true);assert.equal(value.details,false);
 assert.equal(safeLayout({topCollapsed:'true'}).topCollapsed,false);
});

test('列偏好清洗重复和未知键，兼容旧布局并默认显示四列',()=>{
 const old=safeLayout({columns:[420,80,90,70,100]});
 assert.deepEqual(visibleColumns(old).map(column=>column.id),['name','wave','duration','format']);
 const broken=safeLayout({columnOrder:['format','format','obsolete',null,'name'],hiddenColumns:['obsolete','wave','wave']});
 assert.deepEqual(broken.columnOrder,['format','name','wave','duration','rate','status']);assert.deepEqual(broken.hiddenColumns,['wave']);
 assert.deepEqual(safeLayout({columnOrder:'bad',hiddenColumns:'bad'}).columnOrder,columnDefinitions.map(column=>column.id));
});
test('调序与隐藏不改变列身份对应的宽度，并至少保留一列',()=>{
 const initial=safeLayout({columns:[410,220,85,95,75,105],hiddenColumns:[]});
 const moved=moveColumn(initial,'format',-1);assert.deepEqual(moved.columnOrder,['name','wave','duration','format','rate','status']);
 assert.deepEqual(moved.columns,initial.columns);assert.equal(visibleColumns(moved).find(column=>column.id==='format').widthIndex,4);
 let value=moved;for(const column of columnDefinitions)value=toggleColumn(value,column.id);
 assert.equal(visibleColumns(value).length,1);assert.equal(visibleColumns(toggleColumn(value,visibleColumns(value)[0].id)).length,1);
 const repaired=safeLayout({hiddenColumns:columnDefinitions.map(column=>column.id)});assert.deepEqual(visibleColumns(repaired).map(column=>column.id),['name']);
 const roundtrip=safeLayout(JSON.parse(JSON.stringify(value)));assert.deepEqual(roundtrip,value);
 const reset=resetColumns({...value,sidebar:340,leftCollapsed:true});assert.equal(reset.sidebar,340);assert.equal(reset.leftCollapsed,true);assert.deepEqual(visibleColumns(reset).map(column=>column.id),['name','wave','duration','format']);
});
