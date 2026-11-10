export type ColumnId='index'|'name'|'wave'|'category'|'duration'|'bpm'|'key'|'channels'|'rate'|'format'|'favorite'|'playlist';
export type LibraryKind='sound'|'music';
/** musicOnly 的列只在音乐库出现；sortable 的表头可点击切换升/降序。 */
export const columnDefinitions:{id:ColumnId;label:string;widthIndex:number;musicOnly?:boolean;sortable?:boolean}[]=[
 {id:'index',label:'#',widthIndex:0},
 {id:'wave',label:'波形',widthIndex:2},
 {id:'name',label:'文件名',widthIndex:1,sortable:true},
 {id:'category',label:'分类',widthIndex:3},
 {id:'duration',label:'时长',widthIndex:4,sortable:true},
 {id:'bpm',label:'BPM',widthIndex:7,musicOnly:true},
 {id:'key',label:'调性',widthIndex:8,musicOnly:true},
 {id:'channels',label:'声道',widthIndex:6},
 {id:'rate',label:'采样率',widthIndex:5,sortable:true},
 {id:'format',label:'格式',widthIndex:9},
 {id:'favorite',label:'收藏',widthIndex:10},
 {id:'playlist',label:'播放列表',widthIndex:11}
];
const allColumnIds=columnDefinitions.map(column=>column.id);
export const availableColumns=(library:LibraryKind)=>columnDefinitions.filter(column=>!column.musicOnly||library==='music');
export type Layout={sidebar:number;inspector:number;wave:number;columns:number[];columnOrder:ColumnId[];hiddenColumns:ColumnId[];details:boolean;topCollapsed:boolean;leftCollapsed:boolean;bottomCollapsed:boolean};
export const defaultLayout:Layout={sidebar:220,inspector:260,wave:110,columns:[32,200,100,96,68,68,64,52,68,64,36,44],columnOrder:[...allColumnIds],hiddenColumns:['format'],details:true,topCollapsed:false,leftCollapsed:false,bottomCollapsed:false};
export const clamp=(value:number,min:number,max:number)=>Math.min(max,Math.max(min,value));
/** 当前列数的存档原样保留宽度（safeLayout 再逐项夹取）；旧版 6 项含状态列 / 5 项未含波形列按列名回填；其余取新默认宽度。 */
function migrateColumns(raw:unknown):number[]{
 const widths=[...defaultLayout.columns];
 if(!Array.isArray(raw))return widths;
 if(raw.length===widths.length)return widths.map((fallback,index)=>typeof raw[index]==='number'&&Number.isFinite(raw[index])?raw[index] as number:fallback);
 const legacy:ColumnId[][]=[['name','wave','duration','rate','format','status' as ColumnId],['name','duration','rate','format','status' as ColumnId]];
 const order=legacy.find(item=>item.length===raw.length);
 if(!order)return widths;
 order.forEach((id,index)=>{const target=columnDefinitions.find(column=>column.id===id);if(target&&typeof raw[index]==='number'&&Number.isFinite(raw[index]))widths[target.widthIndex]=raw[index];});
 return widths;
}
export function safeLayout(raw:unknown):Layout{
 const v=(typeof raw==='object'&&raw?raw:{}) as Partial<Layout>;
 const number=(n:unknown,fallback:number,min:number,max:number)=>typeof n==='number'&&Number.isFinite(n)?clamp(n,min,max):fallback;
 const validIds=(value:unknown):ColumnId[]=>Array.isArray(value)?[...new Set(value.filter((id):id is ColumnId=>allColumnIds.includes(id as ColumnId)))]:[];
 const stored=validIds(v.columnOrder);
 // 旧版只认识 6 个列（含已删的状态列）。这类存档若直接沿用，新列会被全部追加到末尾且状态列的隐藏项残留，
 // 因此整体回到新默认列序；只要存档里出现任一新列，就按用户已经调过的顺序继续用。
 const legacyPrefix=['name','wave','duration','rate','format'];
 // 三种情况回到新默认列序：全新安装、纯旧版存档、旧列在前且新列被追加在后（升级途中写回的中间态）。
 const legacyOnly=stored.length>0&&stored.every(id=>legacyPrefix.includes(id));
 const appended=stored.length>legacyPrefix.length&&legacyPrefix.every((id,index)=>stored[index]===id);
 const resetOrder=!stored.length||legacyOnly||appended;
 const columnOrder=resetOrder?[...defaultLayout.columnOrder]:[...stored];
 for(const id of allColumnIds)if(!columnOrder.includes(id))columnOrder.push(id);
 const hiddenColumns=resetOrder||!Array.isArray(v.hiddenColumns)?[...defaultLayout.hiddenColumns]:validIds(v.hiddenColumns);
 if(hiddenColumns.length===allColumnIds.length)hiddenColumns.splice(hiddenColumns.indexOf('name'),1);
 const columns=migrateColumns(v.columns);
 return {columnOrder,hiddenColumns,sidebar:number(v.sidebar,220,150,420),inspector:number(v.inspector,260,210,460),wave:number(v.wave,110,56,300),columns:defaultLayout.columns.map((fallback,i)=>number(columns?.[i],fallback,i===0?24:i===1?120:36,i===0?900:i===1?600:300)),details:typeof v.details==='boolean'?v.details:true,topCollapsed:v.topCollapsed===true,leftCollapsed:v.leftCollapsed===true,bottomCollapsed:v.bottomCollapsed===true};
}

