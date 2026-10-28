import React,{useEffect,useRef,useState,useSyncExternalStore} from 'react';
import {invoke,isTauri} from '@tauri-apps/api/core';
import type {CatalogAsset} from './catalog-client';
import {appendLayer,safeLayer,updateLayer,pruneLayerState,refreshLayerMetadata,LAYER_LIMIT,LAYER_SECONDS,type LayerClip} from './layer-state';
import {renderRecipe,renderRecipeKey,renderIsActive,renderMatches,renderReady,renderProgress,type RenderSnapshot} from './layer-render-state';
import './layer-tools.css';
const key='sound-workbench-layer-v1';
type LayerSnapshot={clips:LayerClip[];error:string};
let snapshot:LayerSnapshot|null=null;
const listeners=new Set<()=>void>();
function read():LayerSnapshot{
 if(snapshot)return snapshot;
 try{snapshot={clips:safeLayer(JSON.parse(localStorage.getItem(key)??'[]')),error:''};}
 catch{snapshot={clips:[],error:'叠层配置无法读取，当前使用空编排。'};}
 return snapshot;
}
function write(clips:LayerClip[]){
 let error='';try{localStorage.setItem(key,JSON.stringify(clips));}catch{error='本机存储不可用；当前编排仅在本次窗口中保留。';}
 snapshot={clips,error};for(const listener of listeners)listener();
}
function subscribe(listener:()=>void){listeners.add(listener);return()=>{listeners.delete(listener);};}
if(typeof window!=='undefined')window.addEventListener('storage',event=>{if(event.key===key||event.key===null){snapshot=null;read();for(const listener of listeners)listener();}});
export function addToLayer(assets:CatalogAsset[],scatter=false){
 const result=appendLayer(read().clips,assets,scatter,()=>crypto.randomUUID());write(result.clips);
 return {added:result.added,skipped:result.skipped};
}
export function pruneLayer(selection:{ids?:string[];folder?:string}){
 const before=read().clips,after=pruneLayerState(before,selection);write(after);return before.length-after.length;
}
export function getLayerAssetIds(){return [...new Set(read().clips.map(clip=>clip.assetId))];}
export function refreshLayerAssets(assets:CatalogAsset[]){write(refreshLayerMetadata(read().clips,assets));}
const time=(seconds:number)=>`${Math.floor(seconds/60)}:${(seconds%60).toFixed(2).padStart(5,'0')}`;
export function LayerTools(){
 const {clips,error}=useSyncExternalStore(subscribe,read,read),[scale,setScale]=useState(10);
 const confirm=useRef<HTMLDialogElement>(null),alive=useRef(true),lock=useRef(false),revision=useRef(0);
 const native=isTauri(),[job,setJob]=useState<RenderSnapshot|null>(null),[known,setKnown]=useState(false),[busy,setBusy]=useState(false),[operationError,setOperationError]=useState(''),[statusError,setStatusError]=useState(''),[message,setMessage]=useState(''),[exportPath,setExportPath]=useState('');
 const active=renderIsActive(job),matches=renderMatches(job,clips),ready=known&&!statusError&&renderReady(job,clips),locked=active||busy||(native&&!known);
 const recipeKey=renderRecipeKey(clips),messageRecipe=useRef(recipeKey);
 useEffect(()=>{alive.current=true;return()=>{alive.current=false;revision.current++;};},[]);
 useEffect(()=>{if(messageRecipe.current!==recipeKey){messageRecipe.current=recipeKey;setMessage('');setExportPath('');}},[recipeKey]);
 useEffect(()=>{
  if(!native)return;let stopped=false,timer:number;
  const poll=async()=>{const version=revision.current;
   try{if(!lock.current){const value=await invoke<RenderSnapshot>('layer_render_status');if(!stopped&&version===revision.current){setJob(value);setKnown(true);setStatusError('');}}}
   catch(e){if(!stopped&&version===revision.current)setStatusError(String(e));}
   finally{if(!stopped)timer=window.setTimeout(poll,700);}
  };void poll();return()=>{stopped=true;clearTimeout(timer);};
 },[native]);
 async function run(work:()=>Promise<void>){
  if(lock.current)return;lock.current=true;setBusy(true);setOperationError('');setMessage('');revision.current++;
  try{await work();}catch(e){if(alive.current)setOperationError(String(e));}
  finally{revision.current++;lock.current=false;if(alive.current)setBusy(false);}
 }
 const startRender=()=>run(async()=>{
  const id=await invoke<string>('layer_render_start',{clips:renderRecipe(clips)});
  const value=await invoke<RenderSnapshot>('layer_render_status');
  if(value.job_id!==id)throw new Error('渲染任务已变化，请等待状态刷新后再操作。');
  if(alive.current){setJob(value);setKnown(true);setExportPath('');setStatusError('');setMessage('已提交当前编排快照；未自动播放。');}
 });
 const cancelRender=()=>run(async()=>{await invoke('layer_render_cancel');const value=await invoke<RenderSnapshot>('layer_render_status');if(alive.current){setJob(value);setMessage('已请求取消；正在释放临时渲染文件。');}});
 const previewRender=()=>run(async()=>{if(!ready||!job?.job_id)return;await invoke('player_render_preview',{jobId:job.job_id});if(alive.current)setMessage('正在试听已生成的叠层结果，可在底部播放器暂停或停止。');});
 const exportRender=()=>run(async()=>{
  if(!ready||!job?.job_id)return;
  let initial:string|null=null;try{initial=localStorage.getItem('sound-workbench-layer-export-folder-v1');}catch{/* 目录记忆不可用仍可导出 */}
  const folder=await invoke<string|null>('catalog_pick_folder',{initial});if(!folder)return;
  const result=await invoke<{path:string;bytes:number}>('layer_render_export',{jobId:job.job_id,folder});
  try{localStorage.setItem('sound-workbench-layer-export-folder-v1',folder);}catch{/* 不影响已导出的文件 */}
  if(alive.current){setExportPath(result.path);setMessage('WAV已导出，原素材与现有文件保持不变；导出结果尚未加入素材库。');}
 });
 const states:Record<string,string>={idle:'尚未渲染',queued:'等待中',preparing:'准备音频',running:'正在渲染',cancelling:'正在取消',cancelled:'已取消',failed:'渲染失败',completed:'渲染完成'};

 const end=clips.reduce((value,clip)=>Math.max(value,clip.offset+clip.duration),0),timeline=Math.max(10,Math.ceil(end/10)*10),width=Math.max(400,Math.min(36000,timeline*scale));
 return <section className="layer-tools utility-panel" aria-label="叠层编排"><header><div><h2>叠层编排 <small>{clips.length}/{LAYER_LIMIT} 片段</small></h2><p>添加到叠层：从零点叠放；散布：从当前尾部连续铺设。先渲染当前编排，再显式试听或导出WAV。</p></div><button disabled={!clips.length||locked} onClick={()=>confirm.current?.showModal()}>清空编排…</button></header>
 {error&&<p className="layer-error" role="alert">{error}</p>}
 <div className="layer-toolbar"><span>总长 {time(end)}</span><label>时间轴缩放<input type="range" aria-label="叠层时间轴缩放" min="2" max="100" value={scale} onChange={event=>setScale(Number(event.target.value))}/></label><span>秒</span></div>
 {!clips.length?<p className="layer-empty">在素材右键菜单选择“添加到叠层”或“铺设到叠层（散布）”。支持已知时长、可用且不超过60分钟的素材。</p>:<div className="layer-timeline-scroll"><div className="layer-timeline" style={{width}}><div className="layer-ruler"><span>0:00</span><span>{time(timeline/2)}</span><span>{time(timeline)}</span></div>{clips.map((clip,index)=><div key={clip.id} className="layer-lane"><div className="layer-clip" style={{left:`${clip.offset/timeline*100}%`,width:`${clip.duration/timeline*100}%`}} title={`${clip.name} · ${time(clip.offset)} → ${time(clip.offset+clip.duration)}`}><span>{index+1} · {clip.name}</span></div></div>)}</div></div>}
 {clips.length>0&&<div className="layer-clip-list">{clips.map((clip,index)=><article key={clip.id}><div className="layer-clip-name"><strong title={clip.path}>{index+1}. {clip.name}</strong><small>时长 {time(clip.duration)}</small></div><label>起点（秒）<input type="number" min="0" max={LAYER_SECONDS-clip.duration} step="0.01" aria-label={`${clip.name} 起点秒数`} disabled={locked} value={clip.offset} onChange={event=>{if(event.target.value!=='')write(updateLayer(clips,clip.id,{offset:Number(event.target.value)}));}}/></label><label>音量（%）<input type="number" min="0" max="200" step="1" aria-label={`${clip.name} 编排音量百分比`} disabled={locked} value={Math.round(clip.gain*100)} onChange={event=>{if(event.target.value!=='')write(updateLayer(clips,clip.id,{gain:Number(event.target.value)/100}));}}/></label><button aria-label={`移除叠层片段 ${clip.name}`} disabled={locked} onClick={()=>write(clips.filter(item=>item.id!==clip.id))}>移除</button></article>)}</div>}
 <div className="layer-render-panel" aria-label="叠层渲染与导出"><div className="layer-render-heading"><strong>混音渲染</strong><span>48 kHz · 双声道 · PCM16 WAV</span></div><div className="layer-render-actions"><button disabled={!native||!known||busy||active||!clips.length} onClick={()=>void startRender()}>{active?'正在渲染…':'渲染当前编排'}</button><button disabled={!native||busy||!active||job?.status==='cancelling'} onClick={()=>void cancelRender()}>取消渲染</button><button disabled={!native||busy||!ready} onClick={()=>void previewRender()}>▶ 试听结果</button><button disabled={!native||busy||!ready} onClick={()=>void exportRender()}>导出WAV…</button></div>
 {!native?<p>混音渲染需要桌面版。</p>:!known&&!statusError?<p>正在读取后台任务状态…</p>:job&&<><div className="layer-render-progress"><progress max="1" value={renderProgress(job.progress)} aria-label="叠层渲染进度"/><span role="status">{states[job.status]??job.status} {active?`${Math.round(renderProgress(job.progress)*100)}% · ${time(job.rendered_seconds)} / ${time(job.duration)}`:''}</span></div>{active&&!matches&&<p className="layer-render-warning">后台正在处理之前提交的编排快照；当前编排已有变化，完成后请重新渲染。</p>}{job.status==='completed'&&!matches&&<p className="layer-render-warning">编排与已有结果不同，请重新渲染后试听或导出。</p>}{job.status==='completed'&&matches&&<p className="layer-render-metrics">时长 {time(job.duration)} · 峰值 {Number.isFinite(job.peak)?job.peak.toFixed(3):'未知'} · 截幅采样 {job.clipped_samples.toLocaleString()}{job.clipped_samples>0&&<strong> 有过载截幅，请降低片段音量后重新渲染。</strong>}</p>}{job.status==='completed'&&job.output_path&&<details className="layer-cache-path"><summary>渲染缓存位置</summary><p>{job.output_path}</p></details>}{job.source_stale&&<p className="layer-render-warning">来源已变化，请复扫素材并重新渲染。</p>}{job.error&&<p className="layer-error" role="alert">{job.error}</p>}</>}
 {statusError&&<p className="layer-error" role="alert">任务状态读取失败：{statusError}</p>}{operationError&&<p className="layer-error" role="alert">{operationError}</p>}{message&&<p role="status">{message}</p>}{exportPath&&<p className="layer-export-path" title={exportPath}>{exportPath}</p>}
 <p>最多32个片段同时重叠；支持不高于48 kHz的单/双声道素材，单声道复制至左右声道。高于48 kHz或多声道请先转换，高质量降采样留后续。结果保留在缓存，导出时选择目标目录且不覆盖已有文件；过载仅截幅并报告，不自动调整音量。</p></div>
 <p className="layer-footnote">编排最长60分钟、最多200片段。移除片段或清空编排仅影响此处的编排记录，源音频与素材库保留。</p>
 <dialog ref={confirm} className="layer-confirm"><h3>清空叠层编排？</h3><p>将移除当前 {clips.length} 个编排片段。源音频与素材库记录保留，此操作不能撤销。</p><div className="dialog-actions"><button onClick={()=>confirm.current?.close()}>取消</button><button disabled={locked} onClick={()=>{if(!locked){write([]);confirm.current?.close();}}}>清空编排</button></div></dialog>
 </section>;
}
