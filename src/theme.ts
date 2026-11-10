import {useEffect,useState} from 'react';

/** 主题：4 套强调色 × 明暗两种底色。值写进 <html> 的 data 属性，由 theme.css 消费。 */
export const THEME_ACCENTS=[
 {value:'blue',label:'冷蓝'},
 {value:'gold',label:'暖金'},
 {value:'teal',label:'青绿'},
 {value:'violet',label:'紫罗兰'},
]as const;
export type ThemeAccent=typeof THEME_ACCENTS[number]['value'];
export type ThemeMode='dark'|'light';
/** 与 index.html 里的首帧脚本共用同一个键，改这里要同步改那边。 */
export const THEME_KEY='sfx-tools-theme-v1';

const isAccent=(value:unknown):value is ThemeAccent=>THEME_ACCENTS.some(item=>item.value===value);

export function readTheme():{mode:ThemeMode;accent:ThemeAccent}{
 try{
  const raw=JSON.parse(localStorage.getItem(THEME_KEY)||'{}') as {mode?:string;accent?:string};
  return {mode:raw.mode==='light'?'light':'dark',accent:isAccent(raw.accent)?raw.accent:'blue'};
 }catch{return {mode:'dark',accent:'blue'};}
}

export function applyTheme(mode:ThemeMode,accent:ThemeAccent){
 const root=document.documentElement;
 root.dataset.theme=mode;
 root.dataset.accent=accent;
 try{localStorage.setItem(THEME_KEY,JSON.stringify({mode,accent}));}catch{/* 隐私模式写不进去就只改当前会话 */}
}

export function useTheme(){
 const [state,setState]=useState(readTheme);
 useEffect(()=>{applyTheme(state.mode,state.accent);},[state.mode,state.accent]);
 return {
  mode:state.mode,accent:state.accent,
  setMode:(mode:ThemeMode)=>setState(old=>({...old,mode})),
  setAccent:(accent:ThemeAccent)=>setState(old=>({...old,accent})),
 };
}
