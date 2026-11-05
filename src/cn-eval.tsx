import React,{useState} from 'react';
import {invoke} from '@tauri-apps/api/core';

/** 与 Rust `EvalRun` 对齐。 */
type Metrics={annotated:number;hit_at_1:number;hit_at_3:number;hit_at_5:number;hit_at_10:number;mrr:number};
type Detail={query:string;rank:number|null;annotated:boolean;top_paths:string[]};
type Run={library:string;limit:number;queries:number;unlabeled:number;stale:number;indexed:number;algorithm:string;metrics:Metrics;details:Detail[];note:string};

const pct=(value:number)=>`${(value*100).toFixed(1)}%`;

/**
 * 中文相关性评测面板：把评测集跑成 hit@k / MRR。
 * 只读本地已有索引；不自动重建索引、不改素材，未填期望的条目不进指标。
 */
export function CnEvalPanel(){
 const [path,setPath]=useState(''),[run,setRun]=useState<Run|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState('');
 async function start(){
  if(!path.trim()){setError('请填写评测集 JSON 的绝对路径。');return;}
  setBusy(true);setError('');
  try{setRun(await invoke<Run>('semantic_vector_eval',{path:path.trim()}));}
  catch(e){setError(String(e));setRun(null);}
  finally{setBusy(false);}
 }
 return <div className="cn-eval">
  <p>中文相关性评测：逐条检索并按评测集期望统计命中。评测集为 JSON（查询 + 期望文件名片段）；未填期望的条目不进入指标，避免用空标注凑命中率。</p>
  <label>评测集绝对路径<input value={path} disabled={busy} onChange={e=>setPath(e.target.value)} placeholder="D:/Ai-Files/…/cn-relevance-eval.json"/></label>
  <div className="intelligence-actions">
   <button disabled={busy||!path.trim()} onClick={()=>void start()}>{busy?'评测中…':'运行评测'}</button>
   <span role="status">{run?`${run.queries} 条查询 · 已标注 ${run.metrics.annotated} · 未标注 ${run.unlabeled} · 已索引 ${run.indexed}${run.stale?` · 跳过 ${run.stale}`:''}`:''}</span>
  </div>
  {error&&<p className="asset-error">{error}</p>}
  {run&&<>
   <dl className="cn-eval-metrics">
    <div><dt>hit@1</dt><dd>{pct(run.metrics.hit_at_1)}</dd></div>
    <div><dt>hit@3</dt><dd>{pct(run.metrics.hit_at_3)}</dd></div>
    <div><dt>hit@5</dt><dd>{pct(run.metrics.hit_at_5)}</dd></div>
    <div><dt>hit@10</dt><dd>{pct(run.metrics.hit_at_10)}</dd></div>
    <div><dt>MRR</dt><dd>{run.metrics.mrr.toFixed(3)}</dd></div>
   </dl>
   <ol className="cn-eval-list">
    {run.details.map((item,index)=><li key={`${index}-${item.query}`}><b>{item.query}</b> · {item.annotated?(item.rank?`第 ${item.rank} 位`:'未命中'):'未标注'} · {item.top_paths[0]??'—'}</li>)}
   </ol>
   <p className="cn-eval-note">{run.note}算法：{run.algorithm}，返回上限 {run.limit} 条。</p>
  </>}
 </div>;
}
