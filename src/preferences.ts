import {useEffect,useState} from 'react';
import {defaultMusicFilterTemplates,safeMusicFilterTemplates,type MusicFilterTemplates} from './music-filter-defaults.ts';
export type WorkspacePage='sound'|'music';
export type Preferences={
 startPage:WorkspacePage;
 volume:number;
 waveColor:'blue'|'white';
 rememberLayout:boolean;
 /** 搜索一种语言时同时命中中/英/日词表；公式与分类筛选不受影响。 */
 multilingualSearch:boolean;
 /** 搜索一并匹配 WAV/BWF 内嵌的 bext / iXML 字段；搜索栏「含元数据」按钮与设置页联动，改动写回偏好。关闭时只搜文件名。 */
 metadataSearch:boolean;
 /** 列表小波形把整条音频压成一条单色包络（不分声道、峰值密度更低），并复用缓存减少重复解码。 */
 simpleWaveform:boolean;
 auditionOnSelect:boolean;
 replaySameFromStart:boolean;
 stopOnDragToDaw:boolean;
 /** 可插拔模块开关：默认全关，由 Cargo feature 与设置页双层控制。 */
 pluginAafImport:boolean;
 pluginPtOverlay:boolean;
 pluginPtBouncer:boolean;
 /** 'default' 或输出设备名；设备拔掉后后端自动回落系统默认。 */
 outputDevice:string;
 /** 音乐库筛选各维度的预置值；设置页可编辑，空数组表示该维度不显示预置。 */
 filterTemplates:MusicFilterTemplates;
};
export const defaultPreferences:Preferences={
 startPage:'sound',volume:15,waveColor:'blue',rememberLayout:true,
 multilingualSearch:true,metadataSearch:true,simpleWaveform:true,
 auditionOnSelect:false,replaySameFromStart:true,stopOnDragToDaw:true,outputDevice:'default',
 pluginAafImport:false,pluginPtOverlay:false,pluginPtBouncer:false,
 filterTemplates:defaultMusicFilterTemplates()
};
/** 旧存档缺字段时逐项兜底，避免升级后开关被静默重置成 false。 */
export function safePreferences(raw:unknown):Preferences{
 const value=(typeof raw==='object'&&raw?raw:{}) as Partial<Preferences>;
 const on=(key:keyof Preferences)=>value[key]!==false;
 const off=(key:keyof Preferences)=>value[key]===true;
 return {
  // 0.1.30 起「音频工具」不再是顶级页面：旧存档里的 'tools' 一律回落到音效库。
  startPage:value.startPage==='music'?'music':'sound',
  volume:typeof value.volume==='number'&&Number.isFinite(value.volume)?Math.max(0,Math.min(100,value.volume)):15,
  waveColor:value.waveColor==='white'?'white':'blue',
  rememberLayout:on('rememberLayout'),
  multilingualSearch:on('multilingualSearch'),
  metadataSearch:on('metadataSearch'),
  simpleWaveform:on('simpleWaveform'),
  auditionOnSelect:off('auditionOnSelect'),
  replaySameFromStart:on('replaySameFromStart'),
  stopOnDragToDaw:on('stopOnDragToDaw'),
  pluginAafImport:off('pluginAafImport'),
  pluginPtOverlay:off('pluginPtOverlay'),
  pluginPtBouncer:off('pluginPtBouncer'),
  outputDevice:typeof value.outputDevice==='string'&&value.outputDevice.trim()?value.outputDevice:'default',
  filterTemplates:safeMusicFilterTemplates(value.filterTemplates)
 };
}
export function usePreferences(){
 const [preferences,setPreferences]=useState<Preferences>(()=>{try{return safePreferences(JSON.parse(localStorage.getItem('sound-workbench-preferences-v1')??'{}'));}catch{return safePreferences(null);}});
 useEffect(()=>{try{localStorage.setItem('sound-workbench-preferences-v1',JSON.stringify(preferences));}catch{/* 配置存储不可用不阻断当前操作 */}},[preferences]);
 return {preferences,setPreferences};
}
