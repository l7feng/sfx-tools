import {useCallback,useEffect,useState} from 'react';
import {safePanelDock,type DockDefaults,type PanelDockState} from './panel-dock';

/** 波形播放器的三种落位：底部常驻 / 收进左侧栏 / 独立浮窗。 */
export type PlayerPlacement='bottom'|'sidebar'|'floating';
export type PlayerView={placement:PlayerPlacement;dock:PanelDockState};
const STORAGE_KEY='sound-workbench-player-view-v1';
const dockDefaults:DockDefaults={x:280,y:150,width:560,height:310};
const initial:PlayerView={placement:'bottom',dock:{...dockDefaults,floating:false}};

function safePlayerView(raw:unknown):PlayerView{
 const value=(typeof raw==='object'&&raw?raw:{}) as Partial<PlayerView>;
 const placement=value.placement==='sidebar'||value.placement==='floating'?value.placement:'bottom';
 const dock=safePanelDock(value.dock,dockDefaults);
 return {placement,dock:{...dock,floating:placement==='floating'}};
}

/** 位置切换与浮窗状态合并存档：吸附回底部时自动把落位改回 bottom，
 *  切到浮窗时自动置 floating=true，避免「浮窗不浮动 / 底部还挂着一份」的错乱。 */
export function usePlayerView(){
 const [view,setView]=useState<PlayerView>(()=>{
  try{return safePlayerView(JSON.parse(localStorage.getItem(STORAGE_KEY)??'{}'));}
  catch{return initial;}
 });
 useEffect(()=>{try{localStorage.setItem(STORAGE_KEY,JSON.stringify(view));}catch{/* 存储不可用仅本次会话有效 */}},[view]);
 const setPlacement=useCallback((placement:PlayerPlacement)=>setView(old=>({placement,dock:{...old.dock,floating:placement==='floating'}})),[]);
 const setDock=useCallback((dock:PanelDockState)=>setView(old=>({
  placement:dock.floating?'floating':old.placement==='floating'?'bottom':old.placement,
  dock
 })),[]);
 return {view,setPlacement,setDock};
}
