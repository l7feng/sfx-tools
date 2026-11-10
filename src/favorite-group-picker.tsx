import React,{useLayoutEffect,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import './playlist-picker.css';
import './favorite-group-picker.css';
export type FavoriteGroupOption={name:string;count:number};
/** 批量收藏分组：选已有分组、新建分组，或清空分组；只写本机目录，不动音频文件。 */
export function FavoriteGroupPicker({x,y,ids,groups,onClose,onDone,notify}:{x:number;y:number;ids:string[];groups:FavoriteGroupOption[];onClose:()=>void;onDone:()=>void;notify:(message:string)=>void}){
 const root=useRef<HTMLDivElement>(null),[position,setPosition]=useState({left:x,top:y});
 const [name,setName]=useState(''),[busy,setBusy]=useState(false),[error,setError]=useState('');
 useLayoutEffect(()=>{const element=root.current;if(!element)return;const place=()=>{const rect=element.getBoundingClientRect();setPosition({left:Math.max(6,Math.min(x-rect.width-4,window.innerWidth-rect.width-6)),top:Math.max(6,Math.min(y+4,window.innerHeight-rect.height-6))});};place();element.querySelector<HTMLInputElement>('input')?.focus();window.addEventListener('resize',place);return()=>window.removeEventListener('resize',place);},[x,y]);
 async function save(group:string|null){
  if(busy)return;setBusy(true);setError('');
  try{const count=await invoke<number>('catalog_set_favorite_group',{ids,group});notify(group?`已把 ${count} 条素材加入收藏分组「${group}」`:`已清除 ${count} 条素材的收藏分组（收藏保留）`);onDone();onClose();}
  catch(e){setError(String(e));setBusy(false);}
 }
 return <div className="playlist-picker-backdrop" onPointerDown={event=>{if(event.target===event.currentTarget)onClose();}} onContextMenu={event=>{event.preventDefault();onClose();}}><div ref={root} className="playlist-picker favorite-group-picker" role="dialog" aria-label="收藏分组" style={position} onKeyDown={event=>{if(event.key==='Escape'){event.preventDefault();onClose();}else if(event.key==='Enter'&&name.trim())void save(name.trim());}}>
  <header><strong>收藏分组</strong><button type="button" onClick={onClose} aria-label="关闭" disabled={busy}>×</button></header>
  <p className="playlist-picker-hint">已选 {ids.length} 条素材 · 设分组同时加入收藏</p>
  <div className="playlist-picker-list">
   {groups.length>0&&groups.map(item=><button key={item.name} type="button" disabled={busy} onClick={()=>void save(item.name)}><span>{item.name}</span><small>{item.count}</small></button>)}
   {!groups.length&&<p className="playlist-picker-empty">还没有分组，在下面输入新名称。</p>}
   <button type="button" className="favorite-group-clear" disabled={busy} onClick={()=>void save(null)}><span>清除分组（保留收藏）</span></button>
  </div>
  <div className="playlist-picker-create"><input aria-label="收藏分组名称" value={name} maxLength={40} placeholder="新分组名称…" disabled={busy} onChange={event=>setName(event.target.value)}/><button type="button" disabled={busy||!name.trim()} onClick={()=>void save(name.trim())}>新建并加入</button></div>
  {error&&<p className="asset-error" role="alert">{error}</p>}
 </div></div>;
}
