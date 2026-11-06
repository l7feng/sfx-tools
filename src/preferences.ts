import {useEffect,useState} from 'react';
export type WorkspacePage='sound'|'music'|'tools';
export type Preferences={density:'compact'|'comfortable';startPage:WorkspacePage;volume:number;waveColor:'blue'|'white';rememberLayout:boolean};
export const defaultPreferences:Preferences={density:'compact',startPage:'sound',volume:15,waveColor:'blue',rememberLayout:true};
export function safePreferences(raw:unknown):Preferences{
 const value=(typeof raw==='object'&&raw?raw:{}) as Partial<Preferences>;
 return {density:value.density==='comfortable'?'comfortable':'compact',startPage:['sound','music','tools'].includes(value.startPage??'')?value.startPage!: 'sound',volume:typeof value.volume==='number'&&Number.isFinite(value.volume)?Math.max(0,Math.min(100,value.volume)):15,waveColor:value.waveColor==='white'?'white':'blue',rememberLayout:value.rememberLayout!==false};
}
export function usePreferences(){
 const [preferences,setPreferences]=useState<Preferences>(()=>{try{return safePreferences(JSON.parse(localStorage.getItem('sound-workbench-preferences-v1')??'{}'));}catch{return safePreferences(null);}});
 useEffect(()=>{try{localStorage.setItem('sound-workbench-preferences-v1',JSON.stringify(preferences));}catch{/* 配置存储不可用不阻断当前操作 */}},[preferences]);
 return {preferences,setPreferences};
}
