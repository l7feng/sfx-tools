import React,{useEffect,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {displayPath} from '../catalog-client';
import {PLUGIN_META,pluginApi,type PluginId,type PluginResult} from './client';
import './plugin-workspace.css';

/** 0.1.35：可插拔模块的「工作区」窗口，入口在顶栏「音乐库」右侧。
 *  0.1.41 起只保留三个可插拔模块；外部程序（Audio Dedup）挪到「管理 › 扩展」，
 *  本窗口不再兼任外部程序的启动器。
 *
 *  窗口里只做能被验证的事：
 *  - AAF/OMF：选文件 → 读文件头（静态探测）→ 取候选；
 *  - PT 叠层：本机只读探测能力与状态；
 *  - PT 批量导出：选输出目录 → 出预览清单（不渲染、不写盘）。
 *  写入类命令后端恒返回 WRITE_BLOCKED，这里也不放写入按钮。 */
type Flags=Record<PluginId,boolean>;
// 记住上次打开的模块，下次打开直接回到这里。
// ?pluginItem=<id> 只给本地预览脚本指定初始项，正常启动不带这个参数。
function initialItem():PluginId{
 const fromQuery=new URLSearchParams(location.search).get('pluginItem');
 const valid=(value:string|null):value is PluginId=>PLUGIN_META.some(item=>item.id===value);
 if(valid(fromQuery))return fromQuery;
 try{const saved=localStorage.getItem('sound-workbench-plugin-last');if(valid(saved))return saved;}catch{/* 存储不可用就用默认 */}
 return 'sfx_aaf_import';
}

const STEPS:Record<PluginId,string>={
 sfx_aaf_import:'选择 AAF／OMF 文件后先读文件头，得到静态探测结果；再取候选素材清单。整个过程只读，不解析完整时间线、不改任何工程。',
 sfx_pt_overlay:'探测本机能给 Pro Tools 提供什么（能力清单）以及当前连接状态；未连接 PT 时如实返回不可用，不假装成功。',
 sfx_pt_bouncer:'选择一个输出目录，生成本次要导出的预览清单（条数、目标、预计体积）；不渲染音频、不写盘。',
};

function resultRows(result:PluginResult){
 const data=result.data;
 if(data===null||data===undefined)return '';
 if(typeof data==='object'){
  const entries=Object.entries(data as Record<string,unknown>).slice(0,12);
  return entries.map(([key,value])=>{
   const text=typeof value==='string'?value:JSON.stringify(value);
   return `${key}: ${text&&text.length>120?text.slice(0,120)+'…':text}`;
  }).join('\n');
 }
 return String(data);
}

function Evidence({value}:{value:string}){
 const labels:Record<string,string>={static_parse:'静态文件头',local_probe:'本机探测',pt_live:'PT 实时',contract_only:'仅契约',none:'无证据'};
 return <span className={`pw-evidence ev-${value}`} title="这条结果的证据来源，决定它能被信任到什么程度">{labels[value]??value}</span>;
}

export function PluginWorkspace({open,onClose,flags,native}:{open:boolean;onClose:()=>void;flags:Flags;native:boolean}){
 const dialog=useRef<HTMLDialogElement>(null),[active,setActive]=useState<PluginId>(initialItem);
 const [busy,setBusy]=useState(false),[error,setError]=useState('');
 const [aafPath,setAafPath]=useState(''),[aafParse,setAafParse]=useState<PluginResult|null>(null),[aafCands,setAafCands]=useState<PluginResult|null>(null);
 const [overlay,setOverlay]=useState<{cap:PluginResult|null;status:PluginResult|null}>({cap:null,status:null});
 const [bouncerPath,setBouncerPath]=useState(''),[bouncer,setBouncer]=useState<PluginResult|null>(null);

 useEffect(()=>{
  const node=dialog.current;if(!node)return;
  if(open&&!node.open)node.showModal();else if(!open&&node.open)node.close();
 },[open]);
 useEffect(()=>{if(open)setError('');},[open]);
 useEffect(()=>{try{localStorage.setItem('sound-workbench-plugin-last',active);}catch{/* 存储不可用不影响使用 */}},[active]);

 async function run_(work:()=>Promise<void>){if(busy)return;setBusy(true);setError('');try{await work();}catch(e){setError(String(e));}finally{setBusy(false);} }

 const pickAaf=()=>void run_(async()=>{const path=await invoke<string|null>('plugin_pick_file',{initial:aafPath||null,title:'选择 AAF / OMF 文件'});if(path){setAafPath(path);setAafParse(null);setAafCands(null);}});
 const parseAaf=()=>void run_(async()=>{const result=await pluginApi.aafParse({sourcePath:aafPath,readOnly:true});setAafParse(result);setAafCands(null);});
 const loadCands=()=>void run_(async()=>{const parseId=(aafParse?.data as {parse_id?:string}|null)?.parse_id;const result=pluginApi.aafCandidates({parseId,reviewed:false});setAafCands(await result);});
 const probeOverlay=()=>void run_(async()=>{const [cap,status]=await Promise.all([pluginApi.overlayCapabilities(),pluginApi.overlayStatus()]);setOverlay({cap,status});});
 const pickBouncer=()=>void run_(async()=>{const path=await invoke<string|null>('catalog_pick_folder',{initial:bouncerPath||null});if(path)setBouncerPath(path);});
 const previewBouncer=()=>void run_(async()=>{setBouncer(await pluginApi.bouncerPreview({outputDir:bouncerPath}));});

 const meta=PLUGIN_META.find(item=>item.id===active)!;
 const enabled=flags[active];
 return <dialog ref={dialog} className="plugin-workspace" aria-label="插件工作区" onClose={onClose}>
  <header className="pw-head">
   <div><h1>插件工作区</h1><span className="pw-mode">只读模式 · 写入类命令后端恒阻断</span></div>
   <button aria-label="关闭插件工作区" onClick={onClose}>×</button>
  </header>
  <div className="pw-body">
   <nav className="pw-rail" aria-label="可插拔模块">
    {PLUGIN_META.map(item=><button key={item.id} aria-current={active===item.id?'page':undefined} className={active===item.id?'is-active':''} onClick={()=>{setActive(item.id);setError('');}}>
     <span className="pw-rail-name">{item.label}</span>
     <span className={`pw-rail-state${flags[item.id]?' on':''}`}>{flags[item.id]?'已启用':'未启用'}</span>
    </button>)}
    <p className="pw-rail-note">开关在「管理 › 可插拔模块」里，两层闸门：Cargo feature ＋ 设置开关。外部程序（Audio Dedup）在「管理 › 扩展」里启动，不占这个窗口。</p>
   </nav>
   <section className="pw-work" aria-label={`${meta.label} 工作区`}>
    <div className="pw-title"><h2>{meta.label}</h2><code>{meta.id}</code><span className="pw-deps">{meta.dependency}</span></div>
    {!enabled&&<p className="pw-warn" role="status">此模块在设置里未启用：命令会被后端拒绝执行。请先到「管理 › 可插拔模块」打开开关。</p>}
    <p className="pw-steps">{STEPS[active]}</p>

    {active==='sfx_aaf_import'&&<>
     <div className="pw-row"><button className="pw-primary" disabled={busy||!native} onClick={pickAaf}>选择 AAF／OMF 文件…</button><span className="pw-path" title={aafPath}>{aafPath?displayPath(aafPath):'未选择文件'}</span></div>
     <div className="pw-row"><button disabled={busy||!native||!aafPath} onClick={parseAaf}>解析文件头（只读）</button><button disabled={busy||!native||!aafParse?.ok} onClick={loadCands}>取候选素材</button></div>
     {aafParse&&<ResultCard title="文件头探测" result={aafParse}/>}
     {aafCands&&<ResultCard title="候选素材" result={aafCands}/>}
    </>}

    {active==='sfx_pt_overlay'&&<>
     <div className="pw-row"><button className="pw-primary" disabled={busy||!native} onClick={probeOverlay}>本机只读探测</button><span className="pw-hint">读能力清单 + 连接状态，不接触任何 PT 会话</span></div>
     {overlay.cap&&<ResultCard title="能力清单" result={overlay.cap}/>}
     {overlay.status&&<ResultCard title="连接状态" result={overlay.status}/>}
    </>}

    {active==='sfx_pt_bouncer'&&<>
     <div className="pw-row"><button className="pw-primary" disabled={busy||!native} onClick={pickBouncer}>选择输出目录…</button><span className="pw-path" title={bouncerPath}>{bouncerPath?displayPath(bouncerPath):'未选择目录'}</span></div>
     <div className="pw-row"><button disabled={busy||!native||!bouncerPath} onClick={previewBouncer}>生成导出预览</button><span className="pw-hint">只出清单，不渲染音频</span></div>
     {bouncer&&<ResultCard title="导出预览" result={bouncer}/>}
    </>}

    {error&&<p className="pw-error" role="alert">{error}</p>}
    <footer className="pw-foot">本窗口不提供任何写入入口：<code>{meta.write}</code> 在后端恒返回 WRITE_BLOCKED，未连接真实 Pro Tools、未经过真实工程验证前不应放开。</footer>
   </section>
  </div>
 </dialog>;
}

function ResultCard({title,result}:{title:string;result:PluginResult}){
 const rows=resultRows(result);
 return <article className={`pw-result${result.ok?'':' is-fail'}`} aria-label={title}>
  <header><strong>{title}</strong><span className="pw-code">{result.code}</span><Evidence value={result.evidence}/></header>
  <p>{result.message}</p>
  {rows&&<pre>{rows}</pre>}
  {result.warnings.length>0&&<ul>{result.warnings.slice(0,4).map((item,i)=><li key={i}>{item}</li>)}</ul>}
 </article>;
}
