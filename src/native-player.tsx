import React,{useEffect,useRef,useState} from 'react';
import {invoke,isTauri} from '@tauri-apps/api/core';
import type {CatalogAsset} from './catalog-client';
import {timeLabel} from './demo-data';
import {AudioToolbar} from './audio-toolbar';
import {Envelope} from './waveform';
import {getWaveform,type WaveformData} from './waveform-data';
import {getFullWaveform,type FullWaveSnapshot} from './full-wave-client';
import {usePlaybackClock} from './playback-clock';
import {SpectralView,type VisualData,type VisualSnapshot} from './spectral-view';
import {safeWaveMode,waveModes,type WaveMode} from './spectral-state';
import {analyzeVisual} from './visual-client';
type Snapshot={id:string|null;state:string;position:number;duration:number|null;volume:number;looping:boolean;error:string|null};
type Waveform=WaveformData;
const initial:Snapshot={id:null,state:'idle',position:0,duration:null,volume:.15,looping:false,error:null};
export function NativePlayer({selected,collapsed=false,onToggle,defaultVolume=15,onOpenLayer}:{selected:CatalogAsset|null;collapsed?:boolean;onToggle?:()=>void;defaultVolume?:number;onOpenLayer?:()=>void}){
 const native=isTauri(),[snapshot,setSnapshot]=useState(initial),[wave,setWave]=useState<Waveform|null>(null),[waveError,setWaveError]=useState(''),[error,setError]=useState(''),[busy,setBusy]=useState(false),[waveLoading,setWaveLoading]=useState(false),[retry,setRetry]=useState(0),[volumeDraft,setVolumeDraft]=useState<number|null>(null),[loaded,setLoaded]=useState<{id:string;name:string}|null>(null);
 const rendered=!!snapshot.id?.startsWith('render:');
 const processed=!!snapshot.id?.startsWith('tool:');const derived=rendered||processed;
 const displayPosition=usePlaybackClock(snapshot);
 const [waveMode,setWaveMode]=useState<WaveMode>(()=>{try{return safeWaveMode(localStorage.getItem('sound-workbench-wave-mode-v1'));}catch{return 'peaks';}}),[visual,setVisual]=useState<VisualData|null>(null),[visualKey,setVisualKey]=useState<string|null>(null),[visualStatus,setVisualStatus]=useState<VisualSnapshot|null>(null),[visualError,setVisualError]=useState(''),[visualRetry,setVisualRetry]=useState(0);const visualAbort=useRef<AbortController|null>(null);
 const needsVisual=waveMode!=='peaks';
 const selectedVisualKey=JSON.stringify([selected?.id,selected?.size,selected?.modified]);const shownVisual=visualKey===selectedVisualKey?visual:null;
 useEffect(()=>{try{localStorage.setItem('sound-workbench-wave-mode-v1',waveMode);}catch{/* 配色存储不可用仍可操作 */}},[waveMode]);
 useEffect(()=>{setVisual(null);setVisualStatus(null);setVisualError('');if(!selected||!native||derived||!needsVisual)return;const abort=new AbortController();setVisualKey(selectedVisualKey);visualAbort.current=abort;let done=false;void analyzeVisual(selected.id,abort.signal,value=>{if(!done)setVisualStatus(value);}).then(value=>{if(!done)setVisual(value);},error=>{if(!done&&(error as Error).name!=='AbortError')setVisualError(String(error));});return()=>{done=true;abort.abort();if(visualAbort.current===abort)visualAbort.current=null;};},[selected?.id,selected?.modified,selected?.size,native,derived,needsVisual,visualRetry]);
 const [fullWave,setFullWave]=useState<FullWaveSnapshot|null>(null),[fullJob,setFullJob]=useState<string|null>(null),[fullError,setFullError]=useState('');const fullAbort=useRef<AbortController|null>(null);
 const [waveSeek,setWaveSeek]=useState<number|null>(null),[fullSeek,setFullSeek]=useState<number|null>(null);
 const appliedVolume=useRef<number|null>(null);
 const lock=useRef(false),alive=useRef(true),statusRevision=useRef(0);
 useEffect(()=>{alive.current=true;return()=>{alive.current=false;};},[]);
 useEffect(()=>{
  if(!native)return;let done=false,timer:number;
  const poll=async()=>{const version=statusRevision.current;try{const next=await invoke<Snapshot>('player_status');if(!done&&version===statusRevision.current)setSnapshot(next);}catch(e){if(!done)setError(String(e));}finally{if(!done)timer=window.setTimeout(poll,250);}};
  void poll();return()=>{done=true;clearTimeout(timer);};
 },[native]);
 useEffect(()=>{
  let done=false,fullReady=false;setWave(null);setWaveError('');setFullError('');setFullWave(null);setFullJob(null);setWaveLoading(false);setWaveSeek(null);
  if(!selected||!native||derived)return;
  setWaveLoading(true);const abort=new AbortController(),quickAbort=new AbortController();fullAbort.current=abort;
  void getWaveform(selected,10,quickAbort.signal).then(result=>{if(!done&&!fullReady)setWave(result);}).catch(e=>{if(!done&&!fullReady)setWaveError(String(e));}).finally(()=>{if(!done)setWaveLoading(false);});
  void getFullWaveform(selected,abort.signal,value=>{if(!done)setFullWave(value);}).then(result=>{if(!done){fullReady=true;quickAbort.abort();setWave(result.wave);setFullJob(result.jobId);setWaveError('');setWaveLoading(false);}}).catch(e=>{if(!done&&(e as Error).name!=='AbortError')setFullError(String(e));});
  return()=>{done=true;abort.abort();quickAbort.abort();if(fullAbort.current===abort)fullAbort.current=null;};
 },[selected?.id,selected?.modified,selected?.size,native,retry,derived]);
 async function run(command:string,args:Record<string,unknown>){
  if(lock.current)return;lock.current=true;setBusy(true);setError('');statusRevision.current++;
  try{const next=await invoke<Snapshot>(command,args);if(alive.current){setSnapshot(next);if(command==='player_load'&&selected)setLoaded({id:selected.id,name:selected.name});}}
  catch(e){if(alive.current)setError(String(e));}finally{statusRevision.current++;lock.current=false;if(alive.current)setBusy(false);}
 }
 useEffect(()=>{setFullSeek(null);setWaveSeek(null);},[snapshot.id]);
 const volume=volumeDraft??Math.round(snapshot.volume*100);
 const commitVolume=(value:number)=>{void control('volume',value/100).finally(()=>setVolumeDraft(null));};
 const same=!!selected&&snapshot.id===selected.id;
 const control=(action:string,value:number|null=null)=>run('player_control',{action,value});
 useEffect(()=>{if(native&&!busy&&appliedVolume.current!==defaultVolume){appliedVolume.current=defaultVolume;void control('volume',Math.max(0,Math.min(100,defaultVolume))/100);}},[native,busy,defaultVolume]);
 useEffect(()=>{if(selected&&snapshot.id===selected.id)setLoaded({id:selected.id,name:selected.name});},[snapshot.id,selected?.id,selected?.name]);
 const playSelected=()=>{if(!selected)return;if(same&&snapshot.state==='playing')void control('pause');else if(same&&snapshot.state==='paused')void control('resume');else void run('player_load',{id:selected.id});};
 useEffect(()=>{const key=(event:KeyboardEvent)=>{if(!native||document.querySelector('dialog[open]')||(event.target as HTMLElement).closest('input,textarea,select,button:not(.asset-name),a,summary,[role=separator]'))return;if(event.code==='Space'){event.preventDefault();if(derived){if(snapshot.state==='playing')void control('pause');else if(snapshot.state==='paused')void control('resume');else if(snapshot.id){if(rendered)void run('player_render_preview',{jobId:snapshot.id.slice('render:'.length)});else{const parts=snapshot.id.split(':');void run('audio_tools_preview',{jobId:parts[1],index:Number(parts[2]??0)});}}}else playSelected();}};window.addEventListener('keydown',key);return()=>window.removeEventListener('keydown',key);});
 const playingName=processed?'处理结果试听':rendered?'叠层渲染试听':snapshot.id?(loaded?.id===snapshot.id?loaded.name:snapshot.id):'尚未播放';
 const seekWave=(position:number)=>{if(!selected)return;if(same)void control('seek',position);else void run(fullJob?'player_preview_full_at':'player_preview_at',{id:selected.id,position,...(fullJob?{jobId:fullJob}:{})});};
 const shownDuration=needsVisual&&shownVisual?shownVisual.duration:wave?.analyzed_duration??0;
 const progress=same?Math.min(1,displayPosition/(shownDuration||1)):0;
 const stateLabels:Record<string,string>={idle:'待机',loading:'加载中',playing:'播放中',paused:'已暂停',stopped:'已停止',ended:'已结束',error:'播放错误'};
 return <section className={`player real-player ${collapsed?'is-collapsed':''}`} aria-label="真实音频播放器"><div className="player-heading"><div><span className="player-status">{stateLabels[snapshot.state]??snapshot.state}</span><strong title={playingName}>{playingName}</strong></div><span className="player-spec">{derived?`${processed?'处理':'叠层混音'}结果 · 使用整段定位`:<>波形：{selected?.name??'先选择素材'}{needsVisual&&shownVisual?` · 高级概览 ${timeLabel(shownVisual.duration)}`:wave?` · ${wave.truncated?'前段':'已分析'} ${timeLabel(wave.analyzed_duration)}`:''}</>}</span><button onClick={onToggle} aria-label={collapsed?'展开底部播放器':'折叠底部播放器'} aria-expanded={!collapsed}>{collapsed?'⌃':'⌄'}</button></div>
 {!collapsed&&!derived&&<div className="wave-view-controls"><label>波形视图 <select aria-label="波形显示模式" value={waveMode} onChange={event=>setWaveMode(safeWaveMode(event.target.value))}>{waveModes.map(item=><option key={item.id} value={item.id}>{item.label}</option>)}</select></label>{needsVisual&&visualStatus?.status==='running'&&<><span role="status">真实RMS/频谱分析 {Math.round((visualStatus.progress??0)*100)}%</span><button onClick={()=>{visualAbort.current?.abort();setVisualStatus(old=>old?{...old,status:'cancelled'}:old);}}>取消高级分析</button></>}{needsVisual&&(visualError||visualStatus?.status==='cancelled')&&<><span className="asset-error" role="alert">{visualError||'高级分析已取消'}</span><button onClick={()=>setVisualRetry(value=>value+1)}>重试</button></>}</div>}
 {!collapsed&&<div className="real-wave-area">{!derived&&((needsVisual&&shownVisual)||(!needsVisual&&wave&&wave.peaks.length>0))?<>{needsVisual&&shownVisual?<SpectralView mode={waveMode} data={shownVisual} progress={progress}/>:wave&&<Envelope peaks={wave.peaks} progress={progress}/>}<input className="seek" type="range" aria-label="在已分析波形内定位" min="0" max={shownDuration||1} step="0.01" value={waveSeek??(same?Math.min(displayPosition,shownDuration):0)} disabled={busy||!shownDuration||needsVisual&&!same&&!fullJob} onPointerCancel={()=>setWaveSeek(null)} onChange={e=>setWaveSeek(Number(e.target.value))} onPointerUp={e=>{seekWave(Number(e.currentTarget.value));setWaveSeek(null);}} onKeyUp={e=>{if(['ArrowLeft','ArrowRight','ArrowUp','ArrowDown','PageUp','PageDown','Home','End'].includes(e.key)){seekWave(Number(e.currentTarget.value));setWaveSeek(null);setFullSeek(null);}}}/></>:<p>{derived?'派生处理结果可使用下方整段定位。':needsVisual?(visualError||visualStatus?.status==='cancelled'?'高级视图暂不可用，请重试或切回峰值。':selected?'正在读取真实采样生成高级波形概览…':'选择素材查看真实RMS与频谱。'):waveLoading?'正在读取音频并生成前段波形…':waveError||'选择素材查看真实波形；不会自动播放。'}{!derived&&!needsVisual&&waveError&&<button onClick={()=>setRetry(v=>v+1)}>重试波形</button>}</p>}</div>}
 {!collapsed&&!derived&&needsVisual&&shownVisual&&<p className="wave-analysis-note">{waveMode==='rms'?'全长分桶RMS，跨声道均方后开方。':`频谱概览：${shownVisual.window_count}个真实时间窗 × ${shownVisual.frequencies.length}个对数频带 · FFT ${shownVisual.fft_size} · Hann窗 · -100～0 dBFS；短窗抽样非逐帧编辑。`}</p>}
 {!collapsed&&!derived&&selected&&<div className="player-footnote" role="status">{fullJob?`完整波形 · ${timeLabel(wave?.analyzed_duration??0)}`:fullWave?.status==='running'?<>完整波形分析中 · {fullWave.progress===null?timeLabel(fullWave.analyzed_seconds):`${Math.round(fullWave.progress*100)}%`} <button onClick={()=>{fullAbort.current?.abort();setFullWave(old=>old?{...old,status:'cancelled'}:old);}}>取消完整分析</button></>:fullError?<>{fullError} <button onClick={()=>setRetry(value=>value+1)}>重试完整波形</button></>:fullWave?.status==='cancelled'?<button onClick={()=>setRetry(value=>value+1)}>重新分析完整波形</button>:'正在准备完整波形…'}</div>}
 <div className="transport"><div className="transport-buttons"><button className="play-button" title={derived?'播放选中素材（切回素材试听）':undefined} disabled={!selected||busy||!native||!!selected.error} aria-label={same&&snapshot.state==='playing'?'暂停':'播放选中素材'} onClick={playSelected}>{same&&snapshot.state==='playing'?'Ⅱ':'▶'}</button>{derived&&['playing','paused'].includes(snapshot.state)&&<button disabled={busy} onClick={()=>void control(snapshot.state==='playing'?'pause':'resume')}>{snapshot.state==='playing'?'暂停结果':'继续结果'}</button>}<button disabled={!snapshot.id||busy} onClick={()=>void control('stop')}>停止</button><button disabled={busy||!native} className={snapshot.looping?'enabled':''} aria-pressed={snapshot.looping} onClick={()=>void control('loop',snapshot.looping?0:1)}>循环</button><span className="time-readout">{timeLabel(displayPosition)}<small> / {snapshot.duration==null?'未知':timeLabel(snapshot.duration)}</small></span></div><label className="volume">音量<input type="range" aria-label="试听音量" min="0" max="100" value={volume} disabled={busy||!native} onChange={e=>setVolumeDraft(Number(e.target.value))} onPointerCancel={()=>setVolumeDraft(null)} onPointerUp={e=>commitVolume(Number(e.currentTarget.value))} onKeyUp={e=>{if(['ArrowLeft','ArrowRight','ArrowUp','ArrowDown','PageUp','PageDown','Home','End'].includes(e.key))commitVolume(Number(e.currentTarget.value));}}/><span>{volume}%</span></label></div>
 {!collapsed&&snapshot.id&&snapshot.duration!=null&&snapshot.duration>0&&<label className="full-seek">整段定位<input type="range" aria-label="完整音频定位" min="0" max={snapshot.duration} step="0.1" value={fullSeek??snapshot.position} onPointerCancel={()=>setFullSeek(null)} onChange={e=>setFullSeek(Number(e.target.value))} disabled={busy} onPointerUp={e=>{void control('seek',Number(e.currentTarget.value));setFullSeek(null);}} onKeyUp={e=>{if(['ArrowLeft','ArrowRight','ArrowUp','ArrowDown','PageUp','PageDown','Home','End'].includes(e.key)){void control('seek',Number(e.currentTarget.value));setWaveSeek(null);setFullSeek(null);}}}/><span>拖动后松开定位</span></label>}
 {(error||snapshot.error)&&<p className="player-error" role="alert">{error||snapshot.error}</p>}
 {!collapsed&&<p className="player-footnote">{derived?'派生结果试听 · Space 暂停/继续 · 可停止、循环与整段定位':'选中不自动播 · 大波形后台分析全长；列表小波形保留前段快速预览 · Space 播放／暂停'}</p>}<AudioToolbar selected={selected} onOpenLayer={onOpenLayer}/></section>;
}
