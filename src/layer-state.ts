export const LAYER_LIMIT=200;
export const LAYER_SECONDS=3600;
export type LayerClip={id:string;assetId:string;name:string;path:string;duration:number;offset:number;gain:number};
export type LayerAsset={id:string;name:string;path:string;duration:number|null;status?:string};
const finite=(value:unknown):value is number=>typeof value==='number'&&Number.isFinite(value);
const bound=(value:number,min:number,max:number)=>Math.max(min,Math.min(max,value));
export function safeLayer(raw:unknown):LayerClip[]{
 if(!Array.isArray(raw))return [];
 const seen=new Set<string>();const clips:LayerClip[]=[];
 for(const value of raw){
  if(!value||typeof value!=='object')continue;
  const clip=value as Partial<LayerClip>;
  if(typeof clip.id!=='string'||!clip.id||seen.has(clip.id)||typeof clip.assetId!=='string'||!clip.assetId||typeof clip.name!=='string'||typeof clip.path!=='string'||!finite(clip.duration)||clip.duration<=0||clip.duration>LAYER_SECONDS)continue;
  seen.add(clip.id);clips.push({id:clip.id,assetId:clip.assetId,name:clip.name.slice(0,1024),path:clip.path.slice(0,32768),duration:clip.duration,offset:finite(clip.offset)?bound(clip.offset,0,LAYER_SECONDS-clip.duration):0,gain:finite(clip.gain)?bound(clip.gain,0,2):1});
  if(clips.length>=LAYER_LIMIT)break;
 }
 return clips;
}
export function appendLayer(existing:LayerClip[],assets:LayerAsset[],scatter:boolean,makeId:()=>string):{clips:LayerClip[];added:number;skipped:number}{
 const clips=safeLayer(existing);let tail=clips.reduce((end,clip)=>Math.max(end,clip.offset+clip.duration),0),added=0;
 for(const asset of assets){
  if(clips.length>=LAYER_LIMIT||!asset.id||!asset.path||!finite(asset.duration)||asset.duration<=0||asset.duration>LAYER_SECONDS||(asset.status&&asset.status!=='ready'))continue;
  const offset=scatter?tail:0;
  if(offset+asset.duration>LAYER_SECONDS)continue;
  const id=makeId();if(clips.some(clip=>clip.id===id))continue;
  clips.push({id,assetId:asset.id,name:asset.name,path:asset.path,duration:asset.duration,offset,gain:1});added++;if(scatter)tail=offset+asset.duration;
 }
 return {clips,added,skipped:assets.length-added};
}
export function updateLayer(clips:LayerClip[],id:string,patch:{offset?:number;gain?:number}):LayerClip[]{
 return safeLayer(clips.map(clip=>clip.id===id?{...clip,...patch}:clip));
}

// Windows paths can arrive as canonical extended paths or display paths.
// A directory boundary is mandatory: D:/foo must never select D:/foo2.
export function layerPathKey(path:string){
 let key=path.replaceAll('\\','/');
 if(key.toLowerCase().startsWith('//?/unc/'))key='//'+key.slice(8);
 else if(key.startsWith('//?/'))key=key.slice(4);
 return key.toLowerCase().replace(/\/+$/,'');
}
export function pruneLayerState(clips:LayerClip[],selection:{ids?:string[];folder?:string}){
 const ids=new Set(selection.ids??[]),folder=selection.folder?layerPathKey(selection.folder):'';
 return clips.filter(clip=>!ids.has(clip.assetId)&&!(folder&&(layerPathKey(clip.path)===folder||layerPathKey(clip.path).startsWith(folder+'/'))));
}
export function refreshLayerMetadata(clips:LayerClip[],assets:{id:string;name:string;path:string}[]){
 const byId=new Map(assets.map(asset=>[asset.id,asset]));
 return clips.map(clip=>{const asset=byId.get(clip.assetId);return asset?{...clip,name:asset.name,path:asset.path}:clip;});
}
