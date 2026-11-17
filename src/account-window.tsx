import React,{useEffect,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {EmailAuthForm} from './email-auth-form';
import {AccountPanel} from './account-panel';
import './account-panel.css';
import './account-window.css';

type LicenseStatus={development:boolean;production_configured:boolean;basic_features_allowed:boolean;device_id:string;configured:boolean;endpoint:string|null;public_key_sha256:string|null;logged_in:boolean;offline:{state:string;license_id?:string;expires_at?:number|null;error?:string};notice:string};
type Account={email:string;orders:{id:string;state:string;created:number}[];devices:{device_id:string;order_id:string;active:number;changed:number}[]};
const BIND_KEY='sound-workbench-phone-binding-v1';
const READ_SCOPES=['GitHub · read:user / user:email','Google · openid / email / profile'];
const GATES=['买断与模拟订单','第二台设备激活','离线凭证签发','云端同步'];
const mask=(value:string)=>value.length>=7?`${value.slice(0,3)}****${value.slice(-4)}`:value;

/**
 * 账号窗口：GitHub / Google 登录 + 强制绑定手机。
 * 1000×806，左栏登录（398px），右栏步骤条、绑定、拦截点与设备卡。
 * 说明：OAuth 与短信通道都还没接入发行端，界面如实标注「未配置」，不做假成功。
 */
export function AccountWindow({native,open,onClose}:{native:boolean;open:boolean;onClose:()=>void}){
 const dialog=useRef<HTMLDialogElement>(null);
 const [state,setState]=useState<LicenseStatus|null>(null),[account,setAccount]=useState<Account|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState(''),[message,setMessage]=useState('');
 const [phone,setPhone]=useState(''),[code,setCode]=useState(''),[devOpen,setDevOpen]=useState(false),[hint,setHint]=useState<'github'|'google'|null>(null);
 const [bound,setBound]=useState<{masked:string;at:number}|null>(()=>{try{const raw=localStorage.getItem(BIND_KEY);return raw?JSON.parse(raw):null;}catch{return null;}});
 const alive=useRef(true),lock=useRef(false);
 useEffect(()=>{alive.current=true;return()=>{alive.current=false;};},[]);
 useEffect(()=>{const element=dialog.current;if(!element)return;if(open&&!element.open)element.showModal();else if(!open&&element.open)element.close();},[open]);
 useEffect(()=>{
  if(!open||!native)return;let cancelled=false;
  const fail=(e:unknown)=>{if(!cancelled)setError(String(e));};
  try{void invoke<LicenseStatus>('licensing_status').then(value=>{if(!cancelled){setState(value);if(value.logged_in)void invoke<Account>('licensing_request',{action:'account',payload:{}}).then(item=>{if(!cancelled)setAccount(item);}).catch(()=>{});}},fail);}catch(e){fail(e);}
  return()=>{cancelled=true;};
 },[open,native]);
 async function refresh(){const value=await invoke<LicenseStatus>('licensing_status');if(alive.current)setState(value);return value;}
 async function run(work:()=>Promise<void>){
  if(lock.current)return;lock.current=true;setBusy(true);setError('');setMessage('');
  try{await work();}catch(e){if(alive.current)setError(String(e));}
  finally{lock.current=false;if(alive.current)setBusy(false);}
 }
 const request=(action:'logout'|'account')=>void run(async()=>{
  await invoke('licensing_request',{action,payload:{}});
  const current=await refresh();
  if(action==='logout'){setAccount(null);setMessage('已退出登录，未解除设备绑定。');}
  else{if(current.logged_in)setAccount(await invoke<Account>('licensing_request',{action:'account',payload:{}}).catch(()=>null));setMessage('已刷新账号与设备。');}
 });
 const submitEmail=(action:'register'|'login',email:string,password:string)=>void run(async()=>{
  await invoke('licensing_request',{action,payload:{email,password}});
  const current=await refresh();
  if(current.logged_in)setAccount(await invoke<Account>('licensing_request',{action:'account',payload:{}}).catch(()=>null));
  setMessage(action==='register'?'测试账号已创建，请登录。':'本次开发登录成功；会话只保存在内存。');
 });
 const explainOauth=(provider:'github'|'google')=>{
  setHint(provider);
 };
 const loggedIn=!!state?.logged_in;
 const step=bound?3:loggedIn?2:1;
 const devices=account?.devices??[];
 const canOauth=!!state?.configured;
 return <dialog ref={dialog} className="account-window" onCancel={onClose} onClose={onClose} aria-labelledby="account-title">
  <header className="account-head">
   <h2 id="account-title">账号</h2><span className="account-sub">登录 · 绑定 · 设备</span>
   <span className="account-head-sp"/>
   <div className="account-pills">
    <span className={`account-pill${canOauth?' on':''}`}>{canOauth?'● GitHub 可配置':'○ GitHub 未配置'}</span>
    <span className={`account-pill${canOauth?' on':''}`}>{canOauth?'● Google 可配置':'○ Google 未配置'}</span>
    <span className="account-pill">短信：未配置</span>
   </div>
   <button className="account-close" onClick={onClose} aria-label="关闭账号窗口">✕</button>
  </header>
  <div className="account-body">
   <section className="account-left">
    <h3>登录或注册</h3>
    <p className="account-lead">用一个已有账号完成登录和注册，不需要在应用里设置密码。授权在系统浏览器中完成，应用不接触你的密码。</p>
    <button className={`account-oauth${hint==='github'?' is-active':''}`} onClick={()=>explainOauth('github')}>
     <span className="account-lg">GH</span><b>使用 GitHub 继续</b><span className="account-oauth-r">{canOauth?'↗ 浏览器':'待配置'}</span>
    </button>
    <button className={`account-oauth${hint==='google'?' is-active':''}`} onClick={()=>explainOauth('google')}>
     <span className="account-lg">G</span><b>使用 Google 继续</b><span className="account-oauth-r">{canOauth?'↗ 浏览器':'待配置'}</span>
    </button>
    {hint&&<p className="account-oauth-note" role="status">
     <b>{hint==='github'?'GitHub':'Google'} 授权需要发行端提供两样东西</b>
     ① 第三方 OAuth 应用（Client ID；GitHub 另需 Client Secret，Google 桌面端可不带）；② 服务端用 code 换 token 的地址——Secret 不能进客户端。
     桌面端流程是「系统浏览器授权 + 127.0.0.1 随机端口回环回调 + PKCE」；GitHub 也可改用 Device Flow 六位码，不需要回环端口。
    </p>}
    <div className="account-orline"><span/><em>或使用邮箱</em><span/></div>
    {native?<EmailAuthForm enabled={!!state?.configured&&!busy} busy={busy} onSubmit={submitEmail}/>:<p className="account-lead">网页预览里没有本地开发服务，邮箱登录不可用。</p>}
    {loggedIn&&<div className="account-logged"><span>已登录：{account?.email??state?.development?'本地开发账号':'—'}</span><button disabled={busy} onClick={()=>request('logout')}>退出登录</button></div>}
    <p className="account-note">读到的只有<b>公开资料</b>（用户名、头像）和<b>已验证邮箱</b>，不会请求仓库、组织、联系人等权限。退出登录只清除本机会话，不解除设备绑定。</p>
   </section>

   <section className="account-right">
    <div className="account-steps" aria-label="绑定流程">
     <span className={`account-step${step>1?' done':step===1?' now':''}`}><span className="account-step-b">{step>1?'✓':'1'}</span>授权登录</span>
     <span className="account-step-ln"/>
     <span className={`account-step${step>2?' done':step===2?' now':''}`}><span className="account-step-b">{step>2?'✓':'2'}</span>绑定手机</span>
     <span className="account-step-ln"/>
     <span className={`account-step${step===3?' done':''}`}><span className="account-step-b">3</span>完成</span>
    </div>

    <div className="account-card">
     <div className="account-card-head"><b>绑定手机号</b><span className="account-tag">必填</span></div>
     <div className="account-phone">
      <label className="account-cc">+86<span aria-hidden="true">▾</span><select aria-label="国家码" defaultValue="86"><option value="86">中国 +86</option><option value="852">中国香港 +852</option><option value="886">中国台湾 +886</option></select></label>
      <label className="account-pin"><input value={phone} inputMode="numeric" maxLength={11} aria-label="手机号" placeholder="11 位手机号" onChange={event=>setPhone(event.target.value.replace(/\D/g,''))}/></label>
     </div>
     <div className="account-code">
      <label className="account-code-input"><input value={code} inputMode="numeric" maxLength={6} aria-label="验证码" placeholder="6 位验证码" onChange={event=>setCode(event.target.value.replace(/\D/g,''))}/></label>
      <button className="account-send" disabled title="短信签名与模板未配置，暂不能发送验证码">{loggedIn?'获取验证码':'登录后可获取'}</button>
     </div>
     <div className="account-actions">
      <button className="primary" disabled={!loggedIn||phone.length<11||code.length!==6||busy} title="接入短信服务后启用；当前未配置签名与模板" onClick={()=>{
       const value={masked:mask(phone),at:Date.now()};
       try{localStorage.setItem(BIND_KEY,JSON.stringify(value));}catch{/* 存储不可用不影响本次会话 */}
       setBound(value);setPhone('');setCode('');
       setMessage(`${value.masked} 已记录在本机；短信通道未接入，此次仅完成界面演示。`);
      }}>验证并绑定</button>
      <button className="account-skip" onClick={onClose}>暂不绑定</button>
     </div>
     <p className="account-gate"><b>未绑定前</b>可以正常读取、试听、导出个人素材；{GATES.join('、')}需先完成绑定。短信签名与模板审核通过后，这里才真正发码。</p>
    </div>
    {error&&<p className="account-error" role="alert">{error}</p>}
    {message&&<p className="account-message" role="status">{message}</p>}

    <ul className="account-reasons">
     <li>找回账号与人工客服核对身份的唯一凭据</li>
     <li>双设备名额（最多 2 台）与解绑操作需要手机验证码确认</li>
     <li>发票抬头、退款、跨设备迁移必须能联系到本人</li>
     <li>换设备时凭手机号重新签发离线凭证，不用等客服</li>
    </ul>

    <div className="account-stats">
     <div className="account-stat"><span>登录方式</span><b>{loggedIn?(account?.email?'邮箱 · 本地开发':'本地开发账号'):'未登录'}</b></div>
     <div className="account-stat"><span>账号</span><b>{account?.email?account.email.split('@')[0]:'—'}</b></div>
     <div className="account-stat"><span>设备</span><b>{devices.filter(item=>item.active).length} <small>/ 2</small></b></div>
     <div className="account-stat"><span>手机绑定</span><b className={bound?'is-on':'is-warn'}>{bound?'已记录（未验证）':'未绑定'}</b></div>
    </div>

    {bound&&<div className="account-bound"><span className="account-bound-ic">✓</span><span>{bound.masked}</span><span className="account-bound-sp"/><button onClick={()=>{try{localStorage.removeItem(BIND_KEY);}catch{/* 忽略 */}setBound(null);}}>解除本机记录</button></div>}

    <div className="account-card account-devices-card">
     <div className="account-card-head"><b>设备与授权</b><span className="account-tag is-dim">{devices.filter(item=>item.active).length} / 2</span></div>
     <div className="account-dev">
      {devices.length?devices.map(device=><div className="account-drow" key={device.device_id}>
       <span className="account-di">▣</span>
       <div className="account-dm"><b>{device.device_id===state?.device_id?'本机':'已激活设备'}</b><span>{device.device_id.slice(0,8)}… · {device.active?'使用中':'已解绑'}</span></div>
       <span className={`account-da${device.active?'':' off'}`}>{device.active?'激活':'解绑'}</span>
      </div>):<div className="account-drow">
       <span className="account-di">▣</span>
       <div className="account-dm"><b>本机 · {state?.device_id.slice(0,8)??'读取中'}…</b><span>未登录，仅本机标识；登录后可查看双设备名额</span></div>
       <span className={`account-da${loggedIn?'':' off'}`}>{loggedIn?'可激活':'未登录'}</span>
      </div>}
     </div>
     <div className="account-actions account-device-actions">
      <button disabled={!loggedIn||busy} onClick={()=>request('account')}>刷新授权状态</button>
      <button disabled>解绑本机</button>
     </div>
     <p className="account-gate"><b>授权说明</b>会话只保存在本机内存；解绑、退款在联网核验后生效，永久离线凭证不能即时撤销。</p>
    </div>

    {devOpen&&<section className="account-devpanel"><AccountPanel native={native} initiallyOpen/></section>}
   </section>
  </div>
  <footer className="account-foot">
   <button className="account-foot-link" onClick={()=>setDevOpen(value=>!value)}>{devOpen?'收起本地开发服务':'本地开发服务 / 测试订单'}</button>
   <span className="account-foot-sep"/>
   <span>{state?.configured?`服务 ${state.endpoint}`:'未配置本地开发服务'}</span>
   <span className="account-foot-sp"/>
   <span>会话保存在本机内存，关闭应用后需重新登录</span>
  </footer>
 </dialog>;
}
