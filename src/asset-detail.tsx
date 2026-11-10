import React,{useEffect,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {formatBytes,displayPath,type CatalogAsset} from './catalog-client';
import {BwfMetadataPanel,useBwfMeta} from './bwf-metadata';
import {timeLabel} from './demo-data';
import './asset-detail.css';

type Group='favorite'|'file'|'music'|'tags'|'meta'|'path';
const GROUPS:Record<Group,boolean>={'favorite':true,'file':true,'music':true,'tags':true,'meta':false,'path':false};
const STATUS_LABELS:Record<string,string>={ready:'可用',missing:'文件缺失',unavailable:'不可访问',changed:'已变化',error:'读取失败'};
/** 音乐信息全部落在本机标签里（维度:值），不改写原音频；曲风兼容早期「曲风:」前缀。 */
const MUSIC_FIELDS:{key:string;label:string;aliases?:string[]}[]=[
 {key:'艺术家',label:'艺术家'},{key:'专辑',label:'专辑'},{key:'BPM',label:'BPM'},
 {key:'调性',label:'调性'},{key:'年份',label:'年份'},{key:'流派',label:'曲风',aliases:['曲风']}
];
function musicFieldValue(tags:string[],field:{key:string;aliases?:string[]}){
 for(const key of [field.key,...(field.aliases??[])]){
  const found=tags.find(tag=>tag.startsWith(key+':'));
  if(found)return found.slice(key.length+1).trim();
 }
 return '';
}

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
 * 素材详情栏：顶部摘要卡 + 收藏与分组 / 我的标签 / 文件信息 / 音乐信息 / 内嵌元数据 / 原始文件位置。
 * 摘要只放识别素材要用的信息；技术细节与编辑入口按需展开，避免整栏从上到下全是键值行。
 */
export function AssetDetail({asset,library='sound',groups=[],onChanged,onCollapse}:{asset:CatalogAsset;library?:'sound'|'music';groups?:string[];onChanged:(patch:Partial<CatalogAsset>)=>void;onCollapse:()=>void}){
 const [open,setOpen]=useState<Record<Group,boolean>>(GROUPS);
 const [editing,setEditing]=useState(false);
 const [draft,setDraft]=useState(asset.tags.join('，'));
 const [editingMusic,setEditingMusic]=useState(false),[musicDraft,setMusicDraft]=useState<Record<string,string>>({});
 const [editingGroup,setEditingGroup]=useState(false),[groupSelect,setGroupSelect]=useState(asset.favorite_group??''),[newGroup,setNewGroup]=useState('');
 const [busy,setBusy]=useState(false),[error,setError]=useState(''),[notice,setNotice]=useState('');
 const {meta,metaError,reload}=useBwfMeta(asset.id);

 useEffect(()=>{setDraft(asset.tags.join('，'));setEditing(false);setEditingMusic(false);setEditingGroup(false);setError('');setNotice('');},[asset.id]);
 useEffect(()=>{if(!asset.id)return;setOpen(GROUPS);setGroupSelect(asset.favorite_group??'');setNewGroup('');},[asset.id]);

 const toggle=(id:Group)=>setOpen(old=>({...old,[id]:!old[id]}));
 async function copy(work:()=>Promise<unknown>,message:string){
  setError('');setNotice('');
  try{await work();setNotice(message);}catch(e){setError(String(e));}
 }
 async function saveTags(){
  setBusy(true);setError('');
  try{
   const tags=[...new Set(draft.split(/[,，;；\n]/).map(tag=>tag.trim()).filter(Boolean))];
   await invoke('catalog_set_tags',{id:asset.id,tags});
   setDraft(tags.join('，'));onChanged({tags});setEditing(false);
  }catch(e){setError(String(e));}finally{setBusy(false);}
 }
 function beginMusic(){setMusicDraft(Object.fromEntries(MUSIC_FIELDS.map(field=>[field.key,musicFieldValue(asset.tags,field)])));setEditingMusic(true);}
 async function saveMusic(){
  setBusy(true);setError('');
  try{
   const keys=MUSIC_FIELDS.flatMap(field=>[field.key,...(field.aliases??[])]);
   const kept=asset.tags.filter(tag=>!keys.some(key=>tag.startsWith(key+':')));
   const added=MUSIC_FIELDS.filter(field=>musicDraft[field.key]?.trim()).map(field=>`${field.key}:${musicDraft[field.key].trim()}`);
   const tags=[...new Set([...kept,...added])];
   await invoke('catalog_set_tags',{id:asset.id,tags});onChanged({tags});setEditingMusic(false);
  }catch(e){setError(String(e));}finally{setBusy(false);}
 }
 async function toggleFavorite(){
  setBusy(true);setError('');
  try{const favorite=!asset.favorite;await invoke('catalog_set_favorite',{id:asset.id,favorite});onChanged({favorite,favorite_group:null});}
  catch(e){setError(String(e));}finally{setBusy(false);}
 }
 function beginGroup(){setGroupSelect(asset.favorite_group??'');setNewGroup('');setEditingGroup(true);}
 async function saveGroup(){
  const name=(newGroup.trim()||groupSelect.trim())||null;
  if(name&&[...name].length>40){setError('收藏分组须为1至40个可见字符。');return;}
  setBusy(true);setError('');
  try{
   await invoke<number>('catalog_set_favorite_group',{ids:[asset.id],group:name});
   onChanged({favorite_group:name,favorite:name?true:asset.favorite});
   setEditingGroup(false);setNotice(name?`已放入收藏分组「${name}」`:'已清除收藏分组，收藏状态保留');
  }catch(e){setError(String(e));}finally{setBusy(false);}
 }

 const channels=asset.channels==null?'未知':asset.channels===1?'Mono':asset.channels===2?'Stereo':`${asset.channels} 声道`;
 const embedded=metaError?'读取失败':meta?(meta.has_bext||meta.has_ixml?[meta.has_bext?'bext':null,meta.has_ixml?'iXML':null].filter(Boolean).join(' · '):'暂无嵌入元数据'):'读取中…';
 const groupOptions=[...new Set([...(asset.favorite_group?[asset.favorite_group]:[]),...groups])].sort((a,b)=>a.localeCompare(b,'zh-Hans'));
 const status=asset.status&&asset.status!=='ready'?STATUS_LABELS[asset.status]??asset.status:'';

 return <div className="asset-detail">
  <div className="detail-title">
   <span>素材详情</span>
   <div className="detail-title-actions">
    <button className={`detail-star${asset.favorite?' is-on':''}`} disabled={busy} aria-pressed={asset.favorite} title={asset.favorite?'取消收藏':'加入收藏'} onClick={()=>void toggleFavorite()}>{asset.favorite?'★':'☆'}</button>
    <button onClick={onCollapse} aria-label="折叠右侧详情栏">›</button>
   </div>
  </div>

  <div className="detail-summary">
   <strong title={asset.name}>{asset.name}</strong>
   <div className="detail-badges">
    <span className="detail-badge">{asset.format}</span>
    <span className="detail-badge">{asset.duration==null?'时长未知':timeLabel(asset.duration)}</span>
    <span className="detail-badge">{channels}</span>
    {asset.rate&&<span className="detail-badge">{asset.rate} Hz</span>}
    {asset.favorite&&<span className="detail-badge is-favorite" title={asset.favorite_group?`收藏分组：${asset.favorite_group}`:'已收藏'}>{asset.favorite_group?'★ 收藏 · '+asset.favorite_group:'★ 已收藏'}</span>}
    {status&&<span className="detail-badge is-warning">{status}</span>}
   </div>
   <p className="detail-summary-path" title={displayPath(asset.path)}>{displayPath(asset.path)}</p>
  </div>

  <Group id="favorite" title="收藏与分组" open={open.favorite} onToggle={toggle} action={!editingGroup&&<button className="detail-edit" onClick={beginGroup}>{asset.favorite_group?'改分组':'设分组'}</button>}>
   {editingGroup?<div className="detail-group-editor">
     <label><span>已有分组</span><select aria-label="收藏分组" value={groupSelect} disabled={busy} onChange={event=>{setGroupSelect(event.target.value);setNewGroup('');}}><option value="">未分组</option>{groupOptions.map(name=><option key={name} value={name}>{name}</option>)}</select></label>
     <label><span>或新建</span><input aria-label="新建收藏分组名称" value={newGroup} maxLength={40} disabled={busy} placeholder="例如：对白、环境底噪" onChange={event=>setNewGroup(event.target.value)}/></label>
     <div className="detail-group-actions"><button disabled={busy} onClick={()=>void saveGroup()}>{busy?'保存中…':'保存分组'}</button><button disabled={busy} onClick={()=>setEditingGroup(false)}>取消</button><small>设分组会同时加入收藏；清除分组保留收藏。</small></div>
    </div>:<dl className="detail-rows">
     <Row label="收藏" value={asset.favorite?'已收藏':'未收藏'}/>
     <Row label="分组" value={asset.favorite_group||'未分组'}/>
    </dl>}
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

  <Group id="file" title="文件信息" open={open.file} onToggle={toggle}>
   <dl className="detail-rows">
    <Row label="格式" value={asset.format}/>
    <Row label="时长" value={asset.duration==null?'未知':timeLabel(asset.duration)}/>
    <Row label="采样率" value={asset.rate?`${asset.rate} Hz`:'未知'}/>
    <Row label="声道" value={channels}/>
    <Row label="UCS 分类" value={meta?.ucs?.trim()||'—'}/>
   </dl>
  </Group>

  {library==='music'&&<Group id="music" title="音乐信息" open={open.music} onToggle={toggle} action={<button className="detail-edit" onClick={()=>editingMusic?setEditingMusic(false):beginMusic()}>{editingMusic?'取消':'编辑'}</button>}>
   {editingMusic?<div className="detail-music-editor">
     {MUSIC_FIELDS.map(field=><label key={field.key}><span>{field.label}</span><input value={musicDraft[field.key]??''} maxLength={40} placeholder="未标注" disabled={busy} onChange={event=>setMusicDraft(old=>({...old,[field.key]:event.target.value}))}/></label>)}
     <div className="detail-tag-actions"><button disabled={busy} onClick={()=>void saveMusic()}>{busy?'保存中…':'保存音乐信息'}</button><small>写为本机标签，原音频不改写。</small></div>
    </div>:<dl className="detail-rows">{MUSIC_FIELDS.map(field=><Row key={field.key} label={field.label} value={musicFieldValue(asset.tags,field)||'—'}/>)}</dl>}
  </Group>}

  <Group id="meta" title="内嵌元数据" open={open.meta} onToggle={toggle} action={open.meta?<span className="detail-meta-state">{embedded}</span>:<button className="detail-edit" onClick={()=>toggle('meta')}>查看</button>}>
   <BwfMetadataPanel key={`bwf-${asset.id}`} assetId={asset.id} disabled={busy} injected={meta} injectedError={metaError} useInjected onSaved={()=>{reload();onChanged({});}}/>
  </Group>

  <Group id="path" title="原始文件位置" open={open.path} onToggle={toggle}>
   <dl className="detail-rows">
    <Row label="路径" value={displayPath(asset.path)} mono/>
    <Row label="文件大小" value={formatBytes(asset.size)}/>
    <Row label="修改时间" value={asset.modified}/>
    <Row label="素材 ID" value={asset.id} mono/>
   </dl>
   <div className="detail-path-actions">
    <button disabled={busy} onClick={()=>void copy(()=>invoke('source_copy',{ids:[asset.id]}),'已复制完整路径')}>复制路径</button>
    <button disabled={busy} onClick={()=>void copy(()=>invoke('source_copy',{ids:[asset.id],namesOnly:true}),'已复制文件名')}>复制文件名</button>
    <button disabled={busy} onClick={()=>void copy(()=>invoke('source_reveal',{id:asset.id}),'已在资源管理器中定位')}>在资源管理器中显示</button>
   </div>
   <p className="detail-hint">素材库只保存路径引用，不会复制、移动或改写音频。</p>
  </Group>

  {notice&&<p className="detail-notice" role="status">{notice}</p>}
  {error&&<p role="alert" className="asset-error">{error}</p>}
  {asset.error&&<p className="asset-error">{asset.error}</p>}
 </div>;
}
