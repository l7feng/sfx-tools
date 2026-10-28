export type ColumnId='name'|'wave'|'duration'|'rate'|'format'|'status';
export const columnDefinitions:{id:ColumnId;label:string;widthIndex:number}[]=[{id:'name',label:'素材名称',widthIndex:0},{id:'wave',label:'波形试听',widthIndex:1},{id:'duration',label:'时长',widthIndex:2},{id:'rate',label:'采样率',widthIndex:3},{id:'format',label:'格式',widthIndex:4},{id:'status',label:'状态',widthIndex:5}];
const allColumnIds=columnDefinitions.map(column=>column.id);
export type Layout={sidebar:number;inspector:number;wave:number;columns:number[];columnOrder:ColumnId[];hiddenColumns:ColumnId[];details:boolean;topCollapsed:boolean;leftCollapsed:boolean;bottomCollapsed:boolean};
export const defaultLayout:Layout={sidebar:220,inspector:260,wave:110,columns:[280,210,80,90,70,100],columnOrder:[...allColumnIds],hiddenColumns:['rate','status'],details:true,topCollapsed:false,leftCollapsed:false,bottomCollapsed:false};
export const clamp=(value:number,min:number,max:number)=>Math.min(max,Math.max(min,value));
export function safeLayout(raw:unknown):Layout{
 const v=(typeof raw==='object'&&raw?raw:{}) as Partial<Layout>;
 const number=(n:unknown,fallback:number,min:number,max:number)=>typeof n==='number'&&Number.isFinite(n)?clamp(n,min,max):fallback;
 const validIds=(value:unknown):ColumnId[]=>Array.isArray(value)?[...new Set(value.filter((id):id is ColumnId=>allColumnIds.includes(id as ColumnId)))]:[];
 const columnOrder=validIds(v.columnOrder);for(const id of allColumnIds)if(!columnOrder.includes(id))columnOrder.push(id);
 const hiddenColumns=Array.isArray(v.hiddenColumns)?validIds(v.hiddenColumns):[...defaultLayout.hiddenColumns];
 if(hiddenColumns.length===allColumnIds.length)hiddenColumns.splice(hiddenColumns.indexOf('name'),1);
 const columns=Array.isArray(v.columns)&&v.columns.length===5?[v.columns[0],210,...v.columns.slice(1)]:v.columns;
 return {columnOrder,hiddenColumns,sidebar:number(v.sidebar,220,150,420),inspector:number(v.inspector,260,210,460),wave:number(v.wave,110,56,300),columns:defaultLayout.columns.map((fallback,i)=>number(columns?.[i],fallback,i===0?180:i===1?120:60,i===0?900:i===1?600:300)),details:typeof v.details==='boolean'?v.details:true,topCollapsed:v.topCollapsed===true,leftCollapsed:v.leftCollapsed===true,bottomCollapsed:v.bottomCollapsed===true};
}

export function visibleColumns(layout:Layout){
 const safe=safeLayout(layout);return safe.columnOrder.filter(id=>!safe.hiddenColumns.includes(id)).map(id=>columnDefinitions.find(column=>column.id===id)!);
}
export function moveColumn(layout:Layout,id:ColumnId,direction:-1|1):Layout{
 const safe=safeLayout(layout),index=safe.columnOrder.indexOf(id),target=index+direction;
 if(index<0||target<0||target>=safe.columnOrder.length)return safe;
 const order=[...safe.columnOrder];[order[index],order[target]]=[order[target],order[index]];return {...safe,columnOrder:order};
}
export function toggleColumn(layout:Layout,id:ColumnId):Layout{
 const safe=safeLayout(layout);if(!allColumnIds.includes(id))return safe;
 if(safe.hiddenColumns.includes(id))return {...safe,hiddenColumns:safe.hiddenColumns.filter(value=>value!==id)};
 if(visibleColumns(safe).length<=1)return safe;
 return {...safe,hiddenColumns:[...safe.hiddenColumns,id]};
}
export function resetColumns(layout:Layout):Layout{
 return {...safeLayout(layout),columns:[...defaultLayout.columns],columnOrder:[...defaultLayout.columnOrder],hiddenColumns:[...defaultLayout.hiddenColumns]};
}
export function selectIds(ids:string[],current:string[],id:string,anchor:string|null,toggle:boolean,range:boolean):string[]{
 if(!ids.includes(id))return current;
 if(range&&anchor&&ids.includes(anchor)){const a=ids.indexOf(anchor),b=ids.indexOf(id);const values=ids.slice(Math.min(a,b),Math.max(a,b)+1);return toggle?[...new Set([...current,...values])]:values;}
 return toggle?(current.includes(id)?current.filter(value=>value!==id):[...current,id]):[id];
}
