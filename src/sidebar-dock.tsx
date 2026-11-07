import React,{useEffect,useRef,useState} from 'react';

const STORAGE_KEY='sound-workbench-sidebar-dock-v1';
const clamp=(value:number,min:number,max:number)=>Math.min(Math.max(value,min),Math.max(min,max));

/** floating=false 时侧栏照旧停在 .workspace 左侧；true 时脱离布局成为可拖动的浮窗。 */
export type SidebarDockState={floating:boolean;x:number;y:number};
export const defaultDock:SidebarDockState={floating:false,x:110,y:120};
export function safeDock(raw:unknown):SidebarDockState{
 const value=(typeof raw==='object'&&raw?raw:{}) as Partial<SidebarDockState>;
 return {
  floating:value.floating===true,
  x:typeof value.x==='number'&&Number.isFinite(value.x)?clamp(value.x,0,6000):defaultDock.x,
  y:typeof value.y==='number'&&Number.isFinite(value.y)?clamp(value.y,0,6000):defaultDock.y
 };
}
/** 浮窗位置与停靠状态单独持久化，避免挤进 layout 存档后被「重置布局」一并清掉。 */
export function useSidebarDock(){
 const [dock,setDock]=useState<SidebarDockState>(()=>{try{return safeDock(JSON.parse(localStorage.getItem(STORAGE_KEY)??'{}'));}catch{return defaultDock;}});
 useEffect(()=>{try{localStorage.setItem(STORAGE_KEY,JSON.stringify(dock));}catch{/* 存储不可用时仅本次会话有效 */}},[dock]);
 return {dock,setDock};
}

export type SidebarDockApi={floating:boolean;dragging:boolean;toggleDock:()=>void};
/**
 * 侧栏拖动容器。展开态按住左侧竖条或标题栏即可拖出为浮窗，拖回窗口最左侧重新停靠；
 * 双击手柄、点「停靠」按钮或方向键也能切换/微调。所有指针事件绑在稳定的根节点上，
 * 这样从停靠切到浮动时 React 复用 DOM，pointer capture 不会丢。
 */
export function SidebarDockFrame({dock,onChange,collapsed,children}:{
 dock:SidebarDockState;
 onChange:(next:SidebarDockState)=>void;
 collapsed:boolean;
 children:(api:SidebarDockApi)=>React.ReactNode;
}){
 const root=useRef<HTMLDivElement>(null);
 const drag=useRef<{sx:number;sy:number;dx:number;dy:number;moved:boolean;origin:SidebarDockState}|null>(null);
 const [dragging,setDragging]=useState(false);
 const bounded=(x:number,y:number)=>{
  const element=root.current,width=element?.offsetWidth??240,height=element?.offsetHeight??220;
  return {x:clamp(x,0,window.innerWidth-width-6),y:clamp(y,0,window.innerHeight-height-6)};
 };
 // 窗口缩放后把浮窗拉回可视区，避免拖到屏幕外找不回来。
 useEffect(()=>{
  if(!dock.floating)return;
  const fix=()=>{const next=bounded(dock.x,dock.y);if(next.x!==dock.x||next.y!==dock.y)onChange({...dock,...next});};
  fix();window.addEventListener('resize',fix);
  return()=>window.removeEventListener('resize',fix);
 },[dock,onChange]);
 const toggleDock=()=>onChange(dock.floating?{...dock,floating:false}:{...dock,floating:true,...bounded(dock.x,dock.y)});
 const begin=(event:React.PointerEvent<HTMLDivElement>)=>{
  if(event.button!==0)return;
  const target=event.target as HTMLElement;
  if(target.closest('button,input,select,textarea,a,[role=separator],.sidebar-rail-popover'))return;
  if(!target.closest('.sidebar-dock-grip,.panel-heading'))return;
  const rect=root.current?.getBoundingClientRect();if(!rect)return;
  drag.current={sx:event.clientX,sy:event.clientY,dx:event.clientX-rect.left,dy:event.clientY-rect.top,moved:false,origin:dock};
  event.currentTarget.setPointerCapture(event.pointerId);
 };
 const move=(event:React.PointerEvent<HTMLDivElement>)=>{
  const state=drag.current;if(!state)return;
  if(!state.moved){if(Math.hypot(event.clientX-state.sx,event.clientY-state.sy)<5)return;state.moved=true;setDragging(true);}
  onChange({floating:true,...bounded(event.clientX-state.dx,event.clientY-state.dy)});
 };
 const end=(event:React.PointerEvent<HTMLDivElement>)=>{
  const state=drag.current;drag.current=null;setDragging(false);
  if(event.currentTarget.hasPointerCapture(event.pointerId))event.currentTarget.releasePointerCapture(event.pointerId);
  // 拖到最左侧重新停靠时保留拖动前的浮窗坐标，下次浮动仍回到原位，而不是被夹到 x=0。
  if(state?.moved&&event.clientX<=36)onChange({...state.origin,floating:false});
 };
 const gripKey=(event:React.KeyboardEvent<HTMLDivElement>)=>{
  if(event.key==='Enter'||event.key===' '){event.preventDefault();toggleDock();return;}
  if(!dock.floating)return;
  const step=event.shiftKey?24:8;
  const delta:Record<string,[number,number]>={ArrowLeft:[-step,0],ArrowRight:[step,0],ArrowUp:[0,-step],ArrowDown:[0,step]};
  const shift=delta[event.key];if(!shift)return;
  event.preventDefault();onChange({...dock,...bounded(dock.x+shift[0],dock.y+shift[1])});
 };
 const style:React.CSSProperties=dock.floating?{left:dock.x,top:dock.y}:{};
 return <>
  {dock.floating&&<div className="sidebar-dock-slot"><button title="把侧栏重新停靠到左侧" aria-label="重新停靠侧栏" onClick={()=>onChange({...dock,floating:false})}>⇤</button></div>}
  <div ref={root}
   className={`sidebar-dock${dock.floating?' is-floating':''}${collapsed?' is-collapsed':''}${dragging?' is-dragging':''}`}
   style={style}
   onPointerDown={begin} onPointerMove={move} onPointerUp={end} onPointerCancel={end}
   onDoubleClick={event=>{const target=event.target as HTMLElement;if(target.closest('.sidebar-dock-grip,.panel-heading')&&!target.closest('button'))toggleDock();}}>
   <div className="sidebar-dock-grip" role="button" tabIndex={0} aria-label="拖动侧栏；双击切换停靠；方向键微调浮窗位置" title="拖动以浮动 / 移动侧栏 · 拖回最左侧或双击可重新停靠" onKeyDown={gripKey}><i/><i/><i/></div>
   {children({floating:dock.floating,dragging,toggleDock})}
  </div>
 </>;
}

