import React,{useEffect,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {displayPath} from './catalog-client';
import {toolEntries,type ToolEntry} from './tool-navigation';
import {locateApp} from './tools-window';
import './ext-setup.css';

/** 首次配置向导：把三个外部程序（MUS Prompt / UCS Namer / Audio Dedup）一次配好。
 *
 *  为什么要有这一屏：这三张卡片在「管理 › 扩展」里，但对新装机器来说，程序在哪、
 *  装没装都不确定。与其让用户在卡片上一个个试，不如开局一次说清，配完就不再打扰。
 *  跳过也不会破坏什么：卡片上仍能手动选一次并记住。 */
const FLAG='sound-workbench-extapp-setup-v1';
export const extSetupDone=()=>{try{return localStorage.getItem(FLAG)==='done';}catch{return true;}};

type Info={version:string;path:string};
type Row={entry:ToolEntry;info:Info|null;busy:boolean};

export function ExtSetup({open,onClose,native}:{open:boolean;onClose:(done:boolean)=>void;native:boolean}){
 const dialog=useRef<HTMLDialogElement>(null);
 const apps=toolEntries.filter(entry=>entry.kind==='app');
 const [rows,setRows]=useState<Row[]>(()=>apps.map(entry=>({entry,info:null,busy:false})));
 const [scanning,setScanning]=useState(false),[flash,setFlash]=useState('');

 useEffect(()=>{
  const node=dialog.current;if(!node)return;
  if(open&&!node.open)node.showModal();else if(!open&&node.open)node.close();
 },[open]);
 useEffect(()=>{
  if(!open||!native)return;
  void scanAll();
 },[open,native]);

 const patch=(id:string,next:Partial<Row>)=>setRows(old=>old.map(row=>row.entry.id===id?{...row,...next}:row));

 const scanAll=async()=>{
  setScanning(true);setFlash('正在自动查找三个程序…');
  let hits=0;
  for(const entry of apps){
   patch(entry.id,{busy:true});
   try{
    const info=await locateApp(entry);
    patch(entry.id,{info,busy:false});hits+=1;
   }catch{patch(entry.id,{info:null,busy:false});}
  }
  setScanning(false);
  setFlash(hits?`自动找到 ${hits} / ${apps.length} 个程序`:`一个都没找到，请逐项手动选择（装在哪就选哪）`);
 };

 const pick=async(entry:ToolEntry)=>{
  if(!native){setFlash('需要桌面版才能选择本机文件。');return;}
  patch(entry.id,{busy:true});
  try{
   const path=await invoke<string|null>('plugin_pick_file',{initial:rowPath(entry.id)||null,title:`选择 ${entry.label} 程序`});
   if(!path){patch(entry.id,{busy:false});return;}
   if(entry.exe)await invoke('extapp_remember',{name:entry.exe,path});
   try{localStorage.setItem(`sound-workbench-app-${entry.id}`,path);}catch{/* 存储不可用不影响后端记录 */}
   patch(entry.id,{info:{path,version:''},busy:false});
   setFlash(`已记住 ${entry.label}：${displayPath(path)}`);
  }catch(e){patch(entry.id,{busy:false});setFlash(`选择失败：${String(e)}`);}
 };

 const rowPath=(id:string)=>{
  const row=rows.find(item=>item.entry.id===id);
  return row?.info?.path??'';
 };

 const finish=(done:boolean)=>{
  try{if(done)localStorage.setItem(FLAG,'done');}catch{/* 记不住标记就下次再问 */}
  onClose(done);
 };

 const found=rows.filter(row=>row.info).length;
 return <dialog ref={dialog} className="ext-setup" aria-label="扩展程序配置" onClose={()=>onClose(false)}>
  <header className="ext-setup-head">
   <div><h1>先配一次，之后一键就用</h1><span>三个本机程序，配好之后在「管理 › 扩展」里单击启动，不用再找路径</span></div>
   <button aria-label="关闭配置向导" onClick={()=>finish(false)}>×</button>
  </header>
  <div className="ext-setup-body">
   {rows.map(row=><article key={row.entry.id} className="ext-setup-row">
    <div className="ext-setup-main">
     <b>{row.entry.label}</b>
     <span className="ext-setup-desc">{row.entry.description}</span>
     <span className={`ext-setup-state${row.info?' is-ok':''}`}>{row.busy?'定位中…':(row.info?`${row.entry.id==='dedup'&&row.info.version?`v${row.info.version} · `:''}${displayPath(row.info.path)}`:(native?'未找到':'需要桌面版'))}</span>
    </div>
    <button className="ext-setup-pick" disabled={row.busy} onClick={()=>void pick(row.entry)}>选择…</button>
   </article>)}
   {flash&&<p className="ext-setup-flash" role="status">{flash}</p>}
  </div>
  <footer className="ext-setup-foot">
   <span>已配置 {found} / {rows.length}{found<rows.length?'：缺的可以稍后再配，卡片上也能补':''}</span>
   <div>
    <button className="ext-setup-ghost" disabled={scanning} onClick={()=>void scanAll()}>{scanning?'查找中…':'重新自动查找'}</button>
    <button className="ext-setup-ghost" onClick={()=>finish(false)}>稍后再说</button>
    <button className="ext-setup-primary" onClick={()=>finish(true)}>完成</button>
   </div>
  </footer>
 </dialog>;
}
