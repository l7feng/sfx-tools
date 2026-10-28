import {useEffect,useMemo,useState} from 'react';
export type PlaybackClockSnapshot={id:string|null;state:string;position:number;duration:number|null};
export const PLAYBACK_CLOCK_MAX_AHEAD_MS=500;
/** 只插值显示位置，不生成或改变后端播放状态。 */
export function playbackPositionAt(snapshot:PlaybackClockSnapshot,elapsedMs:number):number{
 const base=Number.isFinite(snapshot.position)?Math.max(0,snapshot.position):0;
 const duration=typeof snapshot.duration==='number'&&Number.isFinite(snapshot.duration)&&snapshot.duration>=0?snapshot.duration:null;
 const elapsed=Number.isFinite(elapsedMs)?Math.max(0,Math.min(PLAYBACK_CLOCK_MAX_AHEAD_MS,elapsedMs))/1000:0;
 const position=base+(snapshot.id&&snapshot.state==='playing'?elapsed:0);
 return duration===null?position:Math.min(duration,position);
}
export function usePlaybackClock(snapshot:PlaybackClockSnapshot):number{
 // 标量相同的轮询不重新起钟：后台停在原位时最多插值0.5秒，随后停住。
 const anchor=useMemo(()=>({snapshot:{id:snapshot.id,state:snapshot.state,position:snapshot.position,duration:snapshot.duration}}),[snapshot.id,snapshot.state,snapshot.position,snapshot.duration]);
 const [frame,setFrame]=useState(()=>({anchor,position:playbackPositionAt(anchor.snapshot,0)}));
 useEffect(()=>{
  let live=true,request:number|null=null;
  setFrame({anchor,position:playbackPositionAt(anchor.snapshot,0)});
  if(!anchor.snapshot.id||anchor.snapshot.state!=='playing'||typeof requestAnimationFrame!=='function')return()=>{live=false;};
  const started=performance.now();
  const tick=(now:number)=>{
   if(!live)return;
   const elapsed=Math.max(0,now-started);
   setFrame({anchor,position:playbackPositionAt(anchor.snapshot,elapsed)});
   if(elapsed<PLAYBACK_CLOCK_MAX_AHEAD_MS)request=requestAnimationFrame(tick);
  };
  request=requestAnimationFrame(tick);
  return()=>{live=false;if(request!==null)cancelAnimationFrame(request);};
 },[anchor]);
 // effect运行前也不把上一源/暂停前的插值位置显示一帧。
 return frame.anchor===anchor?frame.position:playbackPositionAt(anchor.snapshot,0);
}