export function visibleColumns(layout:Layout,library:LibraryKind='sound'){
 const safe=safeLayout(layout),ids=new Set(availableColumns(library).map(column=>column.id));
 return safe.columnOrder.filter(id=>ids.has(id)&&!safe.hiddenColumns.includes(id)).map(id=>columnDefinitions.find(column=>column.id===id)!);
}
export function moveColumn(layout:Layout,id:ColumnId,direction:-1|1):Layout{
 const safe=safeLayout(layout),index=safe.columnOrder.indexOf(id),target=index+direction;
 if(index<0||target<0||target>=safe.columnOrder.length)return safe;
 const order=[...safe.columnOrder];[order[index],order[target]]=[order[target],order[index]];return {...safe,columnOrder:order};
}
export function toggleColumn(layout:Layout,id:ColumnId,library:LibraryKind='sound'):Layout{
 const safe=safeLayout(layout);if(!allColumnIds.includes(id))return safe;
 if(safe.hiddenColumns.includes(id))return {...safe,hiddenColumns:safe.hiddenColumns.filter(value=>value!==id)};
 if(visibleColumns(safe,library).length<=1)return safe;
 return {...safe,hiddenColumns:[...safe.hiddenColumns,id]};
}
export function resetColumns(layout:Layout):Layout{
 return {...safeLayout(layout),columns:[...defaultLayout.columns],columnOrder:[...defaultLayout.columnOrder],hiddenColumns:[...defaultLayout.hiddenColumns]};
}

export type SortField='name'|'duration'|'rate';
export type SortState={field:SortField;dir:'asc'|'desc'};
const SORT_FIELDS:SortField[]=['name','duration','rate'];
export function safeSort(raw:unknown):SortState{
 const [field,direction]=String(raw??'').split('_');
 return {field:SORT_FIELDS.includes(field as SortField)?field as SortField:'name',dir:direction==='desc'?'desc':'asc'};
}
export function sortValue(sort:SortState){return sort.dir==='desc'?`${sort.field}_desc`:sort.field;}
/** 点已排序列切换升降序，点其他列回到该列升序。 */
export function nextSort(sort:SortState,field:SortField):string{
 if(sort.field!==field)return field;
 return sort.dir==='asc'?`${field}_desc`:field;
}
export function selectIds(ids:string[],current:string[],id:string,anchor:string|null,toggle:boolean,range:boolean):string[]{
 if(!ids.includes(id))return current;
 if(range&&anchor&&ids.includes(anchor)){const a=ids.indexOf(anchor),b=ids.indexOf(id);const values=ids.slice(Math.min(a,b),Math.max(a,b)+1);return toggle?[...new Set([...current,...values])]:values;}
 return toggle?(current.includes(id)?current.filter(value=>value!==id):[...current,id]):[id];
}
