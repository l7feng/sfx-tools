import React,{useLayoutEffect,useRef,useState} from 'react';
import {createPortal} from 'react-dom';
import {invoke} from '@tauri-apps/api/core';
import type {CatalogAsset} from './catalog-client';
export function CatalogTools({selected,ids,native,busy,action,refresh,notify,onRelocated}:{selected:CatalogAsset|null;ids:string[];native:boolean;busy:boolean;action:(work:()=>Promise<void>)=>Promise<void>;refresh:()=>Promise<void>;notify:(message:string)=>void;onRelocated:()=>void}){
 const [path,setPath]=useState(''),[anchor,setAnchor]=useState<{x:number;y:number}|null>(null),[hash,setHash]=useState('');
 const current=React.useRef(selected?.id);current.current=selected?.id;
 const alive=React.useRef(true);React.useEffect(()=>{alive.current=true;return()=>{alive.current=false;};},[]);
 React.useEffect(()=>{setPath('');setHash('');setAnchor(null);},[selected?.id]);
 /* 「失联重定位」面板走浮层：工具栏行高固定，面板内联展开会把这一行撑高并被横向滚动区裁掉。 */
 const panel=useRef<HTMLDivElement>(null);
 const [position,setPosition]=useState({left:0,top:0});
 useLayoutEffect(()=>{const element=panel.current;if(!anchor||!element)return;const place=()=>{const rect=element.getBoundingClientRect();setPosition({left:Math.max(6,Math.min(anchor.x,window.innerWidth-rect.width-6)),top:Math.max(6,Math.min(anchor.y,window.innerHeight-rect.height-6))});};place();window.addEventListener('resize',place);return()=>window.removeEventListener('resize',place);},[anchor]);
 useLayoutEffect(()=>{if(!anchor)return;const onKey=(event:KeyboardEvent)=>{if(event.key==='Escape'){event.preventDefault();setAnchor(null);}};window.addEventListener('keydown',onKey);return()=>window.removeEventListener('keydown',onKey);},[anchor]);
 return <section className="catalog-tools" aria-label="素材取用与修复">
 <div className="asset-actions"><span>已选 {ids.length} 条</span><button disabled={!native||busy||!ids.length} onClick={()=>void action(async()=>{const n=await invoke<number>('catalog_copy_paths',{ids});notify(`已复制 ${n} 条完整路径`);})}>复制路径</button><button disabled={!native||busy||!selected} onClick={()=>void action(async()=>{await invoke('catalog_reveal',{id:selected!.id});})}>打开位置</button><button disabled={!native||busy||!selected} onClick={event=>{const rect=event.currentTarget.getBoundingClientRect();setAnchor(old=>old?null:{x:rect.left,y:rect.bottom+4});}} aria-expanded={anchor!==null}>失联重定位</button></div>
 {anchor&&selected&&createPortal(<><div className="relocate-backdrop" onPointerDown={event=>{if(event.target===event.currentTarget)setAnchor(null);}} onContextMenu={event=>{event.preventDefault();if(event.target===event.currentTarget)setAnchor(null);}}/><div ref={panel} className="relocate-panel" role="dialog" aria-label="失联重定位" style={position}>
 <header><strong>失联重定位</strong><button onClick={()=>setAnchor(null)} aria-label="关闭失联重定位">×</button></header>
 <p>移动素材前先登记完整内容指纹。源文件已经失联且没有指纹时，保留记录，不能仅凭名称认领其他文件。</p><button disabled={busy} onClick={()=>void action(async()=>{const id=selected.id;const value=await invoke<string>("catalog_prepare_fingerprint",{id});if(alive.current&&current.current===id){setHash(value);notify("已保存素材内容指纹");}})}>登记源文件指纹</button>{hash&&<small>SHA256：{hash}</small>}<label>移动后的完整文件路径<input value={path} onChange={e=>setPath(e.target.value)} placeholder="D:\声音素材\新位置\音效.wav"/></label><button disabled={busy||!path.trim()} onClick={()=>void action(async()=>{const value=await invoke<string>('catalog_relocate',{id:selected.id,newPath:path.trim()});if(alive.current&&current.current===selected.id){notify(`已重定位：${value}`);setAnchor(null);onRelocated();await refresh();}})}>核验内容并关联</button>
 </div></>,document.body)}
 </section>;
}
