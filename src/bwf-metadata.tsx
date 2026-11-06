import React,{useEffect,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';

/** 与 Rust `BwfMetadata` 对齐；缺失字段一律是空串或 0。 */
export type Meta={file_size:number;has_bext:boolean;has_ixml:boolean;chunk_ids:string[];description:string;originator:string;originator_reference:string;origination_date:string;origination_time:string;time_reference:number;project:string;scene:string;take:string;tape:string;note:string;category:string;keywords:string;license:string;creator:string;source:string;ucs:string};
type Fields={description:string;note:string;category:string;keywords:string;ucs:string;license:string;creator:string;source:string;project:string};
const EMPTY:Fields={description:'',note:'',category:'',keywords:'',ucs:'',license:'',creator:'',source:'',project:''};
const FIELDS:[keyof Fields,string][]=[['description','bext 描述'],['note','iXML 备注'],['category','分类'],['keywords','关键词'],['ucs','UCS 标识'],['license','许可证'],['creator','作者 / 厂牌'],['source','来源'],['project','工程 / 剧集']];

function toFields(meta:Meta):Fields{return {description:meta.description,note:meta.note,category:meta.category,keywords:meta.keywords,ucs:meta.ucs,license:meta.license,creator:meta.creator,source:meta.source,project:meta.project};}

/** 详情栏与元数据面板共用一次读取：传 null 表示不自己读，由上层注入。 */
export function useBwfMeta(assetId:string|null|undefined){
 const [meta,setMeta]=useState<Meta|null>(null),[metaError,setMetaError]=useState(''),[revision,setRevision]=useState(0);
 useEffect(()=>{
  setMeta(null);setMetaError('');
  if(!assetId)return;
  let alive=true;
  invoke<Meta>('bwf_read_metadata',{id:assetId}).then(value=>{if(alive)setMeta(value);},e=>{if(alive)setMetaError(String(e));});
  return()=>{alive=false;};
 },[assetId,revision]);
 return {meta,metaError,reload:()=>setRevision(value=>value+1)};
}

/**
 * BWF / iXML 元数据面板：读写随文件走的生产信息。
 * 只支持 WAV/BWF；其他格式由后端直接报错，这里如实显示而不是隐藏。
 * `useInjected` 为真时改用上层传入的 meta，避免同一素材被读两遍。
 */
export function BwfMetadataPanel({assetId,onSaved,disabled,injected,useInjected,injectedError}:{assetId:string;onSaved:()=>void;disabled?:boolean;injected?:Meta|null;useInjected?:boolean;injectedError?:string}){
 const own=useBwfMeta(useInjected?null:assetId);
 const [current,setCurrent]=useState<Meta|null>(null),[fields,setFields]=useState<Fields>(EMPTY),[busy,setBusy]=useState(false),[error,setError]=useState(''),[message,setMessage]=useState('');
 const meta=current??(useInjected?injected??null:own.meta);
 const readError=useInjected?(injectedError??''):own.metaError;
 useEffect(()=>{setCurrent(null);setError('');setMessage('');},[assetId]);
 useEffect(()=>{if(meta)setFields(toFields(meta));},[meta]);

 async function save(){
  if(!meta)return;
  setBusy(true);setError('');setMessage('');
  try{
   const patch:Record<string,string>={};
   for(const [key] of FIELDS){const next=fields[key];const now=(meta[key] as string)??'';if(next!==now)patch[key]=next;}
   if(Object.keys(patch).length===0){setMessage('没有改动。');return;}
   const report=await invoke<{bytes_before:number;bytes_after:number;metadata:Meta}>('bwf_write_metadata',{id:assetId,patch});
   setCurrent(report.metadata);setFields(toFields(report.metadata));
   setMessage(`已写回文件（${report.bytes_before} → ${report.bytes_after} 字节）；目录需重扫后刷新大小与修改时间。`);
   onSaved();
  }catch(e){setError(String(e));}finally{setBusy(false);}
 }

 if(readError||error)return <div className="bwf-body"><p className="description">{readError||error}</p><p className="bwf-hint">内嵌元数据只适用于 WAV/BWF；MP3、FLAC、AIFF 的原文件不写入标签。</p></div>;
 if(!meta)return <div className="bwf-body"><p className="description">读取中…</p></div>;

 return <div className="bwf-body">
  <div className="bwf-grid">
   {FIELDS.map(([key,label])=><label key={key}>{label}
    <input value={fields[key]} disabled={busy||disabled} maxLength={key==='description'?256:200} onChange={event=>setFields(old=>({...old,[key]:event.target.value}))}/>
   </label>)}
  </div>
  <div className="bwf-actions">
   <button disabled={busy||disabled} onClick={()=>void save()}>{busy?'写入中…':'写回文件'}</button>
   <button disabled={busy||disabled} onClick={()=>{setFields(toFields(meta));setError('');setMessage('');}}>撤销改动</button>
   <span role="status">{message}</span>
  </div>
  <p className="bwf-hint">写入顺序：同目录临时文件 → 回读校验 → 原子替换；失败时原文件保持不变。bext 单字段超长会报错而不截断，音频数据不重编码。</p>
 </div>;
}
