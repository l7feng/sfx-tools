export type WaveformAsset={id:string;size:number;modified:string};
export type WaveformData={peaks:number[];duration:number|null;analyzed_duration:number;truncated:boolean;channels?:number;channel_peaks?:number[][]};
type FetchWave=(id:string,simple:boolean)=>Promise<WaveformData>;
type Waiter={resolve:(wave:WaveformData)=>void;reject:(reason:unknown)=>void;signal?:AbortSignal;abort:()=>void};
type Job={key:string;id:string;simple:boolean;priority:number;waiters:Set<Waiter>};
export type WaveformOptions={simple?:boolean};
export const waveformKey=(asset:WaveformAsset)=>JSON.stringify([asset.id,asset.size,asset.modified]);
const abortError=()=>new Error('波形请求已取消');

/** One decoder at a time, shared across detail player and visible list rows. */
export function createWaveformScheduler(fetchWave:FetchWave,maxQueue=64,maxCache=64){
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
  void Promise.resolve().then(()=>fetchWave(job.id,job.simple)).then(wave=>{
   cache.set(job.key,wave);while(cache.size>maxCache)cache.delete(cache.keys().next().value!);
   settle(job,wave);
  },error=>settle(job,null,error)).finally(()=>{active=null;pump();});
 }
 function get(asset:WaveformAsset,priority=0,signal?:AbortSignal,options?:WaveformOptions):Promise<WaveformData>{
  if(signal?.aborted)return Promise.reject(abortError());
  // 简化/完整波形是两份不同的分析结果与两份磁盘缓存，绝不能共用同一个内存缓存键。
  const simple=options?.simple===true;
  const key=`${waveformKey(asset)}|${simple?'mini':'full'}`,hit=cache.get(key);
  if(hit){cache.delete(key);cache.set(key,hit);return Promise.resolve(hit);}
  let job=jobs.get(key);
  if(!job){
   if(queue.length>=maxQueue){
    const lowest=queue.reduce((a,b)=>a.priority<=b.priority?a:b);
    if(priority<=lowest.priority)return Promise.reject(new Error('波形队列繁忙，请稍后重试'));
    queue=queue.filter(item=>item!==lowest);settle(lowest,null,new Error('波形请求已让位于当前选中素材，请重试'));
   }
   job={key,id:asset.id,simple,priority,waiters:new Set()};jobs.set(key,job);queue.push(job);
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

const shared=createWaveformScheduler(async(id,simple)=>{
 const {invoke}=await import('@tauri-apps/api/core');
 return invoke<WaveformData>('player_waveform',{id,simple});
});
export type WaveformGetter=(asset:WaveformAsset,priority?:number,signal?:AbortSignal,options?:WaveformOptions)=>Promise<WaveformData>;
const RETRY_DELAYS=[120,240,400,600];
const TRANSIENT=[/繁忙/,/让位/,/稍后重试/,/分析锁不可用/];
/** 只有队列竞争类瞬时错误才重试；解码失败、文件损坏等永久错误必须立即上抛。 */
export function isTransientWaveformError(error:unknown){
 const message=String(error instanceof Error?error.message:error??'');
 return TRANSIENT.some(pattern=>pattern.test(message));
}
function delay(ms:number,signal?:AbortSignal){
 return new Promise<void>((resolve,reject)=>{
  if(signal?.aborted){reject(new Error('波形请求已取消'));return;}
  const timer=setTimeout(()=>{signal?.removeEventListener('abort',abort);resolve();},ms);
  function abort(){clearTimeout(timer);reject(new Error('波形请求已取消'));}
  signal?.addEventListener('abort',abort,{once:true});
 });
}
/** 队列满或高优先级让位不再直接落到界面成"波形暂不可用"，先有限次退避重试。 */
export function withWaveformRetry(base:WaveformGetter,delays:number[]=RETRY_DELAYS):WaveformGetter{
 return async(asset,priority=0,signal,options)=>{
  let last:unknown=new Error('波形暂不可用');
  for(let attempt=0;attempt<=delays.length;attempt++){
   if(signal?.aborted)throw new Error('波形请求已取消');
   try{return await base(asset,priority,signal,options);}
   catch(error){
    if(signal?.aborted)throw new Error('波形请求已取消');
    if(!isTransientWaveformError(error))throw error;
    last=error;
    if(attempt<delays.length)await delay(delays[attempt],signal);
   }
  }
  throw last;
 };
}
export const getWaveform=withWaveformRetry(shared.get);

export function fractionAt(clientX:number,left:number,width:number){return width>0?Math.max(0,Math.min(1,(clientX-left)/width)):0;}
export function waveformPosition(fraction:number,duration:number){return Number.isFinite(duration)&&duration>0?Math.max(0,Math.min(1,fraction))*duration:0;}
/** Mirrored peak envelope retains original bins (no random/synthetic waveform).
 *  lanes=1 占满高度；lanes=2 按上下两轨分区，每轨围绕自己的中线镜像，单声道不再冒充立体声。 */
export function envelopePath(peaks:readonly number[],lanes=1,lane=0){
 if(!peaks.length)return '';
 const height=100/Math.max(1,lanes),center=(lane+.5)*height;
 const amplitude=(value:number)=>(Number.isFinite(value)?Math.max(0,Math.min(1,value)):0)*(height/2-2);
 const x=(i:number)=>peaks.length===1?0:i/(peaks.length-1)*1000;
 const top=peaks.map((peak,i)=>`${i?'L':'M'}${x(i).toFixed(3)},${(center-amplitude(peak)).toFixed(3)}`).join(' ');
 const bottom=peaks.map((_,i)=>{const j=peaks.length-1-i;return `L${x(j).toFixed(3)},${(center+amplitude(peaks[j])).toFixed(3)}`;}).join(' ');
 return `${top} ${bottom} Z`;
}
