import React,{useEffect,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import './account-menu.css';
type LicenseStatus={development:boolean;configured:boolean;logged_in:boolean;device_id:string;offline:{state:string;license_id?:string};notice:string};
const offlineLabels:Record<string,string>={none:'无离线凭证',verified_development:'开发凭证已验证',invalid:'凭证不可用'};
export function AccountMenu({native,onManage,notify,initiallyOpen=false}:{native:boolean;onManage:()=>void;notify:(message:string)=>void;initiallyOpen?:boolean}){
 const [open,setOpen]=useState(initiallyOpen),[state,setState]=useState<LicenseStatus|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState('');
 const root=useRef<HTMLDivElement>(null),trigger=useRef<HTMLButtonElement>(null),alive=useRef(true);
 useEffect(()=>{alive.current=true;return()=>{alive.current=false;};},[]);
 useEffect(()=>{
  if(!open)return;
  const outside=(event:PointerEvent)=>{if(!root.current?.contains(event.target as Node))setOpen(false);};
  const key=(event:KeyboardEvent)=>{if(event.key==='Escape'){setOpen(false);trigger.current?.focus();}};
  document.addEventListener('pointerdown',outside);document.addEventListener('keydown',key);
  root.current?.querySelector<HTMLButtonElement>('.account-menu button:not(:disabled)')?.focus();
  return()=>{document.removeEventListener('pointerdown',outside);document.removeEventListener('keydown',key);};
 },[open]);
 useEffect(()=>{if(!open||!native)return;let cancelled=false;invoke<LicenseStatus>('licensing_status').then(value=>{if(!cancelled)setState(value);},e=>{if(!cancelled)setError(String(e));});return()=>{cancelled=true;};},[open,native]);
 const run=async(action:string)=>{
  if(busy)return;setBusy(true);setError('');
  try{await invoke('licensing_request',{action,payload:{}});const value=await invoke<LicenseStatus>('licensing_status');if(alive.current)setState(value);notify(action==='logout'?'已退出登录，未解绑设备。':'已刷新授权状态。');}
  catch(e){if(alive.current)setError(String(e));}
  finally{if(alive.current)setBusy(false);}
 };
 const loggedIn=!!state?.logged_in;
 const menuKeys=(event:React.KeyboardEvent<HTMLDivElement>)=>{
  const buttons=Array.from(root.current?.querySelectorAll<HTMLButtonElement>('.account-menu button:not(:disabled)')??[]);
  const index=buttons.indexOf(document.activeElement as HTMLButtonElement);
  if(['ArrowDown','ArrowUp','Home','End'].includes(event.key)){event.preventDefault();buttons[event.key==='Home'?0:event.key==='End'?buttons.length-1:(index+(event.key==='ArrowDown'?1:-1)+buttons.length)%buttons.length]?.focus();}
 };
 return <div className="account-menu-wrap" ref={root}>
  <button ref={trigger} className="account-trigger" aria-haspopup="menu" aria-expanded={open} title="账号菜单：登录状态、刷新授权与退出登录" onClick={()=>setOpen(value=>!value)}>账号<span className={`account-dot${loggedIn?' is-on':''}`} aria-hidden="true"/></button>
  {open&&<div className="account-menu" role="menu" aria-label="账号菜单" onKeyDown={menuKeys}>
   <div className="account-menu-status"><strong>{state?loggedIn?'已登录本地开发账号':'未登录':'读取中…'}</strong><span>{state?offlineLabels[state.offline.state]??state.offline.state:'正在读取授权状态'} · 订单与支付均为本地模拟</span></div>
   <button role="menuitem" onClick={()=>{setOpen(false);onManage();}}>账号与授权管理…</button>
   <button role="menuitem" disabled={!native||busy||!loggedIn} onClick={()=>void run('account')}>{busy?'处理中…':'刷新授权'}</button>
   <button role="menuitem" disabled={!native||busy||!loggedIn} onClick={()=>void run('logout')}>退出登录</button>
   {error&&<p className="account-menu-error" role="alert">{error}</p>}
   {!native&&<p className="account-menu-note">账号菜单仅在桌面版可用。</p>}
  </div>}
 </div>;
}