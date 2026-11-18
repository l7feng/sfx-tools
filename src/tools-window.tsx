import React,{useEffect,useMemo,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import type {CatalogAsset} from './catalog-client';
import {formatBytes,displayPath} from './catalog-client';
import {timeLabel} from './demo-data';
import {FoundationTools} from './foundation-tools';
import {SmartTools} from './smart-tools';
import {LayerTools} from './layer-tools';
import {PluginPanel} from './plugins/panel';
import {ToolGlyph} from './tool-glyphs';
import {notify} from './notices';
import {toolEntries,toolGroups,toolsOf,type ToolArea,type ToolEntry,type ToolPage,type ToolStatus} from './tool-navigation';
import './tools-window.css';

type CustomLink={id:string;label:string;href:string};
const USAGE_KEY='sound-workbench-tool-usage-v1';
const LINK_KEY='sound-workbench-tool-links-v1';
const CUSTOM_KEY='sound-workbench-tool-custom-links-v1';
const STATUS_LABEL:Record<ToolStatus,string>={ready:'可用',need:'需选素材',dev:'开发中',link:'外链'};
const STATUS_CLASS:Record<ToolStatus,string>={ready:'ok',need:'need',dev:'dev',link:'link'};
const GROUP_LABEL:Record<ToolArea,string>={library:'素材库',production:'制作',info:'信息',extension:'扩展 · 链接',account:'账号'};

function readJson<T>(key:string,fallback:T):T{
 try{const raw=localStorage.getItem(key);return raw?JSON.parse(raw) as T:fallback;}catch{return fallback;}
}
function sinceLabel(stamp:number){
 if(!stamp)return'';
 const diff=Date.now()-stamp;
 if(diff<60_000)return'刚刚';
 if(diff<3_600_000)return`${Math.floor(diff/60_000)} 分钟前`;
 if(diff<86_400_000)return`${Math.floor(diff/3_600_000)} 小时前`;
 const date=new Date(stamp);return`${String(date.getMonth()+1).padStart(2,'0')}-${String(date.getDate()).padStart(2,'0')} ${String(date.getHours()).padStart(2,'0')}:${String(date.getMinutes()).padStart(2,'0')}`;
}

/**
 * 管理窗口：音频工具从顶级页面收进右上角。
 * 1186×818 模态，左侧五组导航、右侧卡片网格；单个工具在窗口内展开，不跳页。
 */
export function ToolsWindow({open,onClose,native,ids,selected,library,tool,onTool,onPick,onChanged,onCheck,onJobs,onAccount,busy}:{
 open:boolean;onClose:()=>void;native:boolean;ids:string[];selected:CatalogAsset|null;library:'sound'|'music';
 tool:ToolPage|null;onTool:(tool:ToolPage|null)=>void;
 onPick:(asset:CatalogAsset)=>void;onChanged:()=>void;onCheck:()=>void;onJobs:()=>void;onAccount:()=>void;busy:boolean;
}){
 const dialog=useRef<HTMLDialogElement>(null),search=useRef<HTMLInputElement>(null);
 const [query,setQuery]=useState(''),[usage,setUsage]=useState<Record<string,number>>(()=>readJson(USAGE_KEY,{})),[links,setLinks]=useState<Record<string,string>>(()=>readJson(LINK_KEY,{})),[custom,setCustom]=useState<CustomLink[]>(()=>readJson(CUSTOM_KEY,[]));
 const [draftName,setDraftName]=useState(''),[draftUrl,setDraftUrl]=useState(''),[editing,setEditing]=useState<string|null>(null),[draftLink,setDraftLink]=useState(''),[flash,setFlash]=useState('');
 // 外部程序（Audio Dedup）的版本与路径：窗口打开时定位一次，直接显示在卡片上，省得点进去才知道装了没。
 const [appInfo,setAppInfo]=useState<{version:string;path:string}|null>(null),[appLocating,setAppLocating]=useState(false);
 useEffect(()=>{const element=dialog.current;if(!element)return;if(open&&!element.open)element.showModal();else if(!open&&element.open)element.close();},[open]);
 useEffect(()=>{if(!open)return;setQuery('');const timer=window.setTimeout(()=>search.current?.focus(),80);return()=>clearTimeout(timer);},[open]);
 // 打开管理窗口时定位一次外部程序，把版本与路径直接显示在卡片上（失败不打扰，点按钮时再让你选）。
 useEffect(()=>{
  if(!open||!native)return;
  let alive=true;setAppLocating(true);
  void invoke<{version:string;path:string}>('dedup_locate')
   .then(info=>{if(alive)setAppInfo(info);})
   .catch(()=>{if(alive)setAppInfo(null);})
   .finally(()=>{if(alive)setAppLocating(false);});
  return()=>{alive=false;};
 },[open,native]);
 useEffect(()=>{try{localStorage.setItem(USAGE_KEY,JSON.stringify(usage));}catch{/* 记录失败不影响使用 */}},[usage]);
 useEffect(()=>{try{localStorage.setItem(LINK_KEY,JSON.stringify(links));}catch{/* 同上 */}},[links]);
 useEffect(()=>{try{localStorage.setItem(CUSTOM_KEY,JSON.stringify(custom));}catch{/* 同上 */}},[custom]);

 const entries=useMemo(()=>toolEntries.reduce<Record<string,ToolEntry>>((map,item)=>{map[item.id]=item;return map;},{}),[]);
 const customEntries=useMemo<Record<string,ToolEntry>>(()=>custom.reduce<Record<string,ToolEntry>>((map,item)=>{map[item.id]={id:item.id,label:item.label,description:'自定义外链工具，在系统浏览器中打开，不读取本地素材。',keywords:'外链 自定义 链接',area:'extension',kind:'link',status:'link',openIn:'browser'};return map;},{}),[]);
 const all=useMemo(()=>[...Object.values(entries),...Object.values(customEntries)],[entries,customEntries]);
 const statusOf=(entry:ToolEntry):ToolStatus=>{
  if(entry.kind==='link')return'link';
  if(entry.status==='dev')return'dev';
  if(entry.status==='need')return ids.length?'ready':'need';
  return'ready';
 };
 const hrefOf=(id:string)=>links[id]??toolEntries.find(entry=>entry.id===id)?.href??'';
 const openLink=(url:string)=>{
  // 窗口是模态的，toast 会被遮住，所以窗口内的反馈统一走 flash 行。
  if(!/^https?:\/\//i.test(url.trim())){setFlash('只允许 http/https 地址，未打开任何外部程序。');return;}
  if(!native){setFlash('外链需要桌面版，网页预览里不能调用系统浏览器。');return;}
  void invoke('open_external',{url:url.trim()}).then(()=>setFlash(`已在系统浏览器打开：${url.trim()}`)).catch(e=>setFlash(`无法打开链接：${String(e)}`));
 };
 /**
  * 外部程序（目前只有 Audio Dedup）：本程序只负责启动它，不代管扫描与处置。
  * 路径优先级：上次记录（后端）→ 自动定位 → 实在找不到才让你选一次并记住。
  */
 const openApp=(id:string)=>{
  if(!native){setFlash('外部程序需要桌面版，网页预览里不能启动。');return;}
  setFlash('正在定位并启动…');
  void (async()=>{
   try{
    let exe=appInfo?.path??'';
    try{if(!exe)exe=localStorage.getItem(`sound-workbench-app-${id}`)??'';}catch{/* 存储不可用就跳过 */}
    if(!exe){const found=await invoke<{version:string;path:string}>('dedup_locate');exe=found.path;setAppInfo(found);try{localStorage.setItem(`sound-workbench-app-${id}`,exe);}catch{/* 同上 */}}
    await invoke('dedup_launch_gui',{exe});
    setFlash(`已启动 Audio Dedup${exe?`：${displayPath(exe)}`:''}`);
   }catch(e){
    try{
     const picked=await invoke<string|null>('plugin_pick_file',{initial:null,title:'选择 Audio Dedup 程序'});
     if(!picked){setFlash('未选择程序，已取消。');return;}
     try{localStorage.setItem(`sound-workbench-app-${id}`,picked);}catch{/* 同上 */}
     await invoke('dedup_launch_gui',{exe:picked});
     setFlash(`已记住并启动：${displayPath(picked)}`);
    }catch(inner){setFlash(`无法启动：${String(inner)}`);}
   }
  })();
 };
 const activate=(entry:ToolEntry)=>{
  setUsage(old=>({...old,[entry.id]:Date.now()}));
  if(entry.kind==='app'){openApp(entry.id);return;}
  if(entry.area==='account'){onAccount();return;}
  onTool(entry.id as ToolPage);
 };
 const matches=query.trim()?all.filter(entry=>{
  const words=query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  return words.every(word=>`${entry.label} ${entry.description} ${entry.keywords}`.toLowerCase().includes(word));
 }):null;
 const current=tool?all.find(entry=>entry.id===tool)??null:null;

 const card=(entry:ToolEntry)=>{
  const status=statusOf(entry),link=entry.kind==='link',app=entry.kind==='app';
  const host=link?(hrefOf(entry.id).replace(/^https?:\/\//,'').replace(/\/.*$/,'')||'未配置地址'):'';
  return <article key={entry.id} className={`manager-card${link||app?' is-link':''}${current?.id===entry.id?' is-open':''}`}>
   <div className="manager-card-head">
    <span className="manager-card-icon"><ToolGlyph id={entry.id as ToolPage}/></span>
    <b>{entry.label}</b>
    <span className={`manager-status ${app?'link':STATUS_CLASS[status]}`}>{app?'外部程序':STATUS_LABEL[status]}</span>
   </div>
   <p>{entry.description}</p>
   <div className="manager-card-foot">
    <span className="manager-card-meta">{app?(appLocating?'正在定位程序…':(appInfo?('v'+appInfo.version+' · '+displayPath(appInfo.path)):(native?'未找到程序 · 点击后可手动选择一次':'本机独立程序 · 版本与路径在桌面版显示'))):(link?host:`${ids.length?`已选 ${ids.length} 条`:'未选择素材'}${usage[entry.id]?` · ${sinceLabel(usage[entry.id])}`:' · 尚未使用'}`)}</span>
    {app
     ? <><span className="manager-card-sp"/><button className="manager-go" onClick={()=>activate(entry)}>打开 ↗</button></>
     : link
     ? <><span className="manager-card-sp"/><button className="manager-go" onClick={()=>{const url=hrefOf(entry.id);if(url)openLink(url);else{setEditing(entry.id);setDraftLink('');}}}>{hrefOf(entry.id)?'打开 ↗':'配置'}</button></>
     : <><span className="manager-card-sp"/><button className="manager-go" onClick={()=>activate(entry)}>展开 ▸</button></>}
   </div>
  </article>;
 };

 const groupSection=(area:ToolArea,cols:2|3,title:string,tip:string,extra?:React.ReactNode)=><section className="manager-section">
  <div className="manager-section-head"><b>{title}</b><span>{tip}</span><i/><span>{toolsOf(area).length} 项</span></div>
  <div className={`manager-grid g${cols}`}>{toolsOf(area).map(card)}{extra}</div>
 </section>;

 return <dialog ref={dialog} className="workspace-manager" onCancel={onClose} onClose={onClose} aria-labelledby="manager-title"
  onKeyDown={event=>{if((event.ctrlKey||event.metaKey)&&event.key.toLowerCase()==='k'){event.preventDefault();search.current?.focus();search.current?.select();}}}>
  <header className="manager-head">
   <h2 id="manager-title">管理</h2><span className="manager-sub">工具 · 扩展 · 账号</span>
   <label className="manager-search"><span aria-hidden="true">⌕</span><input ref={search} value={query} maxLength={120} aria-label="查找工具" placeholder="查找工具…" onChange={event=>setQuery(event.target.value)}/>{(query?<button aria-label="清除查找" onClick={()=>setQuery('')}>×</button>:<kbd>Ctrl K</kbd>)}</label>
   <button className="manager-close" onClick={onClose} aria-label="关闭管理窗口">✕</button>
  </header>
  <div className="manager-body">
   <nav className="manager-rail" aria-label="工具分组">
    {toolGroups.map(group=><React.Fragment key={group.area}>
     <div className="manager-group">{group.label}</div>
     {toolsOf(group.area).map(entry=><button key={entry.id} className={`manager-rail-item${current?.id===entry.id?' is-on':''}`} aria-current={current?.id===entry.id?'page':undefined} onClick={()=>activate(entry)}>
      <span className="manager-rail-icon"><ToolGlyph id={entry.id as ToolPage}/></span>
      {entry.label}
      {(entry.kind==='link'||entry.kind==='app')&&<span className="manager-rail-ext">{entry.kind==='app'?'⤴':'↗'}</span>}
     </button>)}
    </React.Fragment>)}
    <p className="manager-rail-foot">全部处理在本机完成，离线可用。<br/>外部工具以链接方式接入。</p>
   </nav>
   <div className="manager-content">
    {current?<>
     <div className="manager-detail-head">
      <b>{current.label}</b><span className="manager-count">{GROUP_LABEL[current.area]} · {toolsOf(current.area).length} 项</span>
      <span className="manager-head-sp"/>
      <span className={`manager-ctx${ids.length?'':' is-off'}`}>{ids.length?`已选 ${ids.length} 条`:'未选择素材'}</span>
      <button className="manager-back" onClick={()=>onTool(null)}>返回总览</button>
     </div>
     <div className="manager-detail-body">
      {['ucs','duplicates','convert'].includes(current.id)&&<FoundationTools native={native} ids={ids} library={library} onPick={onPick} onChanged={onChanged} section={current.id as 'ucs'|'duplicates'|'convert'}/>}
      {current.id==='plugins'&&<><PluginPanel/><FoundationTools native={native} ids={ids} library={library} onPick={onPick} onChanged={onChanged} section="plugins"/></>}
      {current.id==='layer'&&<LayerTools/>}
      {current.id==='pt'&&<SmartTools native={native} ids={ids} initiallyOpen/>}
      {current.id==='maintenance'&&<section className="manager-panel">
       <h3>素材库维护</h3>
       <p>检查已登记文件是否仍可访问，保留缺失记录和用户标注，不自动删除任何素材。</p>
       <div className="manager-panel-row"><button disabled={!native||busy} onClick={onCheck}>检查所有已登记素材</button><button onClick={onJobs}>查看导入任务</button></div>
      </section>}
      {current.id==='info'&&<section className="manager-panel">
       <h3>音频信息</h3>
       {selected?<><p className="manager-file">{selected.name}</p><dl className="manager-info">
        <div><dt>格式 / 采样率</dt><dd>{selected.format} / {selected.rate?`${selected.rate} Hz`:'未知'}</dd></div>
        <div><dt>声道 / 时长</dt><dd>{selected.channels==null?'未知':`${selected.channels} 声道`} / {selected.duration==null?'未知':timeLabel(selected.duration)}</dd></div>
        <div><dt>文件大小</dt><dd>{selected.size==null?'—':formatBytes(selected.size)}</dd></div>
        <div><dt>原路径</dt><dd className="manager-path">{displayPath(selected.path)}</dd></div>
       </dl></>:<p>先在素材库选择一条音频，再回到这里查看规格。</p>}
      </section>}
      {current.kind==='link'&&<section className="manager-panel">
       <h3>{current.label}</h3>
       <p>外部链接工具：在系统浏览器中打开，不读取本地素材，也不回写标签或文件名。</p>
       <label className="manager-link-field">工具地址<input value={editing===current.id?draftLink:hrefOf(current.id)} placeholder="https://…" onChange={event=>{setEditing(current.id);setDraftLink(event.target.value);}}/></label>
       <div className="manager-panel-row">
        <button className="primary" disabled={!(editing===current.id?draftLink:hrefOf(current.id)).trim()} onClick={()=>{const url=editing===current.id?draftLink:hrefOf(current.id);openLink(url);}}>在浏览器打开 ↗</button>
        <button disabled={editing!==current.id} onClick={()=>{setLinks(old=>({...old,[current.id]:draftLink.trim()}));setEditing(null);setFlash(`已保存：${draftLink.trim()}`);}}>保存地址</button>
       </div>
       <p className="manager-hint">只允许 http/https；窗口不会内嵌网页，避免第三方脚本进入应用。</p>
       {flash&&<p className="manager-flash" role="status">{flash}</p>}
       <ol className="manager-steps">
        <li><b>1 · 在浏览器里处理</b><span>选中素材后在外部工具生成命名清单，应用不改名、不写标签。</span></li>
        <li><b>2 · 回来核对</b><span>把清单与素材库对照，逐条确认 CatID 与关键词再落盘。</span></li>
        <li><b>3 · 只留入口</b><span>工具地址保存在本机设置里，换机器需要重填。</span></li>
       </ol>
       <p className="manager-hint">没有地址时点「配置」填写；也可以点「返回总览」在扩展区添加新的链接工具。</p>
      </section>}
     </div>
    </>:<div className="manager-body-scroll">
     {query.trim()?<>
      <div className="manager-section-head"><b>查找结果</b><span>共 {matches?.length??0} 项</span><i/></div>
      {matches?.length?<div className="manager-grid g2">{matches.map(card)}</div>:<p className="manager-empty">没有匹配的工具，换个词试试。</p>}
     </>:<>
      {groupSection('library',2,'素材库','对所选素材直接生效')}
      {groupSection('production',3,'制作与连接','需要宿主或导出目录')}
      {groupSection('extension',3,'扩展 · 外链与外部程序','链接与独立程序都从这里进，不占主界面',<>
       {custom.map(item=>card(customEntries[item.id]))}
       <div className="manager-add-link">
        <span>＋</span>添加链接工具
        <input value={draftName} maxLength={24} aria-label="新链接工具名称" placeholder="名称" onChange={event=>setDraftName(event.target.value)}/>
        <input value={draftUrl} maxLength={2048} aria-label="新链接工具地址" placeholder="https://…" onChange={event=>setDraftUrl(event.target.value)}/>
        <button disabled={!draftName.trim()||!/^https?:\/\//i.test(draftUrl.trim())} onClick={()=>{
         const id=`custom-${Date.now()}`;setCustom(old=>[...old,{id,label:draftName.trim(),href:draftUrl.trim()}]);
         setLinks(old=>({...old,[id]:draftUrl.trim()}));setFlash(`已添加：${draftName.trim()} → ${draftUrl.trim()}`);setDraftName('');setDraftUrl('');
        }}>添加</button>
       </div>
      </>)}
      <section className="manager-section">
       <div className="manager-section-head"><b>最近使用</b><span>按最近一次打开排序</span><i/><span>{Object.keys(usage).length} 条</span></div>
       {Object.keys(usage).length?<div className="manager-recent">
        <div className="manager-recent-head"><span>工具</span><span>分组</span><span>当前上下文</span><span>上次打开</span></div>
        {Object.entries(usage).sort((a,b)=>b[1]-a[1]).slice(0,5).map(([id,stamp])=>{const entry=all.find(item=>item.id===id);if(!entry)return null;return <div className="manager-recent-row" key={id}>
         <b><i><ToolGlyph id={entry.id as ToolPage}/></i>{entry.label}</b>
         <span className="manager-recent-meta">{GROUP_LABEL[entry.area]}</span>
         <span className="manager-recent-result">{entry.kind==='link'?(hrefOf(entry.id)||'未配置地址'):ids.length?`已选 ${ids.length} 条 · ${library==='music'?'音乐库':'音效库'}`:'未选择素材'}</span>
         <span className="manager-recent-time">{sinceLabel(stamp)}</span>
        </div>;})}
       </div>:<p className="manager-empty">还没有使用记录；打开任意工具后这里会按时间倒序列出最近 5 项。</p>}
      </section>
     </>}
    </div>}
   </div>
  </div>
  <footer className="manager-foot">
   <span className={flash?'is-flash':undefined}>{flash||'点击卡片在窗口内展开，不跳转页面'}</span><kbd>Esc</kbd><span>关闭</span><kbd>Ctrl K</kbd><span>查找工具</span>
   <span className="manager-foot-sp"/>
   <span>{toolGroups.length} 组 · {toolEntries.filter(entry=>entry.kind==='local').length} 个本机工具 · {toolEntries.filter(entry=>entry.kind==='link').length+custom.length} 个外链 · {toolEntries.filter(entry=>entry.kind==='app').length} 个外部程序</span>
  </footer>
 </dialog>;
}
