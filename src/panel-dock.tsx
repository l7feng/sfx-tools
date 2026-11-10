import React,{useEffect,useRef,useState} from 'react';

/**
 * 面板浮窗通用框架：Adobe 式「靠近目标停靠边出现半透明预览，松手吸附回去」，
 * 其余三边做 8px 磁吸对齐；右下角手柄可拖拽调尺寸，位置与尺寸各自持久化。
 * 侧栏（吸附回左侧）与播放器（吸附回底部）共用这一套，避免两套拖动逻辑分叉。
 */
export type DockEdge='left'|'bottom';
export type PanelDockState={floating:boolean;x:number;y:number;width:number;height:number};
export type DockDefaults={x:number;y:number;width:number;height:number};
export type DockApi={floating:boolean;dragging:boolean;toggleDock:()=>void};
type SnapHint='dock'|'left'|'right'|'top'|'bottom'|null;
const clamp=(value:number,min:number,max:number)=>Math.min(Math.max(value,min),Math.max(min,max));

export function safePanelDock(raw:unknown,defaults:DockDefaults):PanelDockState{
 const value=(typeof raw==='object'&&raw?raw:{}) as Partial<PanelDockState>;
 const number=(input:unknown,fallback:number,min:number,max:number)=>typeof input==='number'&&Number.isFinite(input)?clamp(input,min,max):fallback;
 return {
  floating:value.floating===true,
  x:number(value.x,defaults.x,0,6000),
  y:number(value.y,defaults.y,0,6000),
  width:number(value.width,defaults.width,200,1600),
  height:number(value.height,defaults.height,160,1200)
 };
}

/** 浮窗位置 / 尺寸单独存档，不挤进 layout，避免「重置布局」把浮窗状态一并清掉。 */
export function usePanelDock(storageKey:string,defaults:DockDefaults){
 const [dock,setDock]=useState<PanelDockState>(()=>{
  try{return safePanelDock(JSON.parse(localStorage.getItem(storageKey)??'{}'),defaults);}
  catch{return {...defaults,floating:false};}
 });
 useEffect(()=>{try{localStorage.setItem(storageKey,JSON.stringify(dock));}catch{/* 存储不可用仅本次会话有效 */}},[dock,storageKey]);
 return {dock,setDock};
}

type DragState={mode:'move'|'resize';sx:number;sy:number;dx:number;dy:number;w:number;h:number;moved:boolean;origin:PanelDockState};

