import React,{useEffect,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {ContextMenu,type MenuItem} from './context-menu';
import {displayPath,formatBytes,type CatalogAsset} from './catalog-client';
import {pruneLayer,refreshLayerAssets,getLayerAssetIds} from './layer-tools';
export type ContextTarget={x:number;y:number;folder?:string;assets?:CatalogAsset[]};
export type LibraryOverview={tags:{name:string;count:number}[];playlists:{id:string;name:string;count:number}[];pins:string[];history_count:number};
type Edit={kind:string;target:ContextTarget};
type SourcePreview={token:string;action:string;path:string;destination:string|null;files:number;bytes:number;catalog_items:number;unindexed_files:number;warning:string};
type RemovePreview={token:string;directories:number;assets:number;bytes:number;favorites:number;tags:number;notes:number;playlists:number;history:number;paths:string[]};
const leaf=(path:string)=>displayPath(path).split(/[\\/]/).filter(Boolean).at(-1)??path;
export function CatalogContext({target,library,overview,onClose,onChanged,notify,onSimilar,onLayer}:{target:ContextTarget|null;library:'sound'|'music';overview:LibraryOverview;onClose:()=>void;onChanged:(clearSelection?:boolean)=>void;notify:(message:string)=>void;onSimilar:(asset:CatalogAsset)=>void;onLayer:(assets:CatalogAsset[],scatter:boolean)=>void}){
 const [edit,setEdit]=useState<Edit|null>(null),[value,setValue]=useState(''),[playlist,setPlaylist]=useState(''),[preview,setPreview]=useState<SourcePreview|null>(null),[removal,setRemoval]=useState<RemovePreview|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState('');
 const dialog=useRef<HTMLDialogElement>(null),lock=useRef(false),revision=useRef(0);
 const ids=(t:ContextTarget)=>(t.assets??[]).map(a=>a.id);
 const sourceArgs=(t:ContextTarget)=>t.folder?{folder:t.folder}:{id:t.assets?.[0]?.id};
 async function run(work:()=>Promise<void>){if(lock.current)return;lock.current=true;setBusy(true);setError('');try{await work();}catch(e){setError(String(e));}finally{lock.current=false;setBusy(false);}}
 useEffect(()=>{const element=dialog.current;if(edit&&!element?.open)element?.showModal();else if(!edit&&element?.open)element.close();},[edit]);
 const close=()=>{if(lock.current)return;revision.current++;setEdit(null);};
 const begin=(kind:string,t:ContextTarget)=>{revision.current++;setError('');setPreview(null);setRemoval(null);setPlaylist('');setValue(kind==='rename'?leaf(t.folder??t.assets?.[0]?.path??''):kind==='tags'&&t.assets?.length===1?t.assets[0].tags.join('，'):'');setEdit({kind,target:t});};
 const exec=(work:()=>Promise<void>)=>{void run(work);};
 const prepare=()=>exec(async()=>{if(!edit)return;const version=revision.current;
  if(edit.kind==='remove'){const result=await invoke<RemovePreview>('catalog_remove_preview',edit.target.folder?{folder:edit.target.folder}:{ids:ids(edit.target)});if(version===revision.current)setRemoval(result);}
  else {const result=await invoke<SourcePreview>('source_preview',{action:edit.kind,...sourceArgs(edit.target),value:value.trim()||null,library});if(version===revision.current)setPreview(result);}
 });
 const finish=(clearSelection=false)=>{setEdit(null);onChanged(clearSelection);};
 const confirm=()=>exec(async()=>{
  if(removal){await invoke('player_control',{action:'stop',value:null});const count=await invoke<number>('catalog_remove_commit',{token:removal.token});pruneLayer(edit?.target.folder?{folder:edit.target.folder}:{ids:edit?ids(edit.target):[]});notify(`已从两库移除 ${count} 条素材及关联；磁盘文件保留`);finish(true);return;}
  if(preview){await invoke('player_control',{action:'stop',value:null});const result=await invoke<{message:string;path:string|null}>('source_commit',{token:preview.token});let message=result.message;if(preview.action==='trash')pruneLayer({folder:preview.path});else {const layerIds=getLayerAssetIds();if(layerIds.length)try{for(let offset=0;offset<layerIds.length;offset+=100)refreshLayerAssets(await invoke<CatalogAsset[]>('catalog_get_items',{ids:layerIds.slice(offset,offset+100)}));}catch{message+='；叠层路径刷新失败，请重新打开工具页核对。';}}notify(message);finish(true);}
 });
 const items:MenuItem[]=[];
 if(target?.folder){const t=target,path=t.folder!,pinned=overview.pins.includes(path);items.push(
  {label:pinned?'取消常用置顶':'设为常用并置顶',icon:'⌖',run:()=>exec(async()=>{await invoke('library_pin',{library,path,pinned:!pinned});onChanged();notify(pinned?'已移除常用路径':'已添加常用路径');})},
  {label:'重新扫描此文件夹',icon:'↻',run:()=>exec(async()=>{await invoke('catalog_start',{root:path,library});onChanged();notify('正在重新扫描此文件夹');})},
  {label:'新建子文件夹…',separator:true,run:()=>begin('mkdir',t)},
  {label:'重命名此文件夹…',icon:'✎',run:()=>begin('rename',t)},
  {label:'移动文件夹…',run:()=>begin('move',t)},
  {label:'在资源管理器中显示',icon:'▱',separator:true,run:()=>exec(async()=>{await invoke('source_reveal',{folder:path});})},
  {label:'复制文件路径',icon:'⧉',run:()=>exec(async()=>{await invoke('source_copy',{folder:path});notify('已复制文件夹路径');})},
  {label:'从库中移除此文件夹',icon:'⌫',danger:true,separator:true,detail:'清除所选素材的收藏、标签、备注和播放列表关联，已清除关联无法恢复。磁盘文件保留。',run:()=>begin('remove',t)},
  {label:'删除整个磁盘文件夹',icon:'⌫',danger:true,detail:'包含未入库文件。移入系统回收站；不支持时停止，不自动永久删除。',run:()=>begin('trash',t)}
 );}
 else if(target?.assets?.length){const t=target,a=t.assets![0],many=t.assets!.length>1;items.push(
  {label:'查找相似音效',icon:'♧',disabled:many,detail:'依据实际音频声学特征，需先建立索引',run:()=>onSimilar(a)},
  {label:!many&&a.favorite?'取消收藏':'收藏/取消收藏',icon:'♡',separator:true,run:()=>exec(async()=>{const favorite=!t.assets!.every(a=>a.favorite);for(const asset of t.assets!)await invoke('catalog_set_favorite',{id:asset.id,favorite});onChanged();notify(favorite?'已收藏所选素材':'已取消收藏');})},
  {label:'添加到播放列表',icon:'＋',run:()=>begin('playlist',t)},
  {label:'编辑标签',icon:'◇',run:()=>begin('tags',t)},
  {label:'添加到叠层',icon:'▱',separator:true,run:()=>onLayer(t.assets!,false)},
  {label:'铺设到叠层（散布）',icon:'⁙',run:()=>onLayer(t.assets!,true)},
  {label:'播放',icon:'▶',disabled:many||a.status!=='ready',run:()=>exec(async()=>{await invoke('player_load',{id:a.id});notify(`正在播放：${a.name}`);})},
  {label:'复制文件名',icon:'⧉',separator:true,run:()=>exec(async()=>{await invoke('source_copy',{ids:ids(t),namesOnly:true});notify('已复制文件名');})},
  {label:'复制文件路径',icon:'⧉',run:()=>exec(async()=>{await invoke('source_copy',{ids:ids(t)});notify('已复制文件路径');})},
  {label:'在资源管理器中显示',icon:'▱',separator:true,disabled:many,run:()=>exec(async()=>{await invoke('source_reveal',{id:a.id});})},
  {label:'重命名…',icon:'✎',disabled:many,run:()=>begin('rename',t)},
  {label:'重新找到这个素材…',icon:'⌕',disabled:many,run:()=>begin('relocate',t)},
  {label:'移动到文件夹…',disabled:many,run:()=>begin('move',t)},
  {label:'从库中删除…',icon:'⌫',danger:true,separator:true,run:()=>begin('remove',t)}
 );}
 const titles:Record<string,string>={mkdir:'新建子文件夹',rename:'重命名',move:'移动到文件夹',trash:'删除整个磁盘文件夹',remove:'从库中移除',tags:'编辑标签',playlist:'添加到播放列表',relocate:'重新找到这个素材'};
 const t=edit?.target,source=t?.folder??t?.assets?.[0]?.path??'';
 return <>
 {target&&<ContextMenu x={target.x} y={target.y} title={target.folder?displayPath(target.folder):target.assets?.length===1?target.assets[0].name:`已选 ${target.assets?.length??0} 条素材`} items={items} onClose={onClose}/>}
 {!edit&&error&&<div className="context-operation-error" role="alert">{error}<button onClick={()=>setError('')}>关闭</button></div>}
 <dialog ref={dialog} className="operation-dialog" onCancel={event=>{event.preventDefault();close();}} onClose={()=>{if(!lock.current)setEdit(null);}} aria-labelledby="operation-title"><div className="dialog-heading"><h2 id="operation-title">{titles[edit?.kind??'']}</h2><button disabled={busy} onClick={close} aria-label="关闭操作">×</button></div>
 <p className="operation-path">{displayPath(source)}{(t?.assets?.length??0)>1?` 等 ${t!.assets!.length} 条`:''}</p>
 {edit&&['mkdir','rename','move','relocate','tags','playlist'].includes(edit.kind)&&<>
 {edit.kind==='playlist'&&<label>已有播放列表<select value={playlist} disabled={busy} onChange={e=>setPlaylist(e.target.value)}><option value="">新建播放列表…</option>{overview.playlists.map(item=><option key={item.id} value={item.id}>{item.name}</option>)}</select></label>}
 {(!playlist||edit.kind!=='playlist')&&<label>{({mkdir:'子文件夹名称',rename:'新名称（文件需保留扩展名）',move:'目标父文件夹完整路径',relocate:'素材现在的完整文件路径',tags:'标签（逗号分隔，将替换所选素材标签）',playlist:'新播放列表名称'} as Record<string,string>)[edit.kind]}<input value={value} autoComplete="off" disabled={busy} onChange={e=>{setValue(e.target.value);setPreview(null);}}/></label>}
 {edit.kind==='move'&&<button disabled={busy} onClick={()=>exec(async()=>{const path=await invoke<string|null>('catalog_pick_folder',{initial:value||source||null});if(path){setValue(displayPath(path));setPreview(null);}})}>选择目标文件夹…</button>}
 {edit.kind==='relocate'&&<><p>关联新的路径前会核验已保存的内容指纹，不会按文件名认领其他音频。</p><button disabled={busy} onClick={()=>exec(async()=>{await invoke('catalog_prepare_fingerprint',{id:t?.assets?.[0]?.id});notify('已登记原文件指纹');})}>原文件还在：登记指纹</button></>}
 </>}
 {edit?.kind==='remove'&&!removal&&<p className="danger-copy">出库将移除两库中的同一素材及收藏、标签、备注、播放列表和试听历史关联。已清除关联无法恢复；磁盘文件保留。</p>}
 {edit?.kind==='trash'&&!preview&&<p className="danger-copy">将删除整个磁盘文件夹，包含未入库文件。先核对完整清单；只使用系统回收站，回收站不可用时停止。</p>}
 {removal&&<div className="operation-preview"><strong>确认从两库出库 {removal.assets} 条 · 引用文件 {formatBytes(removal.bytes)} · 目录登记 {removal.directories}</strong><p>收藏 {removal.favorites}，有标签素材 {removal.tags}，备注 {removal.notes}，播放列表关联 {removal.playlists}，历史关联 {removal.history}。</p><p>上述软件关联将清除且不可恢复。磁盘文件保留，重新添加会作为重新入库。</p><details><summary>所选素材路径（最多显示前100条，影响统计覆盖全部）</summary>{removal.paths.map(path=><p key={path}>{displayPath(path)}</p>)}</details></div>}
 {preview&&<div className="operation-preview"><strong>{displayPath(preview.path)}</strong>{preview.destination&&<p>→ {displayPath(preview.destination)}</p>}<p>{preview.files} 个文件 · {formatBytes(preview.bytes)} · 已登记 {preview.catalog_items} 条 · 未入库 {preview.unindexed_files} 个</p><p>{preview.warning}</p></div>}
 {error&&<p className="asset-error" role="alert">{error}</p>}
 <div className="dialog-actions"><button disabled={busy} onClick={close}>取消</button>{(preview||removal)&&<button disabled={busy} onClick={prepare}>重新检查</button>}{edit&&['tags','playlist','relocate'].includes(edit.kind)?<button className="primary" disabled={busy||(!value.trim()&&edit.kind!=='tags'&&!playlist)} onClick={()=>exec(async()=>{
  if(edit.kind==='tags'){const tags=[...new Set(value.split(/[,，;；\n]/).map(t=>t.trim()).filter(Boolean))];for(const id of ids(edit.target))await invoke('catalog_set_tags',{id,tags});notify('已保存标签');}
  else if(edit.kind==='playlist'){const id=playlist||await invoke<string>('library_playlist_create',{library,name:value.trim()});await invoke('library_playlist_add',{playlistId:id,ids:ids(edit.target)});notify('已加入播放列表');}
  else {await invoke('catalog_relocate',{id:edit.target.assets?.[0]?.id,newPath:value.trim()});notify('内容核验通过，素材已重新关联');}
  finish();
 })}>{busy?'处理中…':'保存'}</button>:preview||removal?<button className={edit?.kind==='trash'||edit?.kind==='remove'?'danger-button':'primary'} disabled={busy} onClick={confirm}>{busy?'处理中…':'确认执行'}</button>:<button className="primary" disabled={busy||!!edit&&['rename','move','mkdir'].includes(edit.kind)&&!value.trim()} onClick={prepare}>{busy?'检查中…':'预览影响'}</button>}</div>
 </dialog></>;
}
