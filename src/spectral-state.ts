export type WaveMode='peaks'|'rms'|'spectrum'|'combined'|'frequency';
export const waveModes:{id:WaveMode;label:string}[]=[{id:'peaks',label:'峰值'},{id:'rms',label:'峰值＋RMS'},{id:'spectrum',label:'频谱图'},{id:'combined',label:'频谱＋波形'},{id:'frequency',label:'按频率着色'}];
export function safeWaveMode(raw:unknown):WaveMode{return waveModes.some(item=>item.id===raw)?raw as WaveMode:'peaks';}
export function visualMatches(snapshot:{job_id:string;asset_id:string},jobId:string,assetId:string){return snapshot.job_id===jobId&&snapshot.asset_id===assetId;}
export function heatColor(db:number){const value=Number.isFinite(db)?Math.max(0,Math.min(1,(db+100)/100)):0;const stops=[[5,9,15],[30,31,73],[84,48,113],[159,69,100],[223,133,82],[255,231,157]];const at=value*(stops.length-1),i=Math.min(stops.length-2,Math.floor(at)),fraction=at-i;return stops[i].map((v,c)=>Math.round(v+(stops[i+1][c]-v)*fraction));}
export function frequencyColor(hz:number,rate:number){const nyquist=Math.max(1,rate/2),low=Math.min(20,nyquist/2);const value=hz>0?Math.max(0,Math.min(1,Math.log(hz/low)/Math.log(nyquist/low))):0;return `hsl(${Math.round(205-value*170)} 68% 65%)`;}
