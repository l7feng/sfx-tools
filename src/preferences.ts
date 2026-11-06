import {useEffect,useState} from 'react';
export type WorkspacePage='sound'|'music'|'tools';
export type Preferences={
 density:'compact'|'comfortable';
 startPage:WorkspacePage;
 volume:number;
 waveColor:'blue'|'white';
 rememberLayout:boolean;
 /** 搜索一种语言时同时命中中/英/日词表；公式与分类筛选不受影响。 */
 multilingualSearch:boolean;
 /** 列表小波形把整条音频压成一条单色包络（不分声道、峰值密度更低），并复用缓存减少重复解码。 */
 simpleWaveform:boolean;
 /** 打开音频工具页时自动折叠左栏/右栏/底栏，离开时恢复素材库布局。 */
 collapseToolsPanels:boolean;
 auditionOnSelect:boolean;
 replaySameFromStart:boolean;
 stopOnDragToDaw:boolean;
 /** 'default' 或输出设备名；设备拔掉后后端自动回落系统默认。 */
 outputDevice:string;
};
export const defaultPreferences:Preferences={
 density:'compact',startPage:'sound',volume:15,waveColor:'blue',rememberLayout:true,
 multilingualSearch:true,simpleWaveform:true,collapseToolsPanels:true,
 auditionOnSelect:false,replaySameFromStart:true,stopOnDragToDaw:true,outputDevice:'default'
};
/** 旧存档缺字段时逐项兜底，避免升级后开关被静默重置成 false。 */
export function safePreferences(raw:unknown):Preferences{
 const value=(typeof raw==='object'&&raw?raw:{}) as Partial<Preferences>;
 const on=(key:keyof Preferences)=>value[key]!==false;
 const off=(key:keyof Preferences)=>value[key]===true;
 return {
  density:value.density==='comfortable'?'comfortable':'compact',
  startPage:['sound','music','tools'].includes(value.startPage??'')?value.startPage!: 'sound',
  volume:typeof value.volume==='number'&&Number.isFinite(value.volume)?Math.max(0,Math.min(100,value.volume)):15,
  waveColor:value.waveColor==='white'?'white':'blue',
  rememberLayout:on('rememberLayout'),
  multilingualSearch:on('multilingualSearch'),
  simpleWaveform:on('simpleWaveform'),
  collapseToolsPanels:on('collapseToolsPanels'),
  auditionOnSelect:off('auditionOnSelect'),
  replaySameFromStart:on('replaySameFromStart'),
  stopOnDragToDaw:on('stopOnDragToDaw'),
  outputDevice:typeof value.outputDevice==='string'&&value.outputDevice.trim()?value.outputDevice:'default'
 };
}
export function usePreferences(){
 const [preferences,setPreferences]=useState<Preferences>(()=>{try{return safePreferences(JSON.parse(localStorage.getItem('sound-workbench-preferences-v1')??'{}'));}catch{return safePreferences(null);}});
 useEffect(()=>{try{localStorage.setItem('sound-workbench-preferences-v1',JSON.stringify(preferences));}catch{/* 配置存储不可用不阻断当前操作 */}},[preferences]);
 return {preferences,setPreferences};
}
