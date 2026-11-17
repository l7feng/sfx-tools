import React from 'react';
import type {ImportJob} from './catalog-client';
import {displayPath,jobLabels,resumable} from './catalog-client';
import './task-center.css';

type Library='sound'|'music';
const ACTIVE=['running','queued','cancelling'];
const FAILED=['failed','interrupted','completed_with_errors'];
const KIND:Record<string,string>={
 running:'扫描中',queued:'等待中',cancelling:'正在取消',completed:'完成',
 completed_with_errors:'完成（有失败项）',cancelled:'已取消',interrupted:'重启中断',failed:'失败'
};
const pageName=(library?:Library)=>library==='music'?'音乐库':'音效库';
const folderName=(root:string)=>displayPath(root).split(/[\\/]/).filter(Boolean).at(-1)??'—';

/** 状态栏任务胶囊：空闲隐藏，进行中显示进度，失败转警示色。 */
export function TaskChip({jobs,hide,open,onToggle}:{jobs:ImportJob[];hide:Set<string>;open:boolean;onToggle:()=>void}){
 const visible=jobs.filter(job=>!hide.has(job.id));
 const active=visible.filter(job=>ACTIVE.includes(job.status));
 const failed=visible.filter(job=>FAILED.includes(job.status));
 if(!active.length&&!failed.length)return null;
 const total=active.reduce((sum,job)=>sum+job.discovered,0);
 const done=active.reduce((sum,job)=>sum+job.processed,0);
 const percent=total?Math.min(100,Math.round(done/total*100)):0;
 const tone=failed.length&&!active.length?' is-failed':failed.length?' is-warn':'';
 return <button className={`task-chip${tone}`} aria-expanded={open} aria-label="展开任务抽屉" title={active.length?`${active.length} 项任务进行中，点开查看进度`:'有任务失败或中断，点开查看'} onClick={onToggle}>
  {active.length>0&&<span className="task-chip-bar" aria-hidden="true"><i style={{width:`${percent}%`}}/></span>}
  <span>任务 {active.length||failed.length}{active.length?` · ${percent}%`:` · ${failed.length} 失败`}</span>
  <em aria-hidden="true">{open?'⌄':'⌃'}</em>
 </button>;
}

/** 任务抽屉：一行一任务，横向进度 + 计数 + 路径，可取消 / 继续 / 打开目录。 */
export function TaskDrawer({jobs,hide,open,busy,onClose,onCancel,onResume,onReveal,onClearDone}:{
 jobs:ImportJob[];hide:Set<string>;open:boolean;busy:boolean;
 onClose:()=>void;onCancel:(id:string)=>void;onResume:(job:ImportJob)=>void;onReveal:(path:string)=>void;onClearDone:()=>void;
}){
 if(!open)return null;
 const rows=jobs.filter(job=>!hide.has(job.id));
 const live=rows.filter(job=>ACTIVE.includes(job.status));
 const done=rows.filter(job=>!ACTIVE.includes(job.status));
 return <section className="task-drawer" aria-label="任务抽屉">
  <div className="task-drawer-head">
   <strong>任务</strong>
   {live.length>0&&<span className="task-pill">{live.length} 进行中</span>}
   {done.length>0&&<span className="task-pill is-dim">{done.length} 已结束</span>}
   <span className="task-sp"/>
   {live.length>1&&<button className="task-act" disabled={busy} onClick={()=>live.forEach(job=>onCancel(job.id))}>全部取消</button>}
   {done.length>0&&<button className="task-act" disabled={busy} onClick={onClearDone}>清空已结束</button>}
   <button className="task-act" aria-label="收起任务抽屉" onClick={onClose}>收起 ⌄</button>
  </div>
  <div className="task-drawer-rows">
   {!rows.length&&<p className="task-empty">暂无任务。</p>}
   {rows.map(job=>{
    const running=ACTIVE.includes(job.status);
    const percent=job.discovered?Math.min(100,Math.round(job.processed/job.discovered*100)):0;
    return <div className="task-row" key={job.id}>
     <div className="task-name">
      <b>导入 · {pageName(job.library)}</b>
      <i>{folderName(job.root)}</i>
     </div>
     <div className="task-path" title={displayPath(job.root)}>{displayPath(job.root)}</div>
     <div className={`task-bar${job.status==='completed'?' is-done':FAILED.includes(job.status)?' is-bad':''}`} aria-hidden="true"><i style={{width:`${percent}%`}}/></div>
     <div className="task-stat">{KIND[job.status]??job.status} · {job.processed}/{job.discovered} · 新增 {job.added}{job.failed?` · 失败 ${job.failed}`:''}</div>     <div className="task-acts">
      {running&&<button disabled={busy} onClick={()=>onCancel(job.id)}>取消</button>}
      {!running&&resumable(job)&&<button disabled={busy} onClick={()=>onResume(job)}>继续扫描</button>}
      <button disabled={busy} onClick={()=>onReveal(job.root)}>打开目录</button>
     </div>
    </div>;
   })}
  </div>
  {rows.some(job=>job.error)&&<p className="task-error" role="alert">{rows.find(job=>job.error)?.error}</p>}
 </section>;
}
