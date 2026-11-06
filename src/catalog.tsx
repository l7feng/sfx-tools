import React,{useCallback,useEffect,useRef,useState} from 'react';
import {createRoot} from 'react-dom/client';
import {invoke,isTauri} from '@tauri-apps/api/core';
import {getCurrentWebview} from '@tauri-apps/api/webview';
import {formatBytes,jobLabels,resumable,displayPath} from './catalog-client';
import type {CatalogAsset,CatalogPage,ImportJob,FolderNode} from './catalog-client';
import {FolderTree} from './folder-tree';
import {NativePlayer} from './native-player';
import {AssetAnnotations} from './asset-annotations';
import {timeLabel} from './demo-data';
import {useWorkbenchLayout,ResizeHandle} from './layout';
import {safeLayout,selectIds,defaultLayout,visibleColumns} from './workbench-state';
import {ColumnSettings} from './column-settings';
import {useAppVersion} from './app-version';
import {CatalogTools} from './catalog-tools';
import {MiniWaveform} from './waveform';
import {Settings} from './settings';
import {SidebarQuickPanel,type SidebarPanel,type SidebarPanelKind} from './sidebar-quick';
import {CatalogContext,type ContextTarget,type LibraryOverview} from './catalog-context';
import {searchFolder,type SearchScope} from './search-scope';
import {addToLayer} from './layer-tools';
import {ToolsPage} from './tools-page';
import {toolArea,type ToolPage} from './tool-navigation';
import {MusicCategories} from './music-categories';
import {usePreferences,type WorkspacePage} from './preferences';
import './workbench.css';
import './catalog.css';
import './workspace-layout.css';
import './smart-tools.css';
import './workspace-shell.css';
import './context-menu.css';

