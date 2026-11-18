import React,{useEffect,useRef,useState} from 'react';
import {invoke,isTauri} from '@tauri-apps/api/core';
import {displayPath,formatBytes} from '../catalog-client';
import {PLUGIN_META,pluginApi,type PluginId,type PluginResult} from './client';
import './plugin-workspace.css';

/** 0.1.35：可插拔模块的「工作区」窗口，入口在顶栏「音乐库」右侧。
 *  0.1.36 起多一个「Audio Dedup（外部）」条目：按方案 A「外部工具契约」接入——
 *  我们只调它的命令行、读它产出的 groups.json，不做进程内合并，也不提供删除/隔离入口。
 *
 *  窗口里只做能被验证的事：
 *  - AAF/OMF：选文件 → 读文件头（静态探测）→ 取候选；
 *  - PT 叠层：本机只读探测能力与状态；
 *  - PT 批量导出：选输出目录 → 出预览清单（不渲染、不写盘）；
 *  - Audio Dedup：检测程序 → 扫描库 → 读分组汇总。
 *  写入类命令后端恒返回 WRITE_BLOCKED，这里也不放写入按钮。 */
type Flags=Record<PluginId,boolean>;
type WorkItem=PluginId|'dedup';

const DEDUP_LABEL='Audio Dedup（外部）';
const DEDUP_STEPS='先检测去重程序是否支持命令行契约（--cli --version），再选扫描根与索引目录跑扫描；扫描完读它的 groups.json，把重复组汇总到这里。隔离与删除仍只在它自己的界面里人工确认，本程序不提供处置入口。';

const STEPS:Record<PluginId,string>={
 sfx_aaf_import:'选择 AAF／OMF 文件后先读文件头，得到静态探测结果；再取候选素材清单。整个过程只读，不解析完整时间线、不改任何工程。',
 sfx_pt_overlay:'探测本机能给 Pro Tools 提供什么（能力清单）以及当前连接状态；未连接 PT 时如实返回不可用，不假装成功。',
 sfx_pt_bouncer:'选择一个输出目录，生成本次要导出的预览清单（条数、目标、预计体积）；不渲染音频、不写盘。',
};

