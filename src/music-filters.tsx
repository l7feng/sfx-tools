import React,{useEffect,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import type {CatalogAsset} from './catalog-client';
import {DURATION_PRESETS,MUSIC_DIMENSIONS,type DurationRange,type MusicDimension,type MusicFilterTemplates} from './music-filter-defaults';
import './music-filters.css';

export type {DurationRange};
type TagCount={name:string;count:number};
/** 音乐库常驻筛选维度条：维度值来自已有标签（带计数）＋可编辑预置模板；同一时刻只展开一个下拉。 */
const dimensionLabel=(key:string)=>MUSIC_DIMENSIONS.find(item=>item.key===key)?.label??key;
function valuesFor(dimension:MusicDimension,tags:TagCount[]){
 const counts=new Map<string,number>();
 for(const preset of dimension.presets)counts.set(preset,0);
 for(const tag of tags){
  const separator=tag.name.indexOf(':');if(separator<1)continue;
  const prefix=tag.name.slice(0,separator),value=tag.name.slice(separator+1).trim();
  if(prefix!==dimension.key&&!(dimension.key==='流派'&&prefix==='曲风'))continue;
  if(value)counts.set(value,(counts.get(value)??0)+tag.count);
 }
 return [...counts.entries()].map(([value,count])=>({value,count}));
}
/** 老数据可能用「曲风:」前缀，筛选时优先用它真实存在的标签名。 */
function tagNameFor(key:string,value:string,tags:TagCount[]){
 const canonical=`${key}:${value}`;
 if(tags.some(tag=>tag.name===canonical))return canonical;
 const alias=key==='流派'?`曲风:${value}`:'';
 return alias&&tags.some(tag=>tag.name===alias)?alias:canonical;
}
function sameRange(a:DurationRange,b:DurationRange){return a.min===b.min&&a.max===b.max;}
const EMPTY_RANGE:DurationRange={min:null,max:null};

export function MusicFilters({native,selected,tags,filter,filters,duration,templates,onFilter,onFilters,onDuration,onChanged}:{
 native:boolean;selected:CatalogAsset|null;tags:TagCount[];filter:string|null;filters:string[];duration:DurationRange;
 templates:MusicFilterTemplates;
 onFilter:(value:string|null)=>void;onFilters:(value:string[])=>void;onDuration:(value:DurationRange)=>void;onChanged:()=>void;
}){
 const [draft,setDraft]=useState<Record<string,string>>({}),[busy,setBusy]=useState(false),[message,setMessage]=useState('');
 // 手风琴：同一时刻只展开一个筛选下拉，点开另一个即收起上一个。
 const [openKey,setOpenKey]=useState<string|null>(null);
 useEffect(()=>{
  const outside=(event:PointerEvent)=>{if(!(event.target as HTMLElement)?.closest('.music-dimension'))setOpenKey(null);};
  const escape=(event:KeyboardEvent)=>{if(event.key==='Escape')setOpenKey(null);};
  document.addEventListener('pointerdown',outside);document.addEventListener('keydown',escape);
  return()=>{document.removeEventListener('pointerdown',outside);document.removeEventListener('keydown',escape);};
 },[]);
 const dimensions=MUSIC_DIMENSIONS.map(item=>({...item,presets:templates[item.key]??item.presets}));
 const activeDuration=!sameRange(duration,EMPTY_RANGE);
 const chips:{key:string;label:string;clear:()=>void}[]=[
  ...(filter?[{key:`tag:${filter}`,label:`标签 · ${filter}`,clear:()=>onFilter(null)}]:[]),
  ...filters.map(item=>{const separator=item.indexOf(':');const key=separator>0?item.slice(0,separator):'';const value=separator>0?item.slice(separator+1):item;
   return {key:`dim:${item}`,label:`${dimensionLabel(key)} · ${value}`,clear:()=>onFilters(filters.filter(value=>value!==item))};}),
  ...(activeDuration?[{key:'duration',label:`时长 · ${DURATION_PRESETS.find(item=>sameRange(item,duration))?.label??`${duration.min??0}–${duration.max??'∞'} 秒`}`,clear:()=>onDuration(EMPTY_RANGE)}]:[])
 ];
 const toggle=(name:string)=>onFilters(filters.includes(name)?filters.filter(item=>item!==name):[...filters,name].slice(-8));
 const summaryOpen=(key:string)=>(event:React.MouseEvent<HTMLElement>)=>{event.preventDefault();setOpenKey(old=>old===key?null:key);};
 async function addTag(key:string,value:string){
  if(!selected||!value.trim()||busy)return;
  setBusy(true);setMessage('');
  const tag=`${key}:${value.trim()}`;
  try{
   const rows=await invoke<CatalogAsset[]>('catalog_get_items',{ids:[selected.id]});
   if(!rows[0])throw Error('素材已出库');
   await invoke('catalog_set_tags',{id:selected.id,tags:[...new Set([...rows[0].tags,tag])]});
   onChanged();setMessage(`已为所选素材添加「${tag}」`);setDraft(old=>({...old,[key]:''}));
  }catch(e){setMessage(String(e));}finally{setBusy(false);}
 }
 return <section className="music-filters" aria-label="音乐筛选维度">
  <div className="music-filters-row">
   <span className="music-filters-caption">筛选</span>
   {dimensions.map(dimension=>{
    const active=filters.filter(item=>item.startsWith(dimension.key+':')).length;
    return <details className={`music-dimension${active?' is-active':''}`} key={dimension.key} open={openKey===dimension.key}>
     <summary title={`按${dimension.label}筛选`} onClick={summaryOpen(dimension.key)}>{dimension.label}{active?` · ${active}`:''} <span aria-hidden="true">▾</span></summary>
     <div className="music-dimension-menu">
      <div className="music-dimension-head"><strong>{dimension.label}</strong><small>{active?`已选 ${active} 项`:'点选可多选，取交集'}</small></div>
      <div className="music-dimension-values">{valuesFor(dimension,tags).map(item=>{
       const name=tagNameFor(dimension.key,item.value,tags),on=filters.includes(name)||filters.includes(`${dimension.key}:${item.value}`);
       return <button key={item.value} className={`${on?'is-on':''}${item.count?'':' is-unused'}`} aria-pressed={on} title={item.count?`${dimension.label}：${item.value} · ${item.count} 条`:`库中还没有「${item.value}」这个分类，直接筛选会得到 0 条；可先用下方输入框给素材打标`} onClick={()=>toggle(name)}><span>{item.value}</span><small>{item.count||'—'}</small></button>;
      })}</div>
      <div className="music-dimension-add"><input aria-label={`自定义${dimension.label}分类`} value={draft[dimension.key]??''} maxLength={30} placeholder={`自定义${dimension.label}…`} onChange={event=>setDraft(old=>({...old,[dimension.key]:event.target.value}))} onKeyDown={event=>{if(event.key==='Enter')void addTag(dimension.key,draft[dimension.key]??'');}}/><button disabled={!native||!selected||busy||!(draft[dimension.key]??'').trim()} title={selected?'写入所选素材的标签，原音频不改写':'先在列表中选择一条素材'} onClick={()=>void addTag(dimension.key,draft[dimension.key]??'')}>加到素材</button></div>
     </div>
    </details>;
   })}
   <details className={`music-dimension${activeDuration?' is-active':''}`} open={openKey==='duration'}>
    <summary title="按时长筛选" onClick={summaryOpen('duration')}>时长{activeDuration?' ✓':''} <span aria-hidden="true">▾</span></summary>
    <div className="music-dimension-menu">
     <div className="music-dimension-head"><strong>时长</strong><small>按文件真实时长过滤</small></div>
     <div className="music-dimension-values">{DURATION_PRESETS.map(item=><button key={item.label} className={sameRange(item,duration)?'is-on':''} aria-pressed={sameRange(item,duration)} onClick={()=>onDuration(sameRange(item,duration)?EMPTY_RANGE:{min:item.min,max:item.max})}><span>{item.label}</span></button>)}</div>
    </div>
   </details>
   {message&&<span className="music-filters-message" role="status">{message}</span>}
  </div>
  {chips.length>0&&<div className="music-filters-chips">{chips.map(chip=><button key={chip.key} className="music-filter-chip" title="点击移除此筛选" onClick={chip.clear}>{chip.label}<span aria-hidden="true">×</span></button>)}<button className="music-filters-clear" onClick={()=>{onFilter(null);onFilters([]);onDuration(EMPTY_RANGE);}}>清除全部</button></div>}
 </section>;
}
