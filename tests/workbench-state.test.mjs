import test from 'node:test';
import assert from 'node:assert/strict';
import {safeLayout,selectIds,visibleColumns,moveColumn,toggleColumn,resetColumns,columnDefinitions,defaultLayout} from '../src/workbench-state.ts';
test('损坏或过期布局限制宽度并保留合法折叠偏好',()=>{
 const value=safeLayout({sidebar:-500,inspector:Infinity,wave:900,columns:[null,1],details:false});
 assert.equal(value.sidebar,150);assert.equal(value.inspector,260);assert.equal(value.wave,300);assert.equal(value.columns[0],defaultLayout.columns[0]);assert.equal(value.columns[1],defaultLayout.columns[1]);assert.equal(value.details,false);assert.deepEqual(safeLayout(null),safeLayout({}));
});
test('多选支持反选、双向范围与Ctrl Shift累加且不选入其他页',()=>{
 const ids=['a','b','c','d'];assert.deepEqual(selectIds(ids,['a'],'a',null,true,false),[]);assert.deepEqual(selectIds(ids,[],'a','c',false,true),['a','b','c']);assert.deepEqual(selectIds(ids,['d'],'b','a',true,true),['d','a','b']);assert.deepEqual(selectIds(ids,['a'],'other',null,false,false),['a']);
});

test('旧五列布局按列名迁移宽度，四侧折叠状态可恢复',()=>{
 const value=safeLayout({columns:[420,80,90,70,100],topCollapsed:true,leftCollapsed:true,bottomCollapsed:true,details:false});
 // 旧五列 [name,duration,rate,format,status] 按列名回填到新十二列；status 已删被忽略，波形列取默认宽。
 assert.deepEqual(value.columns,[32,420,100,96,80,90,64,52,68,70,36,44]);assert.equal(value.topCollapsed,true);assert.equal(value.leftCollapsed,true);assert.equal(value.bottomCollapsed,true);assert.equal(value.details,false);
 assert.equal(safeLayout({topCollapsed:'true'}).topCollapsed,false);
});

test('列偏好清洗重复和未知键，含新列时保留用户列序并默认显示九列',()=>{
 const old=safeLayout({columns:[420,80,90,70,100]});
 assert.deepEqual(visibleColumns(old).map(column=>column.id),['index','wave','name','category','duration','channels','rate','favorite','playlist']);
 // 含新列 index 的存档不触发整体重置：去重、剔除未知键后按用户顺序补齐其余列。
 const broken=safeLayout({columnOrder:['index','index','obsolete',null,'name'],hiddenColumns:['obsolete','wave','wave']});
 assert.deepEqual(broken.columnOrder,['index','name','wave','category','duration','bpm','key','channels','rate','format','favorite','playlist']);assert.deepEqual(broken.hiddenColumns,['wave']);
 assert.deepEqual(safeLayout({columnOrder:'bad',hiddenColumns:'bad'}).columnOrder,columnDefinitions.map(column=>column.id));
});
test('调序与隐藏不改变列身份对应的宽度，并至少保留一列',()=>{
 const initial=safeLayout({columns:[410,220,85,95,75,105],hiddenColumns:[]});
 const moved=moveColumn(initial,'format',-1);assert.deepEqual(moved.columnOrder,['index','wave','name','category','duration','bpm','key','channels','format','rate','favorite','playlist']);
 assert.deepEqual(moved.columns,initial.columns);assert.equal(columnDefinitions.find(column=>column.id==='format').widthIndex,9);assert.equal(visibleColumns(moved).find(column=>column.id==='duration').widthIndex,4);
 let value=moved;for(const column of columnDefinitions)value=toggleColumn(value,column.id);
 assert.equal(visibleColumns(value).length,1);assert.equal(visibleColumns(toggleColumn(value,visibleColumns(value)[0].id)).length,1);
 const repaired=safeLayout({columnOrder:columnDefinitions.map(column=>column.id),hiddenColumns:columnDefinitions.map(column=>column.id)});assert.deepEqual(visibleColumns(repaired).map(column=>column.id),['name']);
 const roundtrip=safeLayout(JSON.parse(JSON.stringify(value)));assert.deepEqual(roundtrip,value);
 const reset=resetColumns({...value,sidebar:340,leftCollapsed:true});assert.equal(reset.sidebar,340);assert.equal(reset.leftCollapsed,true);assert.deepEqual(visibleColumns(reset).map(column=>column.id),['index','wave','name','category','duration','channels','rate','favorite','playlist']);
});