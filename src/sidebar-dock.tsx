import React,{useEffect,useRef,useState} from 'react';
import {DockFrame,safePanelDock,usePanelDock,type DockApi,type PanelDockState} from './panel-dock';

const STORAGE_KEY='sound-workbench-sidebar-dock-v1';
/** 侧栏浮窗沿用通用 DockFrame：吸附边=左侧，尺寸与位置单独持久化。 */
export type SidebarDockState=PanelDockState;
export const defaultDock={floating:false,x:110,y:120,width:248,height:520};
export function safeDock(raw:unknown):SidebarDockState{return safePanelDock(raw,defaultDock);}
export function useSidebarDock(){return usePanelDock(STORAGE_KEY,defaultDock);}

export type SidebarDockApi=DockApi;
/** 展开态按住左侧竖条或标题栏即可拖出为浮窗，靠近最左侧出现吸附预览，松手重新停靠。 */
export function SidebarDockFrame({dock,onChange,collapsed,children}:{
 dock:SidebarDockState;
 onChange:(next:SidebarDockState)=>void;
 collapsed:boolean;
 children:(api:SidebarDockApi)=>React.ReactNode;
}){
 const slot=dock.floating?<div className="sidebar-dock-slot"><button title="把侧栏重新停靠到左侧" aria-label="重新停靠侧栏" onClick={()=>onChange({...dock,floating:false})}>⇤</button></div>:null;
 return <DockFrame dock={dock} onChange={onChange} collapsed={collapsed} className="sidebar-dock" edge="left" dockedSize={248} snapLabel="松手停靠到左侧" slot={slot}>
  {api=>children(api)}
 </DockFrame>;
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
