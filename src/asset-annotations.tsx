import React,{useEffect,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import type {CatalogAsset} from './catalog-client';
import {BwfMetadataPanel} from './bwf-metadata';
export function AssetAnnotations({asset,onChanged}:{asset:CatalogAsset;onChanged:(patch:Partial<CatalogAsset>)=>void}){
 const [draft,setDraft]=useState(asset.tags.join('，')),[busy,setBusy]=useState(false),[error,setError]=useState('');
 useEffect(()=>{setDraft(asset.tags.join('，'));setError('');},[asset.id,asset.tags.join('\u0000')]);
 async function save(favorite:boolean|undefined){setBusy(true);setError('');try{
  if(favorite!==undefined){await invoke('catalog_set_favorite',{id:asset.id,favorite});onChanged({favorite});}
  else {const tags=[...new Set(draft.split(/[,，;；\n]/).map(t=>t.trim()).filter(Boolean))];await invoke('catalog_set_tags',{id:asset.id,tags});setDraft(tags.join('，'));onChanged({tags});}
 }catch(e){setError(String(e));}finally{setBusy(false);}}
 return <div className="annotations"><button disabled={busy} aria-pressed={asset.favorite} className={asset.favorite?'is-favorite':''} onClick={()=>void save(!asset.favorite)}>{asset.favorite?'★ 已收藏':'☆ 加入收藏'}</button><label>自定义标签<textarea aria-label="素材标签，逗号分隔" value={draft} disabled={busy} onChange={e=>setDraft(e.target.value)} placeholder="雨，森林，远雷" rows={2}/></label><button disabled={busy} onClick={()=>void save(undefined)}>保存标签</button><small>最多20个，每个40字；只写本机目录。</small>{error&&<p role="alert" className="asset-error">{error}</p>}<BwfMetadataPanel key={`bwf-${asset.id}`} assetId={asset.id} disabled={busy} onSaved={()=>onChanged({})}/></div>;
}