export function DockFrame({dock,onChange,collapsed=false,edge='left',className,dockedSize=248,snapLabel='松手停靠到左侧',slot,children}:{
 dock:PanelDockState;
 onChange:(next:PanelDockState)=>void;
 collapsed?:boolean;
 edge?:DockEdge;
 className:string;
 dockedSize?:number;
 snapLabel?:string;
 slot?:React.ReactNode;
 children:(api:DockApi)=>React.ReactNode;
}){
 const root=useRef<HTMLDivElement>(null);
 const drag=useRef<DragState|null>(null);
 const dockRef=useRef(dock);dockRef.current=dock;
 const snapRef=useRef<SnapHint>(null);
 const [dragging,setDragging]=useState(false);
 const [snap,setSnapState]=useState<SnapHint>(null);
 const setSnap=(value:SnapHint)=>{snapRef.current=value;setSnapState(value);};

 const bounded=(x:number,y:number,w=dockRef.current.width,h=dockRef.current.height)=>({
  x:clamp(x,0,Math.max(0,window.innerWidth-Math.min(w,window.innerWidth-12)-8)),
  y:clamp(y,0,Math.max(0,window.innerHeight-Math.min(h,window.innerHeight-12)-8))
 });
 // 窗口缩放后把浮窗拉回可视区，避免拖到屏幕外找不回来。
 useEffect(()=>{
  if(!dock.floating)return;
  const fix=()=>{const next=bounded(dock.x,dock.y,dock.width,dock.height);if(next.x!==dock.x||next.y!==dock.y)onChange({...dockRef.current,...next});};
  fix();window.addEventListener('resize',fix);
  return()=>window.removeEventListener('resize',fix);
 },[dock,onChange]);

 const toggleDock=()=>onChange(dock.floating?{...dock,floating:false}:{...dock,floating:true,...bounded(dock.x,dock.y)});
 const begin=(event:React.PointerEvent<HTMLDivElement>)=>{
  if(event.button!==0)return;
  const target=event.target as HTMLElement;
  const resizing=!!target.closest('.dock-resize-handle');
  if(!resizing){
   if(target.closest('button,input,select,textarea,a,[role=separator],.sidebar-rail-popover'))return;
   if(!target.closest('.dock-grip,.panel-heading'))return;
  }
  const rect=root.current?.getBoundingClientRect();if(!rect)return;
  drag.current={mode:resizing?'resize':'move',sx:event.clientX,sy:event.clientY,dx:event.clientX-rect.left,dy:event.clientY-rect.top,w:rect.width,h:rect.height,moved:false,origin:dock};
  event.currentTarget.setPointerCapture(event.pointerId);
 };
 const move=(event:React.PointerEvent<HTMLDivElement>)=>{
  const state=drag.current;if(!state)return;
  if(!state.moved){if(Math.hypot(event.clientX-state.sx,event.clientY-state.sy)<5)return;state.moved=true;setDragging(true);}
  if(state.mode==='resize'){
   const width=clamp(state.w+(event.clientX-state.sx),220,Math.max(220,window.innerWidth-16));
   const height=clamp(state.h+(event.clientY-state.sy),160,Math.max(160,window.innerHeight-16));
   const next=bounded(dockRef.current.x,dockRef.current.y,width,height);
   onChange({...dockRef.current,floating:true,width,height,...next});
   setSnap(null);
   return;
  }
  const next=bounded(event.clientX-state.dx,event.clientY-state.dy,state.w,state.h);
  const nearDock=edge==='left'?next.x<=72:next.y+state.h>=window.innerHeight-72;
  let hint:SnapHint=null;
  if(nearDock)hint='dock';
  else if(next.x<=14)hint='left';
  else if(next.x+state.w>=window.innerWidth-14)hint='right';
  else if(next.y<=14)hint='top';
  else if(next.y+state.h>=window.innerHeight-14)hint='bottom';
  setSnap(hint);
  const aligned=hint==='left'?{...next,x:8}:hint==='right'?{...next,x:Math.max(0,window.innerWidth-state.w-8)}
   :hint==='top'?{...next,y:8}:hint==='bottom'?{...next,y:Math.max(0,window.innerHeight-state.h-8)}:next;
  onChange({...dockRef.current,floating:true,...aligned});
 };
 const end=(event:React.PointerEvent<HTMLDivElement>)=>{
  const state=drag.current;drag.current=null;setDragging(false);
  if(event.currentTarget.hasPointerCapture(event.pointerId))event.currentTarget.releasePointerCapture(event.pointerId);
  if(!state?.moved)return;
  // 命中停靠区：回到原布局，但保留本次调过的浮窗坐标 / 尺寸，下次浮动仍回到原位。
  if(state.mode==='move'&&snapRef.current==='dock')onChange({...state.origin,width:dockRef.current.width,height:dockRef.current.height,floating:false});
  setSnap(null);
 };
 const gripKey=(event:React.KeyboardEvent<HTMLDivElement>)=>{
  if(event.key==='Enter'||event.key===' '){event.preventDefault();toggleDock();return;}
  if(!dock.floating)return;
  const step=event.shiftKey?24:8;
  const delta:Record<string,[number,number]>={ArrowLeft:[-step,0],ArrowRight:[step,0],ArrowUp:[0,-step],ArrowDown:[0,step]};
  const shift=delta[event.key];if(!shift)return;
  event.preventDefault();onChange({...dock,...bounded(dock.x+shift[0],dock.y+shift[1])});
 };
 const resizeKey=(event:React.KeyboardEvent<HTMLDivElement>)=>{
  if(!dock.floating)return;
  const step=event.shiftKey?40:16;
  const dw=event.key==='ArrowRight'?step:event.key==='ArrowLeft'?-step:0;
  const dh=event.key==='ArrowDown'?step:event.key==='ArrowUp'?-step:0;
  if(!dw&&!dh)return;
  event.preventDefault();
  const width=clamp(dock.width+dw,220,Math.max(220,window.innerWidth-16));
  const height=clamp(dock.height+dh,160,Math.max(160,window.innerHeight-16));
  onChange({...dock,width,height,...bounded(dock.x,dock.y,width,height)});
 };
 const style:React.CSSProperties=dock.floating?(collapsed?{left:dock.x,top:dock.y}:{left:dock.x,top:dock.y,width:dock.width,height:dock.height}):{};
 return <>
  {snap==='dock'&&<div className={`dock-snap-target is-${edge}`} style={edge==='left'?{width:dockedSize}:{height:dockedSize}}><span>{snapLabel}</span></div>}
  {snap&&snap!=='dock'&&<div className={`dock-snap-edge is-${snap}`}/>}
  {slot}
  <div ref={root}
   className={`${className}${dock.floating?' is-floating':''}${collapsed?' is-collapsed':''}${dragging?' is-dragging':''}`}
   style={style}
   onPointerDown={begin} onPointerMove={move} onPointerUp={end} onPointerCancel={end}
   onDoubleClick={event=>{const target=event.target as HTMLElement;if(target.closest('.dock-grip,.panel-heading')&&!target.closest('button'))toggleDock();}}>
   <div className="dock-grip sidebar-dock-grip" role="button" tabIndex={0} aria-label="拖动面板；双击切换停靠；方向键微调浮窗位置" title="拖动以浮动 / 移动面板 · 靠近停靠边松手可吸附回去" onKeyDown={gripKey}><i/><i/><i/></div>
   {children({floating:dock.floating,dragging,toggleDock})}
   {dock.floating&&!collapsed&&<div className="dock-resize-handle" role="separator" tabIndex={0} aria-label="拖动调整浮窗尺寸" title="拖动调整浮窗大小 · 方向键也可以微调" onKeyDown={resizeKey}><i/></div>}
  </div>
 </>;
}
