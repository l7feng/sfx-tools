import React,{useEffect,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {formatBytes,displayPath,type CatalogAsset} from './catalog-client';
import {BwfMetadataPanel,useBwfMeta} from './bwf-metadata';
import {timeLabel} from './demo-data';
import './asset-detail.css';

type Group='file'|'tags'|'meta'|'ai'|'path';
const GROUPS:Record<Group,boolean>={'file':true,'tags':true,'meta':false,'ai':false,'path':false};

/** 一行的标签 + 右侧对齐值；值过长时省略，悬停看全文。 */
function Row({label,value,mono}:{label:string;value:React.ReactNode;mono?:boolean}){
 return <div className={`detail-row${mono?' is-mono':''}`}><dt>{label}</dt><dd title={typeof value==='string'?value:undefined}>{value}</dd></div>;
}

function Group({id,title,open,onToggle,action,children}:{id:Group;title:string;open:boolean;onToggle:(id:Group)=>void;action?:React.ReactNode;children?:React.ReactNode}){
 return <section className={`detail-group detail-group-${id}`}>
  <div className="detail-group-head">
   <button className="detail-group-toggle" aria-expanded={open} aria-controls={`detail-${id}`} onClick={()=>onToggle(id)}>
    <span>{title}</span><span className="detail-chevron" aria-hidden="true">{open?'▾':'▸'}</span>
   </button>
   {action&&<div className="detail-group-action">{action}</div>}
  </div>
  {open&&<div className="detail-group-body" id={`detail-${id}`}>{children}</div>}
 </section>;
}

/**
 * 素材详情栏：文件信息 / 我的标签 / 内嵌元数据 / AI 分类标签 / 原始文件位置 五段，
 * 每段可折叠；标签与元数据各自带编辑入口，不编辑时不占垂直空间。
 */
export function AssetDetail({asset,onChanged,onCollapse}:{asset:CatalogAsset;onChanged:(patch:Partial<CatalogAsset>)=>void;onCollapse:()=>void}){
 const [open,setOpen]=useState<Record<Group,boolean>>(GROUPS);
 const [editing,setEditing]=useState(false);
 const [draft,setDraft]=useState(asset.tags.join('，'));
 const [busy,setBusy]=useState(false),[error,setError]=useState('');
 const {meta,metaError,reload}=useBwfMeta(asset.id);

 useEffect(()=>{setDraft(asset.tags.join('，'));setEditing(false);setError('');},[asset.id]);
 useEffect(()=>{if(!asset.id)return;setOpen(GROUPS);},[asset.id]);

 const toggle=(id:Group)=>setOpen(old=>({...old,[id]:!old[id]}));

 async function saveTags(){
  setBusy(true);setError('');
  try{
   const tags=[...new Set(draft.split(/[,，;；\n]/).map(tag=>tag.trim()).filter(Boolean))];
   await invoke('catalog_set_tags',{id:asset.id,tags});
   setDraft(tags.join('，'));onChanged({tags});setEditing(false);
  }catch(e){setError(String(e));}finally{setBusy(false);}
 }
 async function toggleFavorite(){
  setBusy(true);setError('');
  try{const favorite=!asset.favorite;await invoke('catalog_set_favorite',{id:asset.id,favorite});onChanged({favorite});}
  catch(e){setError(String(e));}finally{setBusy(false);}
 }

 const channels=asset.channels==null?'未知':asset.channels===1?'Mono':asset.channels===2?'Stereo':`${asset.channels} 声道`;
 const embedded=metaError?'读取失败':meta?(meta.has_bext||meta.has_ixml?[meta.has_bext?'bext':null,meta.has_ixml?'iXML':null].filter(Boolean).join(' · '):'暂无嵌入元数据'):'读取中…';

 return <div className="asset-detail">
  <div className="detail-title">
   <span>素材详情</span>
   <div className="detail-title-actions">
    <button className={`detail-star${asset.favorite?' is-on':''}`} disabled={busy} aria-pressed={asset.favorite} title={asset.favorite?'取消收藏':'加入收藏'} onClick={()=>void toggleFavorite()}>{asset.favorite?'★':'☆'}</button>
    <button onClick={onCollapse} aria-label="折叠右侧详情栏">›</button>
   </div>
  </div>
  <Group id="file" title="文件信息" open={open.file} onToggle={toggle}>
   <dl className="detail-rows">
    <Row label="文件名" value={asset.name}/>
    <Row label="格式" value={asset.format}/>
    <Row label="时长" value={asset.duration==null?'未知':timeLabel(asset.duration)}/>
    <Row label="采样率" value={asset.rate?`${asset.rate} Hz`:'未知'}/>
    <Row label="声道" value={channels}/>
    <Row label="UCS 分类" value={meta?.ucs?.trim()||'—'}/>
   </dl>
  </Group>

  <Group id="tags" title="我的标签" open={open.tags} onToggle={toggle} action={<button className="detail-edit" onClick={()=>{setEditing(value=>!value);setError('');}}>{editing?'取消':'编辑'}</button>}>
   {editing?<div className="detail-tag-editor">
     <textarea aria-label="素材标签，逗号分隔" rows={2} value={draft} disabled={busy} placeholder="雨，森林，远雷" onChange={event=>setDraft(event.target.value)}/>
     <div className="detail-tag-actions">
      <button disabled={busy} onClick={()=>void saveTags()}>{busy?'保存中…':'保存标签'}</button>
      <small>最多20个，每个40字；只写本机目录。</small>
     </div>
    </div>:<div className="detail-tags">{asset.tags.length?asset.tags.map(tag=><span key={tag} className="detail-tag">{tag}</span>):<p className="detail-empty">还没有标签</p>}</div>}
  </Group>

  <Group id="meta" title="内嵌元数据" open={open.meta} onToggle={toggle} action={open.meta?<span className="detail-meta-state">{embedded}</span>:<button className="detail-edit" onClick={()=>toggle('meta')}>编辑</button>}>
   <BwfMetadataPanel key={`bwf-${asset.id}`} assetId={asset.id} disabled={busy} injected={meta} injectedError={metaError} useInjected onSaved={()=>{reload();onChanged({});}}/>
  </Group>

  <Group id="ai" title="AI 分类标签" open={open.ai} onToggle={toggle}>
   <p className="detail-empty">无分类数据</p>
   <p className="detail-hint">音频内容语义分类尚未接入；当前只有按名称、路径与标签的检索。</p>
  </Group>

  <Group id="path" title="原始文件位置" open={open.path} onToggle={toggle}>
   <dl className="detail-rows">
    <Row label="路径" value={displayPath(asset.path)} mono/>
    <Row label="文件大小" value={formatBytes(asset.size)}/>
    <Row label="修改时间" value={asset.modified}/>
    <Row label="素材 ID" value={asset.id} mono/>
   </dl>
   <p className="detail-hint">素材库只保存路径引用，不会复制、移动或改写音频。</p>
  </Group>

  {error&&<p role="alert" className="asset-error">{error}</p>}
  {asset.error&&<p className="asset-error">{asset.error}</p>}
 </div>;
}
