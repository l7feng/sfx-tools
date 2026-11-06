import React,{useEffect,useId,useMemo,useRef} from 'react';
import {Envelope,channelLanes} from './waveform';
import {envelopePath} from './waveform-data';
import {heatColor,frequencyColor,type WaveMode} from './spectral-state';
import './spectral-view.css';
export type VisualData={duration:number;rate:number;peaks:number[];rms:number[];spectrum:number[][];frequencies:number[];centroids:number[];fft_size:number;window_count:number;channels?:number;channel_peaks?:number[][];channel_rms?:number[][]};
export type VisualSnapshot={job_id:string;asset_id:string;status:string;progress:number|null;analyzed_seconds:number;error:string|null;result:VisualData|null};
export function SpectralView({mode,data,progress}:{mode:WaveMode;data:VisualData;progress:number}){
 const canvas=useRef<HTMLCanvasElement>(null),gradient=useId().replaceAll(':','');
 // 立体声：左右声道各自成轨，上下分区；单声道与多声道仍走单轨。
 const lanes=channelLanes(data),rmsLanes=channelLanes({channel_peaks:data.channel_rms});
 const paths=useMemo(()=>lanes?lanes.map((row,index)=>envelopePath(row,2,index)):[envelopePath(data.peaks)],[lanes,data.peaks]);
 useEffect(()=>{if(mode!=='spectrum'&&mode!=='combined')return;const element=canvas.current;if(!element)return;const width=data.spectrum.length,height=data.frequencies.length;element.width=width;element.height=height;const context=element.getContext('2d');if(!context)return;const pixels=context.createImageData(width,height);for(let x=0;x<width;x++)for(let y=0;y<height;y++){const color=heatColor(data.spectrum[x][y]);const at=((height-y-1)*width+x)*4;pixels.data[at]=color[0];pixels.data[at+1]=color[1];pixels.data[at+2]=color[2];pixels.data[at+3]=255;}context.putImageData(pixels,0,0);},[data,mode]);
 const bound=Math.max(0,Math.min(1,progress));
 return <div className={`spectral-view spectral-${mode}${lanes?' is-stereo':''}`} aria-label={mode==='rms'?'真实峰值与RMS':mode==='frequency'?'按频谱中心着色的真实波形':'真实采样窗频谱概览'}>
 {(mode==='spectrum'||mode==='combined')&&<><canvas ref={canvas} className="spectral-heatmap"/><div className="spectral-axis"><span>{Math.round(data.rate/2000)}kHz</span><span>对数频带</span><span>{Math.round(data.frequencies[0]??20)}Hz</span></div><div className="spectral-db">0 dBFS ━ -100</div></>}
 {mode==='rms'&&<><Envelope peaks={data.peaks} lanes={lanes}/><Envelope peaks={data.rms} lanes={rmsLanes} className="rms-envelope"/></>}
 {mode==='combined'&&<Envelope peaks={data.peaks} lanes={lanes} className="combined-envelope"/>}
 {mode==='frequency'&&<svg className="frequency-envelope" viewBox="0 0 1000 100" preserveAspectRatio="none" aria-hidden="true"><defs><linearGradient id={gradient}>{data.centroids.map((hz,i)=><stop key={i} offset={`${i/Math.max(1,data.centroids.length-1)*100}%`} stopColor={frequencyColor(hz,data.rate)}/>)}</linearGradient></defs>{paths.map((path,index)=><path key={index} d={path} fill={`url(#${gradient})`} fillOpacity=".65" stroke="none"/>)}</svg>}
 {lanes&&<div className="spectral-lane-labels" aria-hidden="true"><span>L</span><span>R</span></div>}
 <span className="spectral-playhead" style={{left:`${bound*100}%`}}/>
 {mode==='rms'&&<span className="spectral-caption">{lanes?'上下分轨：左 / 右声道 · 外层峰值 内层RMS':'外层峰值 · 内层RMS'}</span>}{mode==='frequency'&&<span className="spectral-caption">蓝：低频 · 金：高频 · 颜色依据谱中心</span>}
 </div>;
}
