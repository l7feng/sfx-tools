import React,{useLayoutEffect,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import './playlist-picker.css';
export type PlaylistOption={id:string;name:string;count:number};
/** 列表行「≡」按钮的浮层：选现有歌单，或新建后直接加入；只写目录，不动音频文件。 */
export function PlaylistPicker({x,y,ids,library,playlists,onClose,onDone,notify}:{x:number;y:number;ids:string[];library:'sound'|'music';playlists:PlaylistOption[];onClose:()=>void;onDone:()=>void;notify:(message:string)=>void}){
 const root=useRef<HTMLDivElement>(null),[position,setPosition]=useState({left:x,top:y});
 const [name,setName]=useState(''),[busy,setBusy]=useState(false),[error,setError]=useState('');
 useLayoutEffect(()=>{const element=root.current;if(!element)return;const place=()=>{const rect=element.getBoundingClientRect();setPosition({left:Math.max(6,Math.min(x-rect.width-4,window.innerWidth-rect.width-6)),top:Math.max(6,Math.min(y+4,window.innerHeight-rect.height-6))});};place();element.querySelector<HTMLInputElement>('input')?.focus();window.addEventListener('resize',place);return()=>window.removeEventListener('resize',place);},[x,y]);
 async function join(playlistId:string){
  if(busy)return;setBusy(true);setError('');
  try{await invoke('library_playlist_add',{playlistId,ids});notify(`已把 ${ids.length} 条素材加入播放列表`);onDone();onClose();}
  catch(e){setError(String(e));setBusy(false);}
 }
 async function create(){
  if(busy||!name.trim())return;setBusy(true);setError('');
  try{const id=await invoke<string>('library_playlist_create',{library,name:name.trim()});await invoke('library_playlist_add',{playlistId:id,ids});notify(`已新建播放列表「${name.trim()}」并加入 ${ids.length} 条素材`);onDone();onClose();}
  catch(e){setError(String(e));setBusy(false);}
 }
 return <div className="playlist-picker-backdrop" onPointerDown={event=>{if(event.target===event.currentTarget)onClose();}} onContextMenu={event=>{event.preventDefault();onClose();}}><div ref={root} className="playlist-picker" role="dialog" aria-label="加入播放列表" style={position} onKeyDown={event=>{if(event.key==='Escape'){event.preventDefault();onClose();}else if(event.key==='Enter'&&name.trim())void create();}}>
  <header><strong>加入播放列表</strong><button onClick={onClose} aria-label="关闭" disabled={busy}>×</button></header>
  <p className="playlist-picker-hint">已选 {ids.length} 条素材</p>
  <div className="playlist-picker-list">{playlists.length?playlists.map(item=><button key={item.id} disabled={busy} onClick={()=>void join(item.id)}><span>{item.name}</span><small>{item.count}</small></button>):<p className="playlist-picker-empty">还没有播放列表，在下面新建一个。</p>}</div>
  <div className="playlist-picker-create"><input aria-label="新播放列表名称" value={name} maxLength={40} placeholder="新建播放列表…" disabled={busy} onChange={event=>setName(event.target.value)}/><button disabled={busy||!name.trim()} onClick={()=>void create()}>新建并加入</button></div>
  {error&&<p className="asset-error" role="alert">{error}</p>}
 </div></div>;
}