/** 侧栏导航图标：与工具页图标同一套 24×24 线性几何，折叠态与浮动态共用。 */
const NAV_PATHS={
 all:<><rect x="3.8" y="3.8" width="6.8" height="6.8" rx="1.5"/><rect x="13.4" y="3.8" width="6.8" height="6.8" rx="1.5"/><rect x="3.8" y="13.4" width="6.8" height="6.8" rx="1.5"/><rect x="13.4" y="13.4" width="6.8" height="6.8" rx="1.5"/></>,
 favorites:<path d="M12 20.3 4.7 13a4.8 4.8 0 0 1 6.8-6.8l.5.5.5-.5A4.8 4.8 0 0 1 19.3 13z"/>,
 history:<><circle cx="12" cy="12" r="8.2"/><path d="M12 7.2V12l3.4 2.1"/></>,
 tags:<><path d="M4.2 6.6A2.4 2.4 0 0 1 6.6 4.2h4a1.7 1.7 0 0 1 1.2.5l7.1 7.1a1.7 1.7 0 0 1 0 2.4l-4.7 4.7a1.7 1.7 0 0 1-2.4 0l-7.1-7.1a1.7 1.7 0 0 1-.5-1.2z"/><circle cx="8.6" cy="8.6" r="1.2"/></>,
 playlists:<path d="M6.4 3.8h11.2v16.4L12 16.2l-5.6 4z"/>,
 jobs:<><path d="M12 3.6v9.2"/><path d="M8.4 9.4 12 13l3.6-3.6"/><path d="M4.2 15.4v3.2a1.8 1.8 0 0 0 1.8 1.8h12a1.8 1.8 0 0 0 1.8-1.8v-3.2"/></>,
 dock:<><rect x="3.6" y="4.6" width="16.8" height="14.8" rx="1.8"/><path d="M9 4.6v14.8"/></>,
 float:<><path d="M4 8.4h11.4v10.8H4z"/><path d="M8.8 8.4V5h11.4v10.4h-3.4"/></>
} as const;
export type NavGlyphId=keyof typeof NAV_PATHS;
export function NavGlyph({id}:{id:NavGlyphId}){
 return <svg className="nav-glyph" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" focusable="false">{NAV_PATHS[id]}</svg>;
}

export type RailItem={id:string;label:string;glyph:NavGlyphId;active?:boolean;onClick:(event:React.MouseEvent<HTMLButtonElement>)=>void};
/** 折叠态：一条纯图标导航（对应参考图 #3 的四枚图标 + 下方「…」更多）。 */
export function SidebarIconRail({items,extra,onExpand,floating,toggleDock}:{
 items:RailItem[];
 extra:RailItem[];
 onExpand:()=>void;
 floating:boolean;
 toggleDock:()=>void;
}){
 const [more,setMore]=useState(false);
 const popover=useRef<HTMLDivElement>(null);
 useEffect(()=>{
  if(!more)return;
  const outside=(event:PointerEvent)=>{const target=event.target as HTMLElement;if(!popover.current?.contains(target)&&!target.closest('.sidebar-rail-more'))setMore(false);};
  document.addEventListener('pointerdown',outside,true);
  return()=>document.removeEventListener('pointerdown',outside,true);
 },[more]);
 return <aside className="sidebar-rail" role="toolbar" aria-label="折叠后的素材导航">
  <div className="sidebar-rail-icons">
   <button className="sidebar-rail-icon sidebar-rail-expand" title="展开侧栏" aria-label="展开侧栏" onClick={onExpand}>›</button>
   {items.map(item=><button key={item.id} className={`sidebar-rail-icon${item.active?' is-active':''}`} title={item.label} aria-label={item.label} aria-pressed={item.active||undefined} onClick={item.onClick}><NavGlyph id={item.glyph}/></button>)}
  </div>
  <button className="sidebar-rail-more" title="更多导航" aria-label="更多导航" aria-expanded={more} onClick={()=>setMore(value=>!value)}>···</button>
  {more&&<div className="sidebar-rail-popover" ref={popover} role="menu" aria-label="更多素材导航">
   {extra.map(item=><button key={item.id} role="menuitem" className={item.active?'is-active':''} onClick={event=>{setMore(false);item.onClick(event);}}><NavGlyph id={item.glyph}/><span>{item.label}</span></button>)}
   <hr/>
   <button role="menuitem" onClick={()=>{setMore(false);toggleDock();}}><NavGlyph id={floating?'dock':'float'}/><span>{floating?'停靠到左侧':'浮动为独立窗口'}</span></button>
  </div>}
 </aside>;
}