type DedupProbe={version:string;path:string};
type DedupRun={ok:boolean;code:number|null;mode:string;workdir:string;stdout_tail:string;stderr_tail:string;groups_path:string};
type DedupFile={path:string;size:number;keep:boolean;grade:string};
type DedupGroup={id:string;layer:string;grade:string;count:number;keep:string;reclaimable_bytes:number;files:DedupFile[]};
type DedupSummary={
 schema:number;generated_at:string;root:string;workdir:string;
 l1_groups:number;l2_groups:number;l3_groups:number;edges:number;total_groups:number;
 reclaimable_bytes:number;scanned_files:number;errors:string[];groups:DedupGroup[];truncated:boolean;
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

function baseName(path:string){const parts=path.split(/[\\/]/);return parts[parts.length-1]??path;}

export function PluginWorkspace({open,onClose,flags,native}:{open:boolean;onClose:()=>void;flags:Flags;native:boolean}){
 const dialog=useRef<HTMLDialogElement>(null),[active,setActive]=useState<WorkItem>('sfx_aaf_import');
 const [busy,setBusy]=useState(false),[error,setError]=useState('');
 const [aafPath,setAafPath]=useState(''),[aafParse,setAafParse]=useState<PluginResult|null>(null),[aafCands,setAafCands]=useState<PluginResult|null>(null);
 const [overlay,setOverlay]=useState<{cap:PluginResult|null;status:PluginResult|null}>({cap:null,status:null});
 const [bouncerPath,setBouncerPath]=useState(''),[bouncer,setBouncer]=useState<PluginResult|null>(null);
 const [dedupExe,setDedupExe]=useState(()=>{try{return localStorage.getItem('sound-workbench-dedup-exe')??'';}catch{return '';}});
 const [dedupRoot,setDedupRoot]=useState(''),[dedupIndex,setDedupIndex]=useState('');
 const [probe,setProbe]=useState<DedupProbe|null>(null),[run,setRun]=useState<DedupRun|null>(null),[summary,setSummary]=useState<DedupSummary|null>(null),[dedupMode,setDedupMode]=useState('scan');

 useEffect(()=>{
  const node=dialog.current;if(!node)return;
  if(open&&!node.open)node.showModal();else if(!open&&node.open)node.close();
 },[open]);
 useEffect(()=>{if(open)setError('');},[open]);
 // 首次打开且没记住路径时，自动找一次去重程序；失败就保持手动选择，不打扰用户。
 useEffect(()=>{
  if(!open||!native||dedupExe)return;
  void (async()=>{
   try{const found=await invoke<DedupProbe>('dedup_locate');setDedupExe(found.path);setProbe(found);}
   catch{/* 没找到：留给「自动查找」按钮与手动选择 */}
  })();
 },[open,native]);

 async function run_(work:()=>Promise<void>){if(busy)return;setBusy(true);setError('');try{await work();}catch(e){setError(String(e));}finally{setBusy(false);} }

 const pickAaf=()=>void run_(async()=>{const path=await invoke<string|null>('plugin_pick_file',{initial:aafPath||null,title:'选择 AAF / OMF 文件'});if(path){setAafPath(path);setAafParse(null);setAafCands(null);}});
 const parseAaf=()=>void run_(async()=>{const result=await pluginApi.aafParse({sourcePath:aafPath,readOnly:true});setAafParse(result);setAafCands(null);});
 const loadCands=()=>void run_(async()=>{const parseId=(aafParse?.data as {parse_id?:string}|null)?.parse_id;const result=await pluginApi.aafCandidates({parseId,reviewed:false});setAafCands(result);});
 const probeOverlay=()=>void run_(async()=>{const [cap,status]=await Promise.all([pluginApi.overlayCapabilities(),pluginApi.overlayStatus()]);setOverlay({cap,status});});
 const pickBouncer=()=>void run_(async()=>{const path=await invoke<string|null>('catalog_pick_folder',{initial:bouncerPath||null});if(path)setBouncerPath(path);});
 const previewBouncer=()=>void run_(async()=>{setBouncer(await pluginApi.bouncerPreview({outputDir:bouncerPath}));});

 const pickDedupExe=()=>void run_(async()=>{const path=await invoke<string|null>('plugin_pick_file',{initial:dedupExe||null,title:'选择 Audio Dedup 程序'});if(path){setDedupExe(path);setProbe(null);try{localStorage.setItem('sound-workbench-dedup-exe',path);}catch{/* 存储不可用不影响本次使用 */}}});
 const pickDedupRoot=()=>void run_(async()=>{const path=await invoke<string|null>('catalog_pick_folder',{initial:dedupRoot||null});if(path)setDedupRoot(path);});
 const pickDedupIndex=()=>void run_(async()=>{const path=await invoke<string|null>('catalog_pick_folder',{initial:dedupIndex||null});if(path)setDedupIndex(path);});
 const dedupProbe=()=>void run_(async()=>{
  const found=await invoke<DedupProbe>('dedup_probe',{exe:dedupExe});
  setProbe(found);
  try{await invoke('dedup_remember_exe',{path:found.path});}catch{/* 记不住路径不影响本次使用 */}
 });
 const dedupRunIt=(mode:string)=>void run_(async()=>{
  setDedupMode(mode);
  const out=await invoke<DedupRun>('dedup_run',{exe:dedupExe,root:dedupRoot||'',workdir:dedupIndex||null,mode});
  setRun(out);
  if(dedupIndex)setDedupIndex(out.workdir);
  try{setSummary(await invoke<DedupSummary>('dedup_groups',{workdir:out.workdir}));}catch{/* 扫描失败时可能没有 groups.json，错误已在 run 输出里 */}
 });
 const readDedup=()=>void run_(async()=>{setSummary(await invoke<DedupSummary>('dedup_groups',{workdir:dedupIndex||null}));});
 // 打开「重复组工作台」独立窗口：不设 parent，可拖到主窗口之外；同组再次点击只聚焦并切换选中项。
 // group 传空时由工作台自己选第一组，这样「还没读过分组」也能一键开窗。
 const openWorkbench=(groupId:string)=>void run_(async()=>{
  let current=summary;
  if(!current)current=await invoke<DedupSummary>('dedup_groups',{workdir:dedupIndex||null});
  if(!current)throw new Error('还没有分组结果：先运行一次扫描。');
  setSummary(current);
  await invoke('dedup_open_workbench',{workdir:current.workdir||dedupIndex||'',group:groupId||current.groups[0]?.id||'',exe:dedupExe||null});
 });
 // 自动定位去重程序：先读上次记录，再按有界目录遍历；不再要求用户手找 exe。
 const locateDedup=()=>void run_(async()=>{
  const found=await invoke<DedupProbe>('dedup_locate');
  setDedupExe(found.path);setProbe(found);
  try{localStorage.setItem('sound-workbench-dedup-exe',found.path);}catch{/* 存储不可用不影响本次使用 */}
 });

 const meta=active==='dedup'?null:PLUGIN_META.find(item=>item.id===active)!;
 const enabled=active==='dedup'?true:flags[active as PluginId];
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
    <button aria-current={active==='dedup'?'page':undefined} className={active==='dedup'?'is-active':''} onClick={()=>{setActive('dedup');setError('');}}>
     <span className="pw-rail-name">{DEDUP_LABEL}</span>
     <span className="pw-rail-state ext">外部工具 · 命令行契约</span>
    </button>
    <p className="pw-rail-note">前三个开关在「管理 › 可插拔模块」里，两层闸门：Cargo feature ＋ 设置开关。外部工具不需要开关，只认程序路径。</p>
   </nav>
   <section className="pw-work" aria-label={meta?`${meta.label} 工作区`:`${DEDUP_LABEL} 工作区`}>
    {meta?<div className="pw-title"><h2>{meta.label}</h2><code>{meta.id}</code><span className="pw-deps">{meta.dependency}</span></div>:<div className="pw-title"><h2>{DEDUP_LABEL}</h2><code>audio-dedup --cli</code><span className="pw-deps">独立程序 · 只读调用 · 不合并进本程序</span></div>}
    {!enabled&&<p className="pw-warn" role="status">此模块在设置里未启用：命令会被后端拒绝执行。请先到「管理 › 可插拔模块」打开开关。</p>}
    <p className="pw-steps">{meta?STEPS[active as PluginId]:DEDUP_STEPS}</p>

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

    {active==='dedup'&&<>
     <div className="pw-row"><button className="pw-primary" disabled={busy||!native} onClick={pickDedupExe}>选择去重程序…</button><button disabled={busy||!native} onClick={locateDedup}>自动查找</button><button disabled={busy||!native||!dedupExe} onClick={dedupProbe}>检测</button><span className="pw-path" title={dedupExe}>{dedupExe?displayPath(dedupExe):'未选择 audio-dedup-gui.exe（点「自动查找」可省去手动找路径）'}</span></div>
     {probe&&<div className="pw-row"><span className="pw-hint">命令行契约可用 · 版本 {probe.version}</span></div>}
     <div className="pw-row"><button disabled={busy||!native} onClick={pickDedupRoot}>选择扫描根…</button><span className="pw-path" title={dedupRoot}>{dedupRoot?displayPath(dedupRoot):'未选择（扫描必填）'}</span></div>
     <div className="pw-row"><button disabled={busy||!native} onClick={pickDedupIndex}>选择索引目录…</button><span className="pw-path" title={dedupIndex}>{dedupIndex?displayPath(dedupIndex):'留空则用默认 %APPDATA%\\sfx-tools\\dedup'}</span></div>
     <div className="pw-row">
      <button disabled={busy||!native||!dedupExe||!dedupRoot} onClick={()=>dedupRunIt('scan')}>扫描（L1+L2）</button>
      <button disabled={busy||!native||!dedupExe||!dedupRoot} onClick={()=>dedupRunIt('l1')}>只做 L1（快）</button>
      <button disabled={busy||!native||!dedupExe} onClick={()=>dedupRunIt('l3')}>L3 感知疑似</button>
      <button disabled={busy||!native||!dedupExe} onClick={()=>dedupRunIt('report')}>重写报告</button>
      <button disabled={busy||!native} onClick={readDedup}>读取分组</button>
      <button className="pw-primary" disabled={busy||!native} onClick={()=>openWorkbench('')}>打开工作台…</button>
      <span className="pw-hint">{busy?`正在跑 ${dedupMode}…（大库可能要几分钟）`:'扫描过程中界面会等待，可切到别的模块做别的事'}</span>
     </div>
     {summary&&<DedupSummaryCard summary={summary} onOpen={openWorkbench}/>}
     {run&&!run.ok&&<article className="pw-result is-fail" aria-label="去重运行输出">
      <header><strong>{run.mode} 退出码 {run.code??'?'}</strong><Evidence value="local_probe"/></header>
      {(run.stderr_tail||run.stdout_tail)&&<pre>{run.stderr_tail||run.stdout_tail}</pre>}
     </article>}
     {run&&run.ok&&<article className="pw-result" aria-label="去重运行输出">
      <header><strong>{run.mode} 完成</strong><code className="pw-code">exit 0</code><Evidence value="local_probe"/></header>
      {run.stdout_tail&&<pre>{run.stdout_tail.slice(-1200)}</pre>}
     </article>}
    </>}

    {error&&<p className="pw-error" role="alert">{error}</p>}
    {meta
     ?<footer className="pw-foot">本窗口不提供任何写入入口：<code>{meta.write}</code> 在后端恒返回 WRITE_BLOCKED，未连接真实 Pro Tools、未经过真实工程验证前不应放开。</footer>
     :<footer className="pw-foot">去重结果只读：本窗口不提供隔离、删除、移动或改写素材的任何入口；需要处置时请回到 Audio Dedup 自己的界面走人工确认流程。扫描根全程只读。</footer>}
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

function DedupSummaryCard({summary,onOpen}:{summary:DedupSummary;onOpen:(id:string)=>void}){
 const shown=summary.groups.slice(0,40);
 return <article className="pw-result dedup-summary" aria-label="去重分组汇总">
  <header><strong>分组汇总</strong><code className="pw-code">{summary.generated_at||'—'}</code><Evidence value="local_probe"/></header>
  <div className="dedup-chips">
   <span>分组 <b>{summary.total_groups}</b></span>
   <span>L1 <b>{summary.l1_groups}</b></span>
   <span>L2 <b>{summary.l2_groups}</b></span>
   <span>L3 <b>{summary.l3_groups}</b></span>
   <span>可释放 <b>{formatBytes(summary.reclaimable_bytes)}</b></span>
   <span>已扫文件 <b>{summary.scanned_files}</b></span>
   <span>错误 <b>{summary.errors.length}</b></span>
  </div>
  {shown.length?<table className="dedup-table">
   <thead><tr><th>组</th><th>层级</th><th>等级</th><th>文件</th><th>可释放</th><th>建议保留</th></tr></thead>
   <tbody>{shown.map(group=><tr key={group.id} title={`${group.keep}\n点击打开重复组工作台（独立窗口）`} className="is-clickable" onClick={()=>onOpen(group.id)}>
    <td>{group.id}</td><td>{group.layer}</td><td>{group.grade}</td><td>{group.count}</td>
    <td>{formatBytes(group.reclaimable_bytes)}</td><td>{baseName(group.keep)}</td>
   </tr>)}</tbody>
  </table>:<p className="pw-hint">还没有重复组。</p>}
  <p className="pw-hint">点任意一行打开「重复组工作台」独立窗口：分组浏览、文件表、最多 5 轨波形与试听，只读。</p>
  {(summary.truncated||summary.groups.length>40)&&<p className="pw-hint">只显示前 40 组{summary.groups.length>40?`（共 ${summary.groups.length} 组）`:''}；完整清单请看 groups.json 或去重工具界面。</p>}
  {summary.errors.length>0&&<ul>{summary.errors.slice(0,5).map((item,i)=><li key={i}>{item}</li>)}</ul>}
  <p className="pw-hint">根目录 {displayPath(summary.root)} · 索引 {displayPath(summary.workdir)}</p>
 </article>;
}
