import type {WaveformData,WaveformAsset} from './waveform-data';
export type FullWaveSnapshot={job_id:string;asset_id:string;status:string;progress:number|null;analyzed_seconds:number;error:string|null;wave:WaveformData|null};
export function fullWaveMatches(snapshot:FullWaveSnapshot,assetId:string,jobId:string){return snapshot.asset_id===assetId&&snapshot.job_id===jobId;}
const aborted=()=>new DOMException('完整波形分析已取消','AbortError');
function wait(milliseconds:number,signal:AbortSignal){return new Promise<void>((resolve,reject)=>{if(signal.aborted){reject(aborted());return;}const finish=()=>{signal.removeEventListener('abort',cancel);resolve();};const timer=setTimeout(finish,milliseconds);const cancel=()=>{clearTimeout(timer);signal.removeEventListener('abort',cancel);reject(aborted());};signal.addEventListener('abort',cancel,{once:true});});}
export async function getFullWaveform(asset:WaveformAsset,signal:AbortSignal,onProgress:(snapshot:FullWaveSnapshot)=>void):Promise<{wave:WaveformData;jobId:string}>{
 const {invoke}=await import('@tauri-apps/api/core');let jobId:string|null=null;
 const cancel=()=>{if(jobId)void invoke('full_wave_cancel',{jobId}).catch(()=>{});};signal.addEventListener('abort',cancel,{once:true});
 try{
  for(let attempt=0;attempt<10;attempt++){if(signal.aborted)throw aborted();try{jobId=await invoke<string>('full_wave_start',{id:asset.id});break;}catch(error){if(attempt>=9||!String(error).includes('正在取消'))throw error;await wait(150,signal);}}
  if(!jobId)throw new Error('完整波形任务未能启动。');if(signal.aborted){cancel();throw aborted();}
  while(!signal.aborted){const snapshot=await invoke<FullWaveSnapshot>('full_wave_status',{jobId});if(signal.aborted)throw aborted();if(!fullWaveMatches(snapshot,asset.id,jobId))throw new Error('波形任务与当前素材不一致，已忽略结果。');onProgress(snapshot);
   if(snapshot.status==='completed'&&snapshot.wave)return {wave:snapshot.wave,jobId};
   if(['failed','cancelled'].includes(snapshot.status))throw new Error(snapshot.error??'完整波形分析已取消。');await wait(250,signal);
  }throw aborted();
 }finally{signal.removeEventListener('abort',cancel);}
}