const PAGE_SIZE=100;
const pageNames={sound:'音效库',music:'音乐库',tools:'音频工具'};
type Library='sound'|'music';
type View={offset:number;folder:string|null;query:string;format:string;sort:string;favorites:boolean;searchScope:SearchScope;tag:string|null;historyOnly:boolean;playlistId:string|null};
const emptyView=():View=>({offset:0,folder:null,query:'',format:'全部格式',sort:'name',favorites:false,searchScope:'global',tag:null,historyOnly:false,playlistId:null});
function Catalog(){
 const appVersion=useAppVersion();
 const [pinsOpen,setPinsOpen]=useState(()=>{try{return localStorage.getItem('sound-workbench-pins-open-v1')!=='0';}catch{return true;}});
 useEffect(()=>{try{localStorage.setItem('sound-workbench-pins-open-v1',pinsOpen?'1':'0');}catch{/* 存储不可用不影响展开 */}},[pinsOpen]);
 const native=isTauri(),{preferences,setPreferences}=usePreferences();
 const {layout,setLayout,style}=useWorkbenchLayout(preferences.rememberLayout);
 const [workspacePage,setWorkspacePage]=useState<WorkspacePage>(preferences.startPage),[library,setLibrary]=useState<Library>(preferences.startPage==='music'?'music':'sound');
 const shownColumns=visibleColumns(layout);
 const [columnMenu,setColumnMenu]=useState<{x:number;y:number}|null>(null);
 const [sidebarPanel,setSidebarPanel]=useState<SidebarPanel|null>(null);
 const [context,setContext]=useState<ContextTarget|null>(null),[overview,setOverview]=useState<LibraryOverview>({tags:[],pins:[],playlists:[],history_count:0});
 const [searchScope,setSearchScope]=useState<SearchScope>('global'),[tag,setTag]=useState<string|null>(null),[historyOnly,setHistoryOnly]=useState(false),[playlistId,setPlaylistId]=useState<string|null>(null);
 const [settingsOpen,setSettingsOpen]=useState(false),[toolQuery,setToolQuery]=useState(''),[activeTool,setActiveTool]=useState<ToolPage|null>(null),[libraryToolsOpen,setLibraryToolsOpen]=useState(false);
 const [page,setPage]=useState<CatalogPage>({items:[],total:0}),[offset,setOffset]=useState(0),[loadedOffset,setLoadedOffset]=useState(0),[jobs,setJobs]=useState<ImportJob[]>([]);
 const [query,setQuery]=useState(''),[format,setFormat]=useState('全部格式'),[selected,setSelected]=useState<CatalogAsset|null>(null),[selection,setSelection]=useState<string[]>([]);
 const [debouncedQuery,setDebouncedQuery]=useState(''),[sort,setSort]=useState('name'),[favoritesOnly,setFavoritesOnly]=useState(false),[loadedKey,setLoadedKey]=useState('');
 const [busy,setBusy]=useState(false),[error,setError]=useState(''),[status,setStatus]=useState(native?'本地素材 · 原路径引用':'真实素材需要桌面版');
 const [root,setRoot]=useState(''),[confirmedRoot,setConfirmedRoot]=useState(''),[open,setOpen]=useState(false),[showJobs,setShowJobs]=useState(false),[trackedId,setTrackedId]=useState(''),[importLibrary,setImportLibrary]=useState<Library>(library);
 const [folders,setFolders]=useState<FolderNode[]>([]),[folder,setFolder]=useState<string|null>(null),[history,setHistory]=useState<string[]>([]),[dragging,setDragging]=useState(false);
 const views=useRef<Record<Library,View>>({sound:emptyView(),music:emptyView()}),anchor=useRef<string|null>(null),list=useRef<HTMLDivElement>(null);
 const treeStamp=useRef(''),dropHandler=useRef<(paths:string[])=>void>(()=>{}),reportedProblems=useRef(new Set<string>());
 const lock=useRef(false),dialog=useRef<HTMLDialogElement>(null),search=useRef<HTMLInputElement>(null),generation=useRef(0),alive=useRef(true);
 const effectiveFolder=searchFolder(folder,debouncedQuery,searchScope);
 const scopeKey=JSON.stringify([library,effectiveFolder,offset,debouncedQuery,format,sort,favoritesOnly,tag,historyOnly,playlistId]);
 const scopeRef=useRef(scopeKey);scopeRef.current=scopeKey;
 const pageLoading=scopeKey!==loadedKey,rows=pageLoading?[]:page.items,selectedIds=selection;
 const running=jobs.some(job=>['running','queued','cancelling'].includes(job.status));
 const currentJob=jobs.find(job=>['running','queued','cancelling'].includes(job.status))??jobs.find(job=>job.id===trackedId);
 const liveStatus=currentJob&&['running','queued','cancelling'].includes(currentJob.status)?`${jobLabels[currentJob.status]} · 已处理 ${currentJob.processed} · 新增 ${currentJob.added} · 失败 ${currentJob.failed}`:status;
 const totalAssets=folders.filter(node=>!node.parent).reduce((sum,node)=>sum+node.count,0);
 const folderInfo=folders.find(node=>node.path===folder);
 const resize=(key:'sidebar'|'inspector'|'wave',value:number)=>setLayout(old=>safeLayout({...old,[key]:value}));
 const toggle=(key:'topCollapsed'|'leftCollapsed'|'bottomCollapsed'|'details')=>setLayout(old=>({...old,[key]:!old[key]}));
 useEffect(()=>{const timer=setTimeout(()=>{setDebouncedQuery(query);setOffset(0);},250);return()=>clearTimeout(timer);},[query]);
 useEffect(()=>{setSelection([]);anchor.current=null;},[scopeKey]);
 const switchPage=(next:WorkspacePage)=>{
  if(next===workspacePage)return;setSidebarPanel(null);setColumnMenu(null);
  views.current[library]={offset,folder,query,format,sort,favorites:favoritesOnly,searchScope,tag,historyOnly,playlistId};setWorkspacePage(next);
  if(next!=='tools'){
   const view=views.current[next];setLibrary(next);setFolder(view.folder);setQuery(view.query);setDebouncedQuery(view.query);setFormat(view.format);setSort(view.sort);setFavoritesOnly(view.favorites);setSearchScope(view.searchScope);setTag(view.tag);setHistoryOnly(view.historyOnly);setPlaylistId(view.playlistId);setOffset(view.offset);if(next!==library){setSelected(null);setSelection([]);}treeStamp.current='';
  }
 };
 const openTool=(tool:ToolPage|null)=>{setToolQuery('');setActiveTool(tool);if(tool&&toolArea(tool)==='library'){if(workspacePage==='tools')switchPage(library);setLibraryToolsOpen(true);}else{setLibraryToolsOpen(false);switchPage('tools');}};
 const choose=(asset:CatalogAsset,event?:{ctrlKey:boolean;metaKey:boolean;shiftKey:boolean})=>{
  setSelection(old=>selectIds(rows.map(item=>item.id),old,asset.id,anchor.current,!!event&&(event.ctrlKey||event.metaKey),!!event?.shiftKey));
  if(!event?.shiftKey)anchor.current=asset.id;setSelected(asset);
 };
 const chooseExternal=(item:CatalogAsset)=>{setSelected(item);setSelection([item.id]);};
 const refresh=useCallback(async()=>{
  if(scopeRef.current!==scopeKey)return;const token=++generation.current;
  const [next,nextJobs,nextOverview]=await Promise.all([invoke<CatalogPage>('catalog_list',{offset,limit:PAGE_SIZE,folder:effectiveFolder,query:debouncedQuery,format:format==='全部格式'?null:format,sort,favoritesOnly,library,tag,historyOnly,playlistId}),invoke<ImportJob[]>('catalog_jobs'),invoke<LibraryOverview>('library_overview',{library})]);
  if(!alive.current||token!==generation.current||scopeRef.current!==scopeKey)return;
  if(offset>0&&next.total<=offset){setOffset(Math.max(0,Math.floor(Math.max(0,next.total-1)/PAGE_SIZE)*PAGE_SIZE));return;}
  setPage(next);setLoadedOffset(offset);setLoadedKey(scopeKey);setJobs(nextJobs);setOverview(nextOverview);
  const stamp=JSON.stringify([library,nextJobs.map(job=>[job.id,job.status,job.processed,job.updated,job.added])]);
  if(treeStamp.current!==stamp){
   const [tree,paths]=await Promise.all([invoke<FolderNode[]>('catalog_folders',{library}),invoke<string[]>('catalog_history')]);
   if(!alive.current||token!==generation.current||scopeRef.current!==scopeKey)return;
   setFolders(tree);setHistory(paths);treeStamp.current=stamp;
  }
  for(const job of nextJobs){const key=`${job.id}:${job.status}`;if(['failed','interrupted','completed_with_errors'].includes(job.status)&&!reportedProblems.current.has(key)){reportedProblems.current.add(key);setShowJobs(true);}}
  setSelected(old=>old?next.items.find(item=>item.id===old.id)??old:null);
 },[offset,effectiveFolder,debouncedQuery,format,sort,favoritesOnly,library,tag,historyOnly,playlistId,scopeKey]);
 useEffect(()=>{alive.current=true;return()=>{alive.current=false;generation.current++;};},[]);
 useEffect(()=>{
  if(!native)return;let stopped=false,timer:number;
  const poll=async()=>{try{await refresh();}catch(e){if(!stopped)setError(String(e));}finally{if(!stopped)timer=window.setTimeout(poll,1000);}};
  void poll();return()=>{stopped=true;generation.current++;clearTimeout(timer);};
 },[native,refresh]);
 useEffect(()=>{
  if(!native)return;let disposed=false,stop:(()=>void)|undefined;
  void getCurrentWebview().onDragDropEvent(event=>{
   if(disposed)return;if(event.payload.type==='enter'||event.payload.type==='over')setDragging(true);
   else{setDragging(false);if(event.payload.type==='drop')dropHandler.current(event.payload.paths);}
  }).then(unlisten=>{if(disposed)unlisten();else stop=unlisten;}).catch(e=>{if(!disposed)setError(`文件夹拖入不可用：${String(e)}`);});
  return()=>{disposed=true;stop?.();};
 },[native]);
 useEffect(()=>{const element=dialog.current;if(open&&!element?.open)element?.showModal();else if(!open&&element?.open)element.close();},[open]);
 useEffect(()=>{const handler=(event:KeyboardEvent)=>{if(!open&&!settingsOpen&&(event.ctrlKey||event.metaKey)&&event.key.toLowerCase()==='k'){event.preventDefault();setSidebarPanel(null);setColumnMenu(null);setLayout(old=>({...old,topCollapsed:false}));requestAnimationFrame(()=>search.current?.focus());}};window.addEventListener('keydown',handler);return()=>window.removeEventListener('keydown',handler);},[open,settingsOpen,setLayout]);
 async function action(work:()=>Promise<void>){if(lock.current)return;lock.current=true;setBusy(true);setError('');try{await work();}catch(e){if(alive.current)setError(String(e));}finally{lock.current=false;if(alive.current)setBusy(false);}}
 async function validatePath(path:string){setRoot(displayPath(path));setConfirmedRoot('');const checked=await invoke<string>('catalog_preview',{root:path.trim().replace(/^"|"$/g,'')});setConfirmedRoot(checked);setHistory(await invoke<string[]>('catalog_history'));}
 const openImport=()=>{setImportLibrary(library);setRoot(displayPath(history[0]??''));setConfirmedRoot('');setError('');setOpen(true);};
 dropHandler.current=paths=>{if(!open&&document.querySelector('dialog[open]')){setStatus('请先完成或关闭当前对话框，再拖入文件夹');return;}if(lock.current){setStatus('当前操作未结束，请稍后再次拖入');return;}if(paths.length!==1){setError('请一次拖入一个素材文件夹。');return;}setImportLibrary(library);setOpen(true);void action(()=>validatePath(paths[0]));};
 const clearViews=()=>{setFavoritesOnly(false);setTag(null);setHistoryOnly(false);setPlaylistId(null);setOffset(0);setSelected(null);};
 const selectFolder=(path:string|null)=>{setFolder(path);setSearchScope('global');clearViews();};
 const showCollection=(kind:'favorites'|'tag'|'history'|'playlist',value:string|null=null)=>{if(workspacePage==='tools')switchPage(library);clearViews();setFolder(null);setQuery('');setDebouncedQuery('');if(kind==='favorites')setFavoritesOnly(true);else if(kind==='tag')setTag(value);else if(kind==='history')setHistoryOnly(true);else setPlaylistId(value);};
 const contextChanged=(clearSelection=false)=>{treeStamp.current='';generation.current++;if(clearSelection){setSelected(null);setSelection([]);setFolder(null);}void refresh().catch(e=>setError(String(e)));};
 const contextAssets=(asset:CatalogAsset,event:React.MouseEvent)=>{setColumnMenu(null);event.preventDefault();event.stopPropagation();dragOrigin.current=null;const assets=selection.includes(asset.id)?rows.filter(item=>selection.includes(item.id)):[asset];if(!selection.includes(asset.id))choose(asset);setContext({x:event.clientX,y:event.clientY,assets});};
 const start=()=>action(async()=>{const id=await invoke<string>('catalog_start',{root:confirmedRoot,library:importLibrary});setTrackedId(id);setOpen(false);setShowJobs(true);setStatus(`正在添加至${pageNames[importLibrary]}`);await refresh();});
 const checkMissing=()=>action(async()=>{const report=await invoke<{checked:number;available:number;missing:number;unavailable:number;changed:number}>('catalog_check_availability',{folder:workspacePage==='tools'?null:folder});setStatus(`检查 ${report.checked} 条：可用 ${report.available}，缺失 ${report.missing}，不可访问 ${report.unavailable}，变化 ${report.changed}`);treeStamp.current='';await refresh();});
 const assignOther=()=>action(async()=>{const target:Library=library==='sound'?'music':'sound';const count=await invoke<number>('catalog_assign_library',{ids:selectedIds,library:target});setStatus(`已添加 ${count} 条引用至${pageNames[target]}，源文件保持原位`);treeStamp.current='';await refresh();});
 const keyNavigate=(event:React.KeyboardEvent<HTMLDivElement>)=>{
  if((event.target as HTMLElement).closest('input,textarea,select,[role=separator],.mini-waveform'))return;
  if((event.key==='ContextMenu'||event.shiftKey&&event.key==='F10')&&selected){event.preventDefault();const rect=document.getElementById(`asset-${selected.id}`)?.getBoundingClientRect();setContext({x:rect?.left??100,y:rect?.top??150,assets:selection.includes(selected.id)?rows.filter(item=>selection.includes(item.id)):[selected]});return;}
  if((event.ctrlKey||event.metaKey)&&event.key.toLowerCase()==='a'){event.preventDefault();setSelection(rows.map(item=>item.id));return;}
  if(event.key==='Escape'){setSelection([]);return;}
  if((event.ctrlKey||event.metaKey)&&event.shiftKey&&event.key.toLowerCase()==='c'){event.preventDefault();void action(async()=>{const count=await invoke<number>('catalog_copy_paths',{ids:selectedIds});setStatus(`已复制 ${count} 条完整路径`);});return;}
  if(event.altKey&&event.key==='Enter'&&selected){event.preventDefault();void action(async()=>{await invoke('catalog_reveal',{id:selected.id});});return;}
  if(['ArrowUp','ArrowDown','Home','End'].includes(event.key)&&rows.length){event.preventDefault();const current=rows.findIndex(item=>item.id===selected?.id);const next=event.key==='Home'?0:event.key==='End'?rows.length-1:Math.max(0,Math.min(rows.length-1,current+(event.key==='ArrowDown'?1:-1)));choose(rows[next],event);list.current?.focus();requestAnimationFrame(()=>document.getElementById(`asset-${rows[next].id}`)?.scrollIntoView({block:'nearest'}));}
 };
 const dragOutLock=useRef(false),dragOrigin=useRef<{id:string;x:number;y:number;pointer:number}|null>(null);
 const dragMove=(event:React.PointerEvent<HTMLTableRowElement>,asset:CatalogAsset)=>{
  const origin=dragOrigin.current;if(!(event.buttons&1)){dragOrigin.current=null;return;}
  if(!origin||origin.id!==asset.id||origin.pointer!==event.pointerId||Math.hypot(event.clientX-origin.x,event.clientY-origin.y)<6)return;
  dragOrigin.current=null;event.preventDefault();if(!native||dragOutLock.current)return;dragOutLock.current=true;setSelected(asset);setError('');
  const ids=selection.includes(asset.id)?selection:[asset.id];void invoke<string>('catalog_drag_many',{ids}).then(message=>{if(alive.current)setStatus(message);}).catch(e=>{if(alive.current)setError(String(e));}).finally(()=>{dragOutLock.current=false;});
 };
 return <div className={`app-shell catalog-shell workspace-v2 density-${preferences.density} wave-${preferences.waveColor}`} style={style}>
  {dragging&&<div className="folder-drop-overlay"><strong>松开以检查素材文件夹</strong><span>添加至{pageNames[library]} · 确认后才导入</span></div>}
  <header className={`workspace-header ${layout.topCollapsed?'is-collapsed':''}`}>
   <div className="workspace-navigation"><div className="compact-brand"><span>∿</span><strong>SFX TOOLS</strong><small className="app-version" title="当前构建版本，与 EXE 文件属性一致">v{appVersion||'—'}</small></div><nav aria-label="工作台页面">{(['sound','music','tools'] as WorkspacePage[]).map(item=><button key={item} aria-current={workspacePage===item?'page':undefined} onClick={()=>item==='tools'?openTool(null):switchPage(item)}>{pageNames[item]}</button>)}</nav><div className="header-actions"><button onClick={()=>openTool('account')}>账号</button><button onClick={()=>openTool('acoustic')}>检索 / 管理</button><button onClick={()=>setSettingsOpen(true)} title="设置" aria-label="打开设置">⚙<span>设置</span></button><button aria-label={layout.topCollapsed?'展开顶栏':'折叠顶栏'} aria-expanded={!layout.topCollapsed} onClick={()=>toggle('topCollapsed')}>{layout.topCollapsed?'⌄':'⌃'}</button></div></div>
   {!layout.topCollapsed&&<div className="workspace-commandbar"><button className="add-source" disabled={!native||busy||running} onClick={openImport}>＋ 添加{pageNames[library]==='音乐库'?'音乐':'音效'}</button><label className="source-location"><span>素材位置</span><select disabled={workspacePage==='tools'} aria-label="素材存放位置（原路径筛选）" value={folder??''} onChange={event=>selectFolder(event.target.value||null)}><option value="">全部{pageNames[library]}位置</option>{folders.map(node=><option key={node.path} value={node.path}>{displayPath(node.path)}</option>)}</select></label><div className="top-search"><button className="search-scope" hidden={workspacePage==='tools'} aria-label="切换全局或当前文件夹搜索" aria-pressed={searchScope==='folder'} disabled={!folder} title={folder?'点击切换全局/当前文件夹':'未选择文件夹，搜索当前库全部位置'} onClick={()=>{setSearchScope(value=>value==='global'?'folder':'global');setOffset(0);}}>{searchScope==='folder'&&folder?'当前文件夹':'全局'}</button><span aria-hidden="true">⌕</span><input ref={search} aria-label={workspacePage==='tools'?'查找音频工具':'搜索素材名称、路径和标签'} placeholder={workspacePage==='tools'?'查找工具…':`搜索${pageNames[library]}名称、路径、标签…`} value={workspacePage==='tools'?toolQuery:query} maxLength={1024} onChange={event=>workspacePage==='tools'?setToolQuery(event.target.value):setQuery(event.target.value)}/>{(workspacePage==='tools'?toolQuery:query)?<button aria-label="清除搜索" onClick={()=>workspacePage==='tools'?setToolQuery(''):setQuery('')}>×</button>:<kbd>Ctrl K</kbd>}</div></div>}
  </header>
  {error&&<div className="catalog-error shell-error" role="alert">{error}<button onClick={()=>setError('')} aria-label="关闭错误提示">×</button></div>}
  <div className="workspace">
   {layout.leftCollapsed?<aside className="panel-rail left-rail"><button onClick={()=>toggle('leftCollapsed')} aria-label="展开左侧素材栏">›</button><span>素材库</span></aside>:<><aside className="sidebar"><div className="panel-heading"><span>{pageNames[library]}</span><button onClick={()=>toggle('leftCollapsed')} aria-label="折叠左侧素材栏">‹</button></div><nav className="sidebar-shortcuts" aria-label="素材导航"><button className={!folder&&!favoritesOnly&&!tag&&!historyOnly&&!playlistId?'active':''} title={`全部素材 · ${totalAssets}条`} onClick={()=>{setSidebarPanel(null);if(workspacePage==='tools')switchPage(library);selectFolder(null);setQuery('');setFormat('全部格式');}}>全部素材</button><button className={favoritesOnly?'active':''} onClick={()=>{setSidebarPanel(null);showCollection('favorites');}}>收藏</button><button className={historyOnly?'active':''} title={`试听历史 · ${overview.history_count}条`} onClick={()=>{setSidebarPanel(null);showCollection('history');}}>历史</button>{(['tags','playlists'] as SidebarPanelKind[]).map(kind=><button key={kind} className={kind==='tags'&&tag||kind==='playlists'&&playlistId?'active':''} aria-expanded={sidebarPanel?.kind===kind} aria-haspopup="dialog" title={`${({tags:'标签',playlists:'播放列表'})[kind]} · ${kind==='tags'?overview.tags.length:overview.playlists.length}项`} onClick={event=>{const rect=event.currentTarget.getBoundingClientRect();setSidebarPanel(old=>old?.kind===kind?null:{kind,x:rect.right+6,y:rect.top});}}>{({tags:'标签',playlists:'播放列表'})[kind]}</button>)}<button title={`导入任务 · ${jobs.length}项`} aria-expanded={showJobs} onClick={()=>{setSidebarPanel(null);setShowJobs(value=>!value);}}>导入任务</button></nav><div className="sidebar-label tree-label">原路径目录树</div><FolderTree nodes={folders} selected={folder} onContext={(path,event)=>setContext({x:event.clientX,y:event.clientY,folder:path})} onSelect={path=>{if(workspacePage==='tools')switchPage(library);selectFolder(path);}}/><div className={`sidebar-pins${pinsOpen?' is-open':''}`}><button className="pins-toggle" aria-expanded={pinsOpen} aria-controls="sidebar-pin-list" onClick={()=>setPinsOpen(value=>!value)} title={pinsOpen?'收起常用路径':'展开常用路径'}><span className="pins-toggle-label">常用路径</span><b>{overview.pins.length}</b><span className="pins-toggle-caret" aria-hidden="true">{pinsOpen?'⌄':'⌃'}</span></button>{pinsOpen&&<div className="pins-list" id="sidebar-pin-list" role="list">{overview.pins.map(item=>{const name=displayPath(item.path).split(/[\\/]/).filter(Boolean).at(-1)??item.path;return <div key={item.path} role="listitem" className={`pins-row${folder===item.path?' selected':''}`} onContextMenu={event=>{event.preventDefault();event.stopPropagation();setContext({x:event.clientX,y:event.clientY,folder:item.path});}}><span className="pin-dot" data-color={item.color??'none'} aria-hidden="true"/><button className="pins-select" title={displayPath(item.path)} aria-current={folder===item.path?'location':undefined} onClick={()=>{setSidebarPanel(null);if(workspacePage==='tools')switchPage(library);selectFolder(item.path);}}>{name}</button></div>;})}{!overview.pins.length&&<p className="pins-empty">右击目录树中的文件夹，设为常用后可标颜色。</p>}</div>}</div><p className="sidebar-footnote">拖入文件夹添加 · 原路径引用</p></aside><ResizeHandle label="侧栏宽度" value={layout.sidebar} onChange={value=>resize('sidebar',value)}/></>}
   <main className={`browser ${workspacePage==='tools'?'tools-browser':''}`}>
    {workspacePage==='tools'?<ToolsPage native={native} ids={selectedIds} selected={selected} library={library} query={toolQuery} active={activeTool&&toolArea(activeTool)==='production'?activeTool:null} area="production" onNavigate={setActiveTool} onClearQuery={()=>setToolQuery('')} onBack={()=>switchPage(library)} onPick={chooseExternal} onChanged={contextChanged} onCheck={()=>void checkMissing()} onJobs={()=>setShowJobs(true)} busy={busy||running}/>:<>
     <div className="library-strip"><div><h1>{favoritesOnly?'收藏':historyOnly?'试听历史':tag?`标签 · ${tag}`:playlistId?overview.playlists.find(item=>item.id===playlistId)?.name??'播放列表':debouncedQuery&&searchScope==='global'?'全局搜索':folderInfo?.name??pageNames[library]}</h1><span>{page.total} 条</span>{folder&&<button title={displayPath(folder)} onClick={()=>selectFolder(null)}>返回全部位置</button>}</div><div className="library-filters"><select aria-label="格式筛选" value={format} onChange={event=>{setFormat(event.target.value);setOffset(0);}}>{['全部格式','WAV','AIFF','FLAC','MP3'].map(item=><option key={item}>{item}</option>)}</select><select aria-label="排序方式" value={sort} onChange={event=>{setSort(event.target.value);setOffset(0);}}><option value="name">名称排序</option><option value="duration">时长排序</option><option value="rate">采样率排序</option></select><button title="刷新当前素材列表" disabled={!native||busy} onClick={()=>void action(refresh)}>刷新</button></div></div>
     {libraryToolsOpen&&<section className="library-tools-inline"><header><strong>素材管理与检索</strong><button onClick={()=>setLibraryToolsOpen(false)} aria-label="收起素材管理">×</button></header><ToolsPage native={native} ids={selectedIds} selected={selected} library={library} query="" active={activeTool} area="library" onNavigate={setActiveTool} onClearQuery={()=>{}} onBack={()=>setLibraryToolsOpen(false)} onPick={chooseExternal} onChanged={contextChanged} onCheck={()=>void checkMissing()} onJobs={()=>setShowJobs(true)} busy={busy||running}/></section>}{library==='music'&&<MusicCategories native={native} selected={selected} tags={overview.tags} filter={tag} onFilter={value=>{setTag(value);setOffset(0);}} onChanged={contextChanged}/>}<div className="selection-tools"><button disabled={!selectedIds.length} onClick={()=>openTool('acoustic')}>以声搜声</button><button disabled={selectedIds.length!==1} onClick={()=>openTool('ucs')}>UCS建议</button><button disabled={!selectedIds.length} onClick={()=>openTool('convert')}>导出 / 转换</button><CatalogTools selected={selected} ids={selectedIds} native={native} busy={busy} action={action} refresh={refresh} notify={setStatus} onRelocated={()=>{treeStamp.current='';setSelected(null);setSelection([]);setFolder(null);setOffset(0);}}/><button disabled={!native||busy||!selectedIds.length} onClick={()=>void assignOther()}>加入{pageNames[library==='sound'?'music':'sound']}</button></div>
     <div ref={list} onKeyDown={keyNavigate} className="asset-scroll" tabIndex={0} role="region" aria-label={`${pageNames[library]}素材列表`} aria-busy={pageLoading}><table style={{tableLayout:'fixed',minWidth:shownColumns.reduce((sum,column)=>sum+layout.columns[column.widthIndex],0)}}><colgroup>{shownColumns.map(column=><col key={column.id} style={{width:layout.columns[column.widthIndex]}}/>)}</colgroup><thead onContextMenu={event=>{event.preventDefault();event.stopPropagation();setSidebarPanel(null);setContext(null);setColumnMenu({x:event.clientX,y:event.clientY});}} onKeyDown={event=>{if(event.key==='ContextMenu'||event.shiftKey&&event.key==='F10'){event.preventDefault();event.stopPropagation();const rect=(event.target as HTMLElement).getBoundingClientRect();setSidebarPanel(null);setContext(null);setColumnMenu({x:rect.left,y:rect.bottom});}}}><tr>{shownColumns.map(column=><th key={column.id} tabIndex={0} title="右键设置列显示、隐藏与顺序" aria-haspopup="dialog">{column.label}<ResizeHandle label={`${column.label}列宽`} value={layout.columns[column.widthIndex]} onChange={value=>setLayout(old=>safeLayout({...old,columns:old.columns.map((width,index)=>index===column.widthIndex?value:width)}))}/></th>)}</tr></thead><tbody>{rows.map(asset=><tr id={`asset-${asset.id}`} key={asset.id} onContextMenu={event=>contextAssets(asset,event)} draggable={false} onDragStart={event=>event.preventDefault()} onPointerDown={event=>{if(native&&event.isPrimary&&event.button===0)dragOrigin.current={id:asset.id,x:event.clientX,y:event.clientY,pointer:event.pointerId};}} onPointerMove={event=>dragMove(event,asset)} onPointerUp={()=>{dragOrigin.current=null;}} onPointerCancel={()=>{dragOrigin.current=null;}} onPointerLeave={()=>{dragOrigin.current=null;}} aria-selected={selection.includes(asset.id)} onClick={event=>choose(asset,event)}>{shownColumns.map(column=><td key={column.id} className={column.id==='wave'?'waveform-cell':column.id==='duration'||column.id==='rate'?'mono':column.id==='status'&&asset.error?'asset-error':''}>{column.id==='name'?<button className="asset-name" tabIndex={-1} title="按住名称拖到目标文件夹以复制"><strong>{asset.favorite?'★ ':''}{asset.name}</strong><small title={asset.path}>{displayPath(asset.path)}</small></button>:column.id==='wave'?<MiniWaveform asset={asset} onChoose={chooseExternal}/>:column.id==='duration'?(asset.duration==null?'未知':timeLabel(asset.duration)):column.id==='rate'?(asset.rate?`${asset.rate/1000} kHz`:'—'):column.id==='format'?<span className="format-badge">{asset.format}</span>:({ready:'可用',missing:'文件缺失',unavailable:'不可访问',changed:'已变化',error:'读取失败'} as Record<string,string>)[asset.status]??asset.status}</td>)}</tr>)}</tbody></table>{!rows.length&&<div className="empty-state"><h2>{pageLoading?'正在读取素材…':query||format!=='全部格式'||favoritesOnly||tag||historyOnly||playlistId?'没有匹配素材':`添加你的${library==='music'?'音乐':'音效'}素材`}</h2><p>{favoritesOnly||tag||historyOnly||playlistId?'当前筛选下没有素材，可切换视图或清除筛选。':library==='music'?'可添加音乐文件夹，或从音效库把已选素材加入音乐库。':'选择文件夹后，只读扫描WAV、AIFF、FLAC、MP3。'}</p><button disabled={!native||busy||running} onClick={openImport}>添加素材文件夹</button></div>}</div>
     <div className="list-footer"><span>{rows.length} 条可见 / 共 {page.total} 条</span><div className="pager"><button aria-label="上一页" disabled={offset===0||busy||pageLoading} onClick={()=>{setOffset(Math.max(0,offset-PAGE_SIZE));setSelected(null);}}>‹</button><span>{Math.floor(loadedOffset/PAGE_SIZE)+1} / {Math.max(1,Math.ceil(page.total/PAGE_SIZE))}</span><button aria-label="下一页" disabled={offset+PAGE_SIZE>=page.total||busy||pageLoading} onClick={()=>{setOffset(offset+PAGE_SIZE);setSelected(null);}}>›</button></div></div>
    </>}
   </main>
   {layout.details?<><ResizeHandle label="详情宽度" value={layout.inspector} reverse onChange={value=>resize('inspector',value)}/><aside className="inspector"><div className="panel-heading"><span>素材详情</span><button onClick={()=>toggle('details')} aria-label="折叠右侧详情栏">›</button></div><div className="inspector-content">{selected?<><div className="detail-category">{selected.format} · 原文件引用</div><h2>{selected.name}</h2><dl><div><dt>文件大小</dt><dd>{formatBytes(selected.size)}</dd></div><div><dt>时长</dt><dd>{selected.duration==null?'未知':timeLabel(selected.duration)}</dd></div><div><dt>采样率</dt><dd>{selected.rate?`${selected.rate/1000} kHz`:'未知'}</dd></div><div><dt>声道数</dt><dd>{selected.channels??'未知'}</dd></div></dl><AssetAnnotations key={selected.id} asset={selected} onChanged={patch=>{setSelected(old=>old?.id===selected.id?{...old,...patch}:old);void action(refresh);}}/><details className="file-details" open><summary>原始文件位置</summary><p>{displayPath(selected.path)}</p><p>素材ID：{selected.id}</p></details>{selected.error&&<p className="asset-error">{selected.error}</p>}</>:<p className="description">选择素材查看文件信息、收藏和标签。</p>}</div></aside></>:<aside className="panel-rail right-rail"><button onClick={()=>toggle('details')} aria-label="展开右侧详情栏">‹</button><span>详情</span></aside>}
  </div>
  {!layout.bottomCollapsed&&<ResizeHandle label="波形高度" value={layout.wave} axis="y" reverse onChange={value=>resize('wave',value)}/>}
  <NativePlayer selected={selected} collapsed={layout.bottomCollapsed} onToggle={()=>toggle('bottomCollapsed')} defaultVolume={preferences.volume} onOpenLayer={()=>openTool('layer')}/>
  <footer className="statusbar"><span role="status" aria-live="polite">{liveStatus}</span><span className="status-version" title="当前构建版本，与 EXE 文件属性里的产品版本一致">v{appVersion||'—'}</span><button aria-expanded={showJobs} onClick={()=>setShowJobs(value=>!value)}>任务 {jobs.length} {showJobs?'⌄':'⌃'}</button></footer>
  {showJobs&&<section className="log-panel import-jobs" aria-label="导入任务"><div><strong>导入任务</strong><button aria-label="关闭任务面板" onClick={()=>setShowJobs(false)}>×</button></div>{jobs.length?jobs.map(job=><article className="job" key={job.id}><div><strong>{jobLabels[job.status]??job.status}</strong><span>{pageNames[job.library??'sound']} · {job.processed}/{job.discovered}</span></div><p className="job-path">{displayPath(job.root)}</p><div className="job-counts">新增 {job.added} · 更新 {job.updated} · 跳过 {job.skipped} · 失败 {job.failed}</div>{job.error&&<p className="asset-error">{job.error}</p>}<div className="job-actions">{job.status==='running'&&<button disabled={busy} onClick={()=>void action(async()=>{await invoke('catalog_cancel',{id:job.id});setStatus('已请求取消扫描');await refresh();})}>取消扫描</button>}{!running&&<button disabled={busy} onClick={()=>void action(async()=>{const recovering=resumable(job);const id=await invoke<string>(recovering?'catalog_resume':'catalog_start',recovering?{id:job.id}:{root:job.root,library:job.library??'sound'});setTrackedId(id);setStatus('扫描已启动');await refresh();})}>{resumable(job)?'继续扫描':'重新扫描'}</button>}</div></article>):<p>尚无导入任务。</p>}</section>}
  <dialog ref={dialog} onCancel={event=>{if(busy)event.preventDefault();else setOpen(false);}} onClose={()=>setOpen(false)} aria-labelledby="catalog-dialog-title"><div className="dialog-heading"><h2 id="catalog-dialog-title">添加至{pageNames[importLibrary]}</h2><button disabled={busy} aria-label="关闭添加素材" onClick={()=>setOpen(false)}>×</button></div><p>选择文件夹、填写原路径或将文件夹拖入窗口。音频仍保存在原位置。</p><label className="import-label">素材文件夹<input value={root} onChange={event=>{setRoot(event.target.value);setConfirmedRoot('');}} disabled={busy} placeholder="D:\声音素材"/></label><button className="choose-folder" disabled={busy||!native} onClick={()=>void action(async()=>{const chosen=await invoke<string|null>('catalog_pick_folder',{initial:root||history[0]||null});if(chosen)await validatePath(chosen);})}>选择文件夹…</button>{history.length>0&&<label className="import-label">最近使用<select aria-label="最近使用的素材路径" value="" disabled={busy} onChange={event=>{if(event.target.value){setRoot(displayPath(event.target.value));setConfirmedRoot('');}}}><option value="">选择记住的路径…</option>{history.map(path=><option key={path} value={path}>{displayPath(path)}</option>)}</select></label>}{confirmedRoot&&<div className="import-preview"><strong>目录可读取</strong><p className="job-path">{displayPath(confirmedRoot)}</p><p>将登记至{pageNames[importLibrary]}，不会复制源音频。</p></div>}{error&&<p className="asset-error">{error}</p>}<div className="dialog-actions"><button disabled={!root.trim()||busy} onClick={()=>void action(()=>validatePath(root))}>{busy?'处理中…':'检查目录'}</button><button className="primary" disabled={!confirmedRoot||busy||running} onClick={()=>void start()}>开始添加</button></div></dialog>
  {columnMenu&&<ColumnSettings x={columnMenu.x} y={columnMenu.y} layout={layout} onChange={setLayout} onClose={()=>setColumnMenu(null)}/>}
  {sidebarPanel&&<SidebarQuickPanel key={sidebarPanel.kind} panel={sidebarPanel} overview={overview} onClose={()=>setSidebarPanel(null)} onSelect={(kind,id)=>{setSidebarPanel(null);showCollection(kind==='tags'?'tag':'playlist',id);}}/>}
  <CatalogContext target={context} library={library} overview={overview} onClose={()=>setContext(null)} onChanged={contextChanged} notify={setStatus} onSimilar={asset=>{chooseExternal(asset);openTool('acoustic');}} onLayer={(assets,scatter)=>{try{const result=addToLayer(assets,scatter);setStatus(`已添加 ${result.added} 个叠层片段，跳过 ${result.skipped} 个（需可用且时长已知）`);openTool('layer');}catch(e){setError(String(e));}}}/>
  <Settings open={settingsOpen} onClose={()=>setSettingsOpen(false)} value={preferences} onChange={setPreferences} onResetLayout={()=>setLayout(defaultLayout)} onOpenTool={tool=>{setSettingsOpen(false);openTool(tool);}}/>
 </div>;
}
createRoot(document.getElementById('root')!).render(<React.StrictMode><Catalog/></React.StrictMode>);
