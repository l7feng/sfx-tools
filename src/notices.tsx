import React,{useEffect,useRef,useState,useSyncExternalStore} from 'react';
import './notices.css';

/**
 * 瞬时提示（toast）总线：任何模块都能调用 notify()，
 * 同一个标题+说明在可见期内重复触发会合并成计数，避免连点刷屏。
 */
export type NoticeKind='info'|'success'|'warning'|'error';
type Notice={id:number;kind:NoticeKind;title:string;detail:string;count:number;life:number;version:number};
const LIFE:Record<NoticeKind,number>={info:4000,success:4000,warning:6000,error:8000};
const ICON:Record<NoticeKind,string>={info:'i',success:'✓',warning:'!',error:'✕'};
let notices:Notice[]=[];let sequence=0;
const listeners=new Set<()=>void>();
const emit=()=>{for(const listener of listeners)listener();};
const subscribe=(listener:()=>void)=>{listeners.add(listener);return()=>{listeners.delete(listener);};};
const snapshot=()=>notices;

export function notify(kind:NoticeKind,title:string,detail=''){
 const same=notices.find(item=>item.title===title&&item.detail===detail);
 if(same){same.count+=1;same.version+=1;same.kind=kind;emit();return;}
 notices=[{id:++sequence,kind,title,detail,count:1,life:LIFE[kind],version:1},...notices].slice(0,3);
 emit();
}
export function dismissNotice(id:number){
 notices=notices.filter(item=>item.id!==id);emit();
}

/** 单条 toast：悬停暂停计时，合并时按 version 重置进度。 */
function NoticeCard({item}:{item:Notice}){
 const [paused,setPaused]=useState(false);
 const remaining=useRef(item.life);
 useEffect(()=>{remaining.current=item.life;setPaused(false);},[item.version,item.life]);
 useEffect(()=>{
  if(paused)return;
  const started=Date.now();
  const timer=window.setTimeout(()=>dismissNotice(item.id),remaining.current);
  return()=>{clearTimeout(timer);remaining.current=Math.max(0,remaining.current-(Date.now()-started));};
 },[paused,item.version,item.id]);
 return <article className={`toast ${item.kind}`} role={item.kind==='error'?'alert':'status'}
  onPointerEnter={()=>setPaused(true)} onPointerLeave={()=>setPaused(false)}>
  <span className="toast-icon" aria-hidden="true">{ICON[item.kind]}</span>
  <div className="toast-body">
   <strong>{item.title}{item.count>1&&<em> ×{item.count}</em>}</strong>
   {item.detail&&<p title={item.detail}>{item.detail}</p>}
  </div>
  <button className="toast-close" aria-label="关闭这条提示" onClick={()=>dismissNotice(item.id)}>✕</button>
  <i className="toast-timer" key={item.version} style={{animationDuration:`${item.life}ms`}}/>
 </article>;
}

/** 右上角常驻宿主；为空时不占位。 */
export function NoticeStack(){
 const items=useSyncExternalStore(subscribe,snapshot,snapshot);
 if(!items.length)return null;
 return <div className="toast-stack" aria-live="polite" aria-atomic="false">{items.map(item=><NoticeCard key={item.id} item={item}/>)}</div>;
}
