import {invoke} from '@tauri-apps/api/core';
import {visualMatches} from './spectral-state';
import type {VisualSnapshot,VisualData} from './spectral-view';
const abortError=()=>new DOMException('高级波形分析已取消','AbortError');
const wait=(ms:number,signal:AbortSignal)=>new Promise<void>((resolve,reject)=>{if(signal.aborted){reject(abortError());return;}const timer=setTimeout(()=>{signal.removeEventListener('abort',cancel);resolve();},ms);const cancel=()=>{clearTimeout(timer);signal.removeEventListener('abort',cancel);reject(abortError());};signal.addEventListener('abort',cancel,{once:true});});
export async function analyzeVisual(assetId:string,signal:AbortSignal,progress:(snapshot:VisualSnapshot)=>void):Promise<VisualData>{
 let jobId:string|null=null;const cancel=()=>{if(jobId)void invoke('visual_analysis_cancel',{jobId}).catch(()=>{});};signal.addEventListener('abort',cancel,{once:true});
 try{for(let attempt=0;attempt<12;attempt++){if(signal.aborted)throw abortError();try{jobId=await invoke<string>('visual_analysis_start',{id:assetId});break;}catch(error){if(attempt===11||!String(error).includes('正在取消'))throw error;await wait(150,signal);}}
 if(!jobId)throw new Error('高级波形任务未启动。');if(signal.aborted){cancel();throw abortError();}
 while(!signal.aborted){const snapshot=await invoke<VisualSnapshot>('visual_analysis_status',{jobId});if(signal.aborted)throw abortError();if(!visualMatches(snapshot,jobId,assetId))throw new Error('频谱任务或素材已变化，忽略旧结果。');progress(snapshot);if(snapshot.status==='completed'&&snapshot.result)return snapshot.result;if(['failed','cancelled'].includes(snapshot.status))throw new Error(snapshot.error??'高级波形分析已取消。');await wait(250,signal);}throw abortError();
 }finally{signal.removeEventListener('abort',cancel);}
}
