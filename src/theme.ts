import {useEffect,useState} from 'react';

/** 主题：单一强调色（冷蓝）× 明暗两种底色（2026-11-14 删去青绿、紫罗兰，同日弃用暖金/浅橘）。
   值写进 <html> 的 data 属性，由 theme.css 消费。 */
export const THEME_ACCENTS=[
 {value:'blue',label:'冷蓝'},
]as const;
export type ThemeAccent=typeof THEME_ACCENTS[number]['value'];
export type ThemeMode='dark'|'light';
/** 与 index.html 里的首帧脚本共用同一个键，改这里要同步改那边。 */
export const THEME_KEY='sfx-tools-theme-v1';

const isAccent=(value:unknown):value is ThemeAccent=>THEME_ACCENTS.some(item=>item.value===value);
/** 旧值迁移：teal / violet / gold / clay 都已废弃，老存档读到一律回落到冷蓝，不让界面停在无效色上。 */
const LEGACY_ACCENTS:Record<string,ThemeAccent>={teal:'blue',violet:'blue',gold:'blue',clay:'blue'};

export function readTheme():{mode:ThemeMode;accent:ThemeAccent}{
 try{
  const raw=JSON.parse(localStorage.getItem(THEME_KEY)||'{}') as {mode?:string;accent?:string};
  const stored=String(raw.accent??'');
  return {mode:raw.mode==='light'?'light':'dark',accent:isAccent(stored)?stored:LEGACY_ACCENTS[stored]??'blue'};
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
