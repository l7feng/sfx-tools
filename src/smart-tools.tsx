import React,{useEffect,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
type PTStatus={connected:boolean;state:string;session:null|{name:string;sampleRate:unknown};error?:string};
type Preview={executionEnabled:boolean;assets:{id:string;path:string;size:number}[];target:PTStatus;warnings:string[]};
export function SmartTools({native,ids,initiallyOpen=false}:{native:boolean;ids:string[];initiallyOpen?:boolean}){
 const [open,setOpen]=useState(initiallyOpen),[busy,setBusy]=useState(false),[error,setError]=useState('');
 const [pt,setPT]=useState<PTStatus|null>(null),[preview,setPreview]=useState<Preview|null>(null),[destination,setDestination]=useState('new_track'),[location,setLocation]=useState('session_start');
 const lock=useRef(false),alive=useRef(true);
 const previewKey=JSON.stringify([ids,destination,location]),currentPreview=useRef(previewKey);currentPreview.current=previewKey;
 useEffect(()=>{alive.current=true;return()=>{alive.current=false;};},[]);
 useEffect(()=>{setPreview(null);},[ids.join('|'),destination,location]);
 async function run(work:()=>Promise<void>){if(lock.current)return;lock.current=true;setBusy(true);setError('');try{await work();}catch(e){if(alive.current)setError(String(e));}finally{lock.current=false;if(alive.current)setBusy(false);}}
 return <section className="smart-tools"><button className="smart-toggle" disabled={!native} onClick={()=>setOpen(v=>!v)} aria-expanded={open}>Pro Tools导入预览 <span>{open?'收起':'展开'}</span></button>
 {open&&<div className="smart-panel">
 <p>检测本机Pro Tools和当前工程，核对所选素材的导入安排。此版本只生成预览。</p><div className="smart-actions"><button disabled={busy} onClick={()=>void run(async()=>{const value=await invoke<PTStatus>('protools_status');if(alive.current)setPT(value);})}>检测连接</button><span>{pt?.session?.name??(pt?({not_listening:'未启动服务',ready:'连接就绪',no_session:'未打开工程',session_unavailable:'工程状态不可读',connection_failed:'连接失败'} as Record<string,string>)[pt.state]??pt.state:'尚未检测')}</span></div>{pt?.error&&<p className="asset-error">{pt.error}</p>}
 <div className="smart-actions"><label>目标<select value={destination} onChange={e=>setDestination(e.target.value)}><option value="new_track">新轨道</option><option value="clip_list">片段列表</option></select></label><label>位置<select value={location} onChange={e=>setLocation(e.target.value)}><option value="session_start">工程起点</option><option value="selection">当前选择位置</option></select></label><button disabled={busy||!ids.length} onClick={()=>{const selection=previewKey;void run(async()=>{const value=await invoke<Preview>('protools_import_preview',{ids,destination,location});if(alive.current&&selection===currentPreview.current){setPreview(value);setPT(value.target);}});}}>预览所选 {ids.length} 条</button></div>
 {preview&&<div className="pt-preview"><strong>导入预览 · 未执行</strong><p>目标：{preview.target.session?.name??'尚无可用工程'} · {preview.assets.length} 条素材</p>{preview.assets.map(item=><p key={item.id}>{item.path}</p>)}{preview.warnings.map(message=><p key={message}>{message}</p>)}</div>}
 {error&&<p className="asset-error" role="alert">{error}</p>}</div>}
 </section>;
}
