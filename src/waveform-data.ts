export type WaveformAsset={id:string;size:number;modified:string};
export type WaveformData={peaks:number[];duration:number|null;analyzed_duration:number;truncated:boolean};
type FetchWave=(id:string)=>Promise<WaveformData>;
type Waiter={resolve:(wave:WaveformData)=>void;reject:(reason:unknown)=>void;signal?:AbortSignal;abort:()=>void};
type Job={key:string;id:string;priority:number;waiters:Set<Waiter>};
export const waveformKey=(asset:WaveformAsset)=>JSON.stringify([asset.id,asset.size,asset.modified]);
const abortError=()=>new Error('波形请求已取消');

/** One decoder at a time, shared across detail player and visible list rows. */
export function createWaveformScheduler(fetchWave:FetchWave,maxQueue=24,maxCache=64){
 const cache=new Map<string,WaveformData>(),jobs=new Map<string,Job>();
 let queue:Job[]=[],active:Job|null=null;
 function settle(job:Job,wave:WaveformData|null,error?:unknown){
  for(const waiter of job.waiters){waiter.signal?.removeEventListener('abort',waiter.abort);if(error!==undefined)waiter.reject(error);else waiter.resolve(wave!);}
  job.waiters.clear();jobs.delete(job.key);
 }
 function pump(){
  if(active)return;
  queue=queue.filter(job=>job.waiters.size>0);
  queue.sort((a,b)=>b.priority-a.priority);
  const job=queue.shift();if(!job)return;active=job;
  void Promise.resolve().then(()=>fetchWave(job.id)).then(wave=>{
   cache.set(job.key,wave);while(cache.size>maxCache)cache.delete(cache.keys().next().value!);
   settle(job,wave);
  },error=>settle(job,null,error)).finally(()=>{active=null;pump();});
 }
 function get(asset:WaveformAsset,priority=0,signal?:AbortSignal):Promise<WaveformData>{
  if(signal?.aborted)return Promise.reject(abortError());
  const key=waveformKey(asset),hit=cache.get(key);
  if(hit){cache.delete(key);cache.set(key,hit);return Promise.resolve(hit);}
  let job=jobs.get(key);
  if(!job){
   if(queue.length>=maxQueue){
    const lowest=queue.reduce((a,b)=>a.priority<=b.priority?a:b);
    if(priority<=lowest.priority)return Promise.reject(new Error('波形队列繁忙，请稍后重试'));
    queue=queue.filter(item=>item!==lowest);settle(lowest,null,new Error('波形请求已让位于当前选中素材，请重试'));
   }
   job={key,id:asset.id,priority,waiters:new Set()};jobs.set(key,job);queue.push(job);
  }else job.priority=Math.max(job.priority,priority);
  const current=job;
  const promise=new Promise<WaveformData>((resolve,reject)=>{
   const waiter:Waiter={resolve,reject,signal,abort:()=>{
    current.waiters.delete(waiter);signal?.removeEventListener('abort',waiter.abort);reject(abortError());
    if(current!==active&&current.waiters.size===0){queue=queue.filter(item=>item!==current);jobs.delete(current.key);}
   }};
   current.waiters.add(waiter);signal?.addEventListener('abort',waiter.abort,{once:true});
  });
  pump();return promise;
 }
 return {get};
}

const shared=createWaveformScheduler(async id=>{
 const {invoke}=await import('@tauri-apps/api/core');
 return invoke<WaveformData>('player_waveform',{id});
});
export const getWaveform=shared.get;

export function fractionAt(clientX:number,left:number,width:number){return width>0?Math.max(0,Math.min(1,(clientX-left)/width)):0;}
export function waveformPosition(fraction:number,duration:number){return Number.isFinite(duration)&&duration>0?Math.max(0,Math.min(1,fraction))*duration:0;}
/** Mirrored peak envelope retains original bins (no random/synthetic waveform). */
export function envelopePath(peaks:readonly number[]){
 if(!peaks.length)return '';
 const amplitude=(value:number)=>Number.isFinite(value)?Math.max(0,Math.min(1,value))*48:0;
 const x=(i:number)=>peaks.length===1?0:i/(peaks.length-1)*1000;
 const top=peaks.map((peak,i)=>`${i?'L':'M'}${x(i).toFixed(3)},${(50-amplitude(peak)).toFixed(3)}`).join(' ');
 const bottom=peaks.map((_,i)=>{const j=peaks.length-1-i;return `L${x(j).toFixed(3)},${(50+amplitude(peaks[j])).toFixed(3)}`;}).join(' ');
 return `${top} ${bottom} Z`;
}
