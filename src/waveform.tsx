import React,{useEffect,useMemo,useRef,useState} from 'react';
import {invoke,isTauri} from '@tauri-apps/api/core';
import type {CatalogAsset} from './catalog-client';
import {envelopePath,fractionAt,getWaveform,waveformKey,waveformPosition} from './waveform-data';
import type {WaveformData} from './waveform-data';

export function Envelope({peaks,progress=0,cursor=null,className=''}:{peaks:number[];progress?:number;cursor?:number|null;className?:string}){
 const path=useMemo(()=>envelopePath(peaks),[peaks]);
 const bound=(value:number)=>Math.max(0,Math.min(1,Number.isFinite(value)?value:0))*1000;
 return <svg className={`wave-envelope ${className}`} viewBox="0 0 1000 100" preserveAspectRatio="none" aria-hidden="true">
  <path className="wave-envelope-shape" d={path} fill="currentColor"/>
  {progress>0&&<line className="wave-envelope-progress" x1={bound(progress)} x2={bound(progress)} y1="0" y2="100" stroke="currentColor" vectorEffect="non-scaling-stroke"/>}
  {cursor!==null&&<line className="wave-envelope-cursor" x1={bound(cursor)} x2={bound(cursor)} y1="0" y2="100" stroke="currentColor" vectorEffect="non-scaling-stroke"/>}
 </svg>;
}

const label=(seconds:number)=>`${Math.floor(seconds/60)}:${(seconds%60).toFixed(1).padStart(4,'0')}`;
export function MiniWaveform({asset,onChoose}:{asset:CatalogAsset;onChoose?:(asset:CatalogAsset)=>void}){
 const root=useRef<HTMLDivElement>(null),live=useRef(true),previewBusy=useRef(false),revision=useRef(0);
 const key=waveformKey(asset);
 const [visible,setVisible]=useState(false),[waveState,setWaveState]=useState<{key:string;data:WaveformData}|null>(null),[cursor,setCursor]=useState<number|null>(null),[keyboard,setKeyboard]=useState(0),[error,setError]=useState(''),[busy,setBusy]=useState(false),[retry,setRetry]=useState(0);
 const wave=waveState?.key===key?waveState.data:null;
 useEffect(()=>{live.current=true;return()=>{live.current=false;};},[]);
 useEffect(()=>{revision.current++;setCursor(null);setKeyboard(0);setError('');},[key]);
 useEffect(()=>{
  const element=root.current;if(!element||typeof IntersectionObserver==='undefined')return;
  const observer=new IntersectionObserver(entries=>setVisible(entries.some(e=>e.isIntersecting)),{threshold:0.01});
  observer.observe(element);return()=>observer.disconnect();
 },[]);
 useEffect(()=>{
  if(!visible||!isTauri()||asset.status!=='ready')return;
  const controller=new AbortController();let done=false;
  setError('');
  void getWaveform(asset,0,controller.signal).then(data=>{if(!done)setWaveState({key,data});},e=>{if(!done)setError(String(e));});
  return()=>{done=true;controller.abort();};
 },[key,visible,retry,asset.status]);
 const duration=wave?.analyzed_duration??0;
 async function preview(fraction:number){
  if(!wave||!duration||previewBusy.current||!isTauri())return;
  previewBusy.current=true;setBusy(true);setError('');const version=revision.current;
  try{onChoose?.(asset);await invoke('player_preview_at',{id:asset.id,position:waveformPosition(fraction,duration)});}
  catch(e){if(live.current&&version===revision.current)setError(String(e));}
  finally{previewBusy.current=false;if(live.current)setBusy(false);}
 }
 const position=waveformPosition(cursor??keyboard,duration);
 return <div ref={root} className={`mini-waveform${busy?' is-busy':''}`} role="button" tabIndex={0} aria-label={`${asset.name} 波形试听，方向键定位，Enter 试听`} aria-disabled={!wave||busy} title={error||`鼠标点击试听 · ${wave?.truncated?'仅前段 · ':''}${label(position)}`}
  onDragStart={e=>{e.preventDefault();e.stopPropagation();}} draggable={false}
  onPointerDown={e=>{e.preventDefault();e.stopPropagation();e.currentTarget.focus({preventScroll:true});}} onPointerUp={e=>e.stopPropagation()} onDoubleClick={e=>e.stopPropagation()}
  onPointerMove={e=>{e.stopPropagation();const rect=e.currentTarget.getBoundingClientRect();setCursor(fractionAt(e.clientX,rect.left,rect.width));}}
  onPointerLeave={()=>setCursor(null)}
  onClick={e=>{e.stopPropagation();if(error&&!wave){setRetry(value=>value+1);setVisible(true);return;}const rect=e.currentTarget.getBoundingClientRect();const fraction=e.detail===0?keyboard:fractionAt(e.clientX,rect.left,rect.width);setKeyboard(fraction);void preview(fraction);}}
  onFocus={()=>setVisible(true)}
  onKeyDown={e=>{e.stopPropagation();if(['ArrowLeft','ArrowRight','Home','End','Enter',' '].includes(e.key))e.preventDefault();if(['ArrowLeft','ArrowRight','Home','End'].includes(e.key))setCursor(null);if(e.key==='ArrowLeft')setKeyboard(v=>Math.max(0,v-.02));else if(e.key==='ArrowRight')setKeyboard(v=>Math.min(1,v+.02));else if(e.key==='Home')setKeyboard(0);else if(e.key==='End')setKeyboard(1);else if(e.key==='Enter'&&!e.repeat){if(error&&!wave)setRetry(v=>v+1);else void preview(keyboard);}}}>
  {wave?<><Envelope peaks={wave.peaks} cursor={cursor??keyboard}/><span className="mini-waveform-time">{label(position)}{wave.truncated?' · 前段':''}</span></>:<span className="mini-waveform-message">{error?'波形暂不可用 · 点击重试':asset.status!=='ready'?'素材不可用':visible?'读取峰值…':'波形'}</span>}
 </div>;
}
