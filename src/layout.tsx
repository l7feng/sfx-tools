import React,{useEffect,useState} from 'react';
import {safeLayout,type Layout} from './workbench-state';
const key='sound-workbench-layout-v1';
export function useWorkbenchLayout(remember=true){
 const [layout,setLayout]=useState<Layout>(()=>{try{return remember?safeLayout(JSON.parse(localStorage.getItem(key)??'{}')):safeLayout(null);}catch{return safeLayout(null);}});
 useEffect(()=>{const timer=setTimeout(()=>{try{if(remember)localStorage.setItem(key,JSON.stringify(layout));else localStorage.removeItem(key);}catch{/* 存储不可用仍可操作 */}},150);return()=>clearTimeout(timer);},[layout,remember]);
 return {layout,setLayout,style:{'--sidebar-width':`${layout.sidebar}px`,'--inspector-width':`${layout.inspector}px`,'--wave-height':`${layout.wave}px`} as React.CSSProperties};
}
export function ResizeHandle({label,value,onChange,axis='x',reverse=false}:{label:string;value:number;onChange:(value:number)=>void;axis?:'x'|'y';reverse?:boolean}){
 const start=React.useRef<{point:number;value:number}|null>(null),latest=React.useRef(onChange);latest.current=onChange;
 const sign=reverse?-1:1;
 return <div className={`resize-handle resize-${axis}`} role="separator" tabIndex={0} aria-label={label} aria-orientation={axis==='x'?'vertical':'horizontal'} aria-valuenow={Math.round(value)} onDoubleClick={()=>onChange(axis==='y'?110:label.includes('详情')?260:label.includes('侧栏')?220:120)} onPointerDown={event=>{if(event.button!==0)return;event.preventDefault();event.currentTarget.setPointerCapture(event.pointerId);start.current={point:axis==='x'?event.clientX:event.clientY,value};}} onPointerMove={event=>{if(!start.current)return;latest.current(start.current.value+((axis==='x'?event.clientX:event.clientY)-start.current.point)*sign);}} onPointerUp={event=>{start.current=null;if(event.currentTarget.hasPointerCapture(event.pointerId))event.currentTarget.releasePointerCapture(event.pointerId);}} onPointerCancel={()=>{start.current=null;}} onKeyDown={event=>{if(['ArrowLeft','ArrowUp','ArrowRight','ArrowDown'].includes(event.key)){event.preventDefault();onChange(value+(['ArrowRight','ArrowDown'].includes(event.key)?10:-10)*sign);}}}/>;
}
