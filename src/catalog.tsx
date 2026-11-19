import React,{useCallback,useEffect,useRef,useState} from 'react';
import {createRoot} from 'react-dom/client';
import {invoke,isTauri} from '@tauri-apps/api/core';
import {getCurrentWebview} from '@tauri-apps/api/webview';
import {formatBytes,jobLabels,resumable,displayPath} from './catalog-client';
import type {CatalogAsset,CatalogPage,ImportJob,FolderNode} from './catalog-client';
import {FolderTree} from './folder-tree';
import {NativePlayer} from './native-player';
import {AssetDetail} from './asset-detail';
import {timeLabel} from './demo-data';
import {useWorkbenchLayout,ResizeHandle} from './layout';
import {safeLayout,selectIds,defaultLayout,visibleColumns,safeSort,nextSort,type SortField,type ColumnId,type Layout} from './workbench-state';
import {ColumnSettings} from './column-settings';
import {PlaylistPicker} from './playlist-picker';
import {FavoriteGroupPicker} from './favorite-group-picker';
import {useAppVersion} from './app-version';
import {CatalogTools} from './catalog-tools';
import {MiniWaveform} from './waveform';
import {BrandMark} from './brand-mark';
import {Settings} from './settings';
import {AccountMenu} from './account-menu';
import {SidebarQuickPanel,type SidebarPanel,type SidebarPanelKind} from './sidebar-quick';
import {SidebarDockFrame,SidebarIconRail,NavGlyph,useSidebarDock,type RailItem} from './sidebar-dock';
import {DockFrame} from './panel-dock';
import {usePlayerView,type PlayerPlacement} from './player-dock';
import {CatalogContext,type ContextTarget,type LibraryOverview} from './catalog-context';
import {searchFolder,type SearchScope} from './search-scope';
import {addToLayer} from './layer-tools';
import {ToolsWindow} from './tools-window';
import {ExtSetup,extSetupDone} from './ext-setup';
import {AccountWindow} from './account-window';
import {ErrorBoundary,installCrashHandlers} from './error-boundary';
import {FavoriteChips,TagsIndex} from './saved-views';
import {armedLabels,getArmed} from './audio-toolbar';
import {PluginWorkspace} from './plugins/workspace';
import {notify,NoticeStack} from './notices';
import {TaskChip,TaskDrawer} from './task-center';
import type {ToolPage} from './tool-navigation';
import {MusicFilters,type DurationRange} from './music-filters';
import {usePreferences,type WorkspacePage} from './preferences';
import './app.css';

const PAGE_SIZE=100;
const pageNames={sound:'音效库',music:'音乐库'};
type Library='sound'|'music';
type View={offset:number;folder:string|null;query:string;format:string;sort:string;favorites:boolean;favoriteGroup:string|null;favoriteUngrouped:boolean;searchScope:SearchScope;tag:string|null;historyOnly:boolean;playlistId:string|null;duration:DurationRange;extraTags:string[]};
const emptyView=():View=>({offset:0,folder:null,query:'',format:'全部格式',sort:'name',favorites:false,favoriteGroup:null,favoriteUngrouped:false,searchScope:'global',tag:null,historyOnly:false,playlistId:null,duration:{min:null,max:null},extraTags:[]});
const statusText:Record<string,string>={ready:'可用',missing:'文件缺失',unavailable:'不可访问',changed:'已变化',error:'读取失败'};
/** 分类列取「维度:值」形式的标签，没有维度标签时退回到第一个普通标签。 */
function categoryOf(asset:CatalogAsset){
 const values=asset.tags.filter(tag=>/^(流派|曲风|情绪|用途|乐器|人声|分类)[:：]/.test(tag)).map(tag=>tag.replace(/^[^:：]+[:：]/,'').trim()).filter(Boolean);
 return values.length?values:asset.tags.length?[asset.tags[0]]:[];
}
function tagValue(tags:string[],key:string){
 const found=tags.find(tag=>tag.startsWith(key+':')||tag.startsWith(key+'：'));
 return found?found.slice(key.length+1).trim()||'—':'—';
}
function channelLabel(channels:number|null){return channels==null?'—':channels===1?'单声道':channels===2?'立体声':channels===6?'5.1':`${channels} 声道`;}
/** 列表名去掉音频扩展名；格式另有列与详情面板可查，原名保留在悬停提示里。 */
function shortName(name:string){return name.replace(/\.(wav|aif|aiff|flac|mp3|ogg|m4a|aac|wma)$/i,'');}
function Catalog(){
 const appVersion=useAppVersion();
 const [pinsOpen,setPinsOpen]=useState(()=>{try{return localStorage.getItem('sound-workbench-pins-open-v1')==='1';}catch{return false;}});
 useEffect(()=>{try{localStorage.setItem('sound-workbench-pins-open-v1',pinsOpen?'1':'0');}catch{/* 存储不可用不影响展开 */}},[pinsOpen]);
 const [shortcutsCompact,setShortcutsCompact]=useState(()=>{try{return localStorage.getItem('sound-workbench-sidebar-shortcuts-compact-v1')==='1';}catch{return false;}});
 useEffect(()=>{try{localStorage.setItem('sound-workbench-sidebar-shortcuts-compact-v1',shortcutsCompact?'1':'0');}catch{/* 存储不可用不影响图标模式 */}},[shortcutsCompact]);
 const native=isTauri(),{preferences,setPreferences}=usePreferences();
 const {layout,setLayout,style}=useWorkbenchLayout(preferences.rememberLayout);
 const {dock,setDock}=useSidebarDock();
 const {view:playerView,setPlacement:setPlayerPlacement,setDock:setPlayerDock}=usePlayerView();
 /** 选「左侧栏」时先展开左栏；左栏被手动折叠后波形自动回到底部，避免播放器找不到。 */
 const changePlayerPlacement=(next:PlayerPlacement)=>{
  setPlayerPlacement(next);
  if(next==='sidebar')setLayout(old=>({...old,leftCollapsed:false}));
 };
 useEffect(()=>{if(playerView.placement==='sidebar'&&layout.leftCollapsed)setPlayerPlacement('bottom');},[playerView.placement,layout.leftCollapsed,setPlayerPlacement]);
 const [workspacePage,setWorkspacePage]=useState<WorkspacePage>(preferences.startPage),[library,setLibrary]=useState<Library>(preferences.startPage==='music'?'music':'sound');
 // 窗口标题与顶栏品牌保持同一写法：SFX TOOLS · 当前页面。
 useEffect(()=>{document.title=`SFX TOOLS · ${pageNames[library]}`;},[library]);
 const shownColumns=visibleColumns(layout,library);
 const [columnMenu,setColumnMenu]=useState<{x:number;y:number}|null>(null);
 const [playlistMenu,setPlaylistMenu]=useState<{x:number;y:number;ids:string[]}|null>(null);
 const [groupMenu,setGroupMenu]=useState<{x:number;y:number;ids:string[]}|null>(null);
 const [sidebarPanel,setSidebarPanel]=useState<SidebarPanel|null>(null);
 const [context,setContext]=useState<ContextTarget|null>(null),[overview,setOverview]=useState<LibraryOverview>({tags:[],pins:[],playlists:[],history_count:0,favorite_count:0,favorite_ungrouped:0,favorite_groups:[]});
 const [searchScope,setSearchScope]=useState<SearchScope>('global'),[tag,setTag]=useState<string|null>(null),[historyOnly,setHistoryOnly]=useState(false),[playlistId,setPlaylistId]=useState<string|null>(null);
 /** 标签索引页（0.1.34）：true 时主区铺标签网格，点标签才切回列表视图。 */
 const [tagIndex,setTagIndex]=useState(false);
 const [settingsOpen,setSettingsOpen]=useState(false),[managerOpen,setManagerOpen]=useState(()=>new URLSearchParams(location.search).has('manager')),[extSetupOpen,setExtSetupOpen]=useState(()=>new URLSearchParams(location.search).has('extsetup')),[accountOpen,setAccountOpen]=useState(false),[activeTool,setActiveTool]=useState<ToolPage|null>(null);
 // 首次启动（含新机器）把三个外部程序一次配好；配过就不再打扰，跳过的下次还能补。
 useEffect(()=>{if(!native||extSetupDone())return;setExtSetupOpen(true);},[native]);
 /** 插件工作区窗口（0.1.35）：入口在顶栏「音乐库」右侧，激活可插拔模块后在这里真正干活。 */
 const [pluginOpen,setPluginOpen]=useState(()=>new URLSearchParams(location.search).has('plugin'));
 const [page,setPage]=useState<CatalogPage>({items:[],total:0}),[offset,setOffset]=useState(0),[loadedOffset,setLoadedOffset]=useState(0),[jobs,setJobs]=useState<ImportJob[]>([]);
 const [query,setQuery]=useState(''),[format,setFormat]=useState('全部格式'),[selected,setSelected]=useState<CatalogAsset|null>(null),[selection,setSelection]=useState<string[]>([]);
 const [debouncedQuery,setDebouncedQuery]=useState(''),[sort,setSort]=useState('name'),[favoritesOnly,setFavoritesOnly]=useState(false),[favoriteGroup,setFavoriteGroup]=useState<string|null>(null),[favoriteUngrouped,setFavoriteUngrouped]=useState(false),[loadedKey,setLoadedKey]=useState('');
 const [busy,setBusy]=useState(false),[error,setError]=useState(''),[status,setStatus]=useState(native?'本地素材 · 原路径引用':'真实素材需要桌面版');
 const [duration,setDuration]=useState<DurationRange>({min:null,max:null});
 const [extraTags,setExtraTags]=useState<string[]>([]);
 // 「隐藏不可用素材」是列表级过滤：与搜索关键词、格式、收藏等条件同时进入后端查询，避免只在前端遮住当前页。
 const [hideUnavailable,setHideUnavailable]=useState(()=>{try{return localStorage.getItem('sound-workbench-hide-unavailable-v1')==='1';}catch{return false;}});
 useEffect(()=>{try{localStorage.setItem('sound-workbench-hide-unavailable-v1',hideUnavailable?'1':'0');}catch{/* 本地存储不可用时不阻断界面 */}},[hideUnavailable]);
 const [root,setRoot]=useState(''),[confirmedRoot,setConfirmedRoot]=useState(''),[open,setOpen]=useState(false),[trackedId,setTrackedId]=useState(''),[importLibrary,setImportLibrary]=useState<Library>(library);
 // 任务抽屉：默认收起；开合状态记忆到下次启动，「已结束」行按需清空。
 const [jobsOpen,setJobsOpen]=useState(()=>{try{return localStorage.getItem('sound-workbench-task-drawer-v1')==='1';}catch{return false;}});
 const [hiddenJobs,setHiddenJobs]=useState<string[]>([]);
 useEffect(()=>{try{localStorage.setItem('sound-workbench-task-drawer-v1',jobsOpen?'1':'0');}catch{/* 存储不可用不影响任务展示 */}},[jobsOpen]);
 const [folders,setFolders]=useState<FolderNode[]>([]),[folder,setFolder]=useState<string|null>(null),[history,setHistory]=useState<string[]>([]),[dragging,setDragging]=useState(false);
 const views=useRef<Record<Library,View>>({sound:emptyView(),music:emptyView()}),anchor=useRef<string|null>(null),list=useRef<HTMLDivElement>(null);
 const treeStamp=useRef(''),dropHandler=useRef<(paths:string[])=>void>(()=>{}),jobStates=useRef<Record<string,string>>({});
 const lock=useRef(false),dialog=useRef<HTMLDialogElement>(null),search=useRef<HTMLInputElement>(null),generation=useRef(0),alive=useRef(true);
 const effectiveFolder=searchFolder(folder,debouncedQuery,searchScope);
 const scopeKey=JSON.stringify([library,effectiveFolder,offset,debouncedQuery,format,sort,favoritesOnly,favoriteGroup,favoriteUngrouped,tag,historyOnly,playlistId,duration,extraTags,preferences.multilingualSearch,preferences.metadataSearch,hideUnavailable]);
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
  views.current[library]={offset,folder,query,format,sort,favorites:favoritesOnly,favoriteGroup,favoriteUngrouped,searchScope,tag,historyOnly,playlistId,duration,extraTags};setWorkspacePage(next);
  const view=views.current[next];setLibrary(next);setFolder(view.folder);setQuery(view.query);setDebouncedQuery(view.query);setFormat(view.format);setSort(view.sort);setFavoritesOnly(view.favorites);setFavoriteGroup(view.favoriteGroup);setFavoriteUngrouped(view.favoriteUngrouped);setSearchScope(view.searchScope);setTag(view.tag);setHistoryOnly(view.historyOnly);setPlaylistId(view.playlistId);setDuration(view.duration);setExtraTags(view.extraTags);setOffset(view.offset);if(next!==library){setSelected(null);setSelection([]);}treeStamp.current='';
 };
 /** 打开管理窗口；tool 为空表示停在总览。 */
 const openTool=(tool:ToolPage|null)=>{setActiveTool(tool);setManagerOpen(true);};
 const audition=(asset:CatalogAsset)=>{void invoke('player_load',{id:asset.id}).catch(e=>{if(alive.current)setError(String(e));});};
 const choose=(asset:CatalogAsset,event?:{ctrlKey:boolean;metaKey:boolean;shiftKey:boolean})=>{
  const additive=!!event&&(event.ctrlKey||event.metaKey);
  setSelection(old=>selectIds(rows.map(item=>item.id),old,asset.id,anchor.current,additive,!!event?.shiftKey));
  if(!event?.shiftKey)anchor.current=asset.id;
  const same=selected?.id===asset.id;setSelected(asset);
  // 图片#2：选中即试听 / 重新选中同一文件时从头播放；多选与范围选择不触发。
  if(native&&preferences.auditionOnSelect&&asset.status==='ready'&&!additive&&!event?.shiftKey&&(!same||preferences.replaySameFromStart))audition(asset);
 };
 const chooseExternal=(item:CatalogAsset)=>{setSelected(item);setSelection([item.id]);};
 const toggleRowFavorite=(asset:CatalogAsset)=>{void (async()=>{try{const favorite=!asset.favorite;await invoke('catalog_set_favorite',{id:asset.id,favorite});setSelected(old=>old?.id===asset.id?{...old,favorite}:old);await refresh();}catch(e){setError(String(e));}})();};
 const refresh=useCallback(async()=>{
  if(scopeRef.current!==scopeKey)return;const token=++generation.current;
  const [next,nextJobs,nextOverview]=await Promise.all([invoke<CatalogPage>('catalog_list',{offset,limit:PAGE_SIZE,folder:effectiveFolder,query:debouncedQuery,format:format==='全部格式'?null:format,sort,favoritesOnly,favoriteGroup,favoriteUngrouped,library,tag,historyOnly,playlistId,tags:extraTags.length?extraTags:null,minDuration:duration.min,maxDuration:duration.max,multilingual:preferences.multilingualSearch,metadata:preferences.metadataSearch,readyOnly:hideUnavailable}),invoke<ImportJob[]>('catalog_jobs'),invoke<LibraryOverview>('library_overview',{library})]);
  if(!alive.current||token!==generation.current||scopeRef.current!==scopeKey)return;
  if(offset>0&&next.total<=offset){setOffset(Math.max(0,Math.floor(Math.max(0,next.total-1)/PAGE_SIZE)*PAGE_SIZE));return;}
  setPage(next);setLoadedOffset(offset);setLoadedKey(scopeKey);setJobs(nextJobs);setOverview(nextOverview);
  const stamp=JSON.stringify([library,nextJobs.map(job=>[job.id,job.status,job.processed,job.updated,job.added])]);
  if(treeStamp.current!==stamp){
   const [tree,paths]=await Promise.all([invoke<FolderNode[]>('catalog_folders',{library}),invoke<string[]>('catalog_history')]);
   if(!alive.current||token!==generation.current||scopeRef.current!==scopeKey)return;
   setFolders(tree);setHistory(paths);treeStamp.current=stamp;
  }
  // 任务状态迁移才提示：首次读到的历史任务不弹 toast，避免重启后一次刷出旧结果。
  const seen=jobStates.current;
  for(const job of nextJobs){
   const before=seen[job.id];seen[job.id]=job.status;
   if(!before||before===job.status)continue;
   const place=displayPath(job.root);
   if(job.status==='completed')notify('success','导入完成',`新增 ${job.added} · 更新 ${job.updated} · ${place}`);
   else if(['completed_with_errors','failed','interrupted'].includes(job.status))notify('error','导入未完成',`${job.failed} 项失败 · ${place}`);
   else if(job.status==='cancelled')notify('warning','导入已取消',place);
  }
  setSelected(old=>old?next.items.find(item=>item.id===old.id)??old:null);
 },[offset,effectiveFolder,debouncedQuery,format,sort,favoritesOnly,favoriteGroup,favoriteUngrouped,library,tag,historyOnly,playlistId,duration,extraTags,scopeKey,preferences.multilingualSearch,preferences.metadataSearch,hideUnavailable]);
 useEffect(()=>{alive.current=true;return()=>{alive.current=false;generation.current++;};},[]);
 // 启动时把本机保存的输出设备名送回播放器；设备不存在时后端自动回落系统默认。
 useEffect(()=>{if(!native)return;void invoke('settings_player_set_device',{name:preferences.outputDevice||'default'}).catch(()=>{});},[native,preferences.outputDevice]);
 useEffect(()=>{
  if(!native)return;let stopped=false,timer:number;
  const poll=async()=>{try{await refresh();}catch(e){if(!stopped)setError(String(e));}finally{if(!stopped)timer=window.setTimeout(poll,1000);}};
  void poll();return()=>{stopped=true;generation.current++;clearTimeout(timer);};
 },[native,refresh]);
 useEffect(()=>{
  if(!native)return;let disposed=false,stop:(()=>void)|undefined;
  void getCurrentWebview().onDragDropEvent(event=>{
   if(disposed)return;
   // 拖出素材后又拖回本窗口时，系统同样会派发拖入事件；这是「取消拖出」，不是导入文件夹。
   const dragBack=dragOutActive.current||Date.now()<dragOutGraceUntil.current;
   if(event.payload.type==='enter'||event.payload.type==='over'){if(!dragBack)setDragging(true);}
   else{
    setDragging(false);
    if(event.payload.type==='drop'){
     if(dragBack){setStatus('素材已拖回窗口：按取消拖出处理，未重复导入。');return;}
     dropHandler.current(event.payload.paths);
    }
   }
  }).then(unlisten=>{if(disposed)unlisten();else stop=unlisten;}).catch(e=>{if(!disposed)setError(`文件夹拖入不可用：${String(e)}`);});
  return()=>{disposed=true;stop?.();};
 },[native]);
 useEffect(()=>{const element=dialog.current;if(open&&!element?.open)element?.showModal();else if(!open&&element?.open)element.close();},[open]);
 useEffect(()=>{const handler=(event:KeyboardEvent)=>{if(!open&&!settingsOpen&&!managerOpen&&!accountOpen&&(event.ctrlKey||event.metaKey)&&event.key.toLowerCase()==='k'){event.preventDefault();setSidebarPanel(null);setColumnMenu(null);setLayout(old=>({...old,topCollapsed:false}));requestAnimationFrame(()=>search.current?.focus());}};window.addEventListener('keydown',handler);return()=>window.removeEventListener('keydown',handler);},[open,settingsOpen,managerOpen,accountOpen,setLayout]);
 async function action(work:()=>Promise<void>){if(lock.current)return;lock.current=true;setBusy(true);setError('');try{await work();}catch(e){if(alive.current)setError(String(e));}finally{lock.current=false;if(alive.current)setBusy(false);}}
 async function validatePath(path:string){setRoot(displayPath(path));setConfirmedRoot('');const checked=await invoke<string>('catalog_preview',{root:path.trim().replace(/^"|"$/g,'')});setConfirmedRoot(checked);setHistory(await invoke<string[]>('catalog_history'));}
 const openImport=()=>{setImportLibrary(library);setRoot(displayPath(history[0]??''));setConfirmedRoot('');setError('');setOpen(true);};
 dropHandler.current=paths=>{if(!open&&document.querySelector('dialog[open]')){setStatus('请先完成或关闭当前对话框，再拖入文件夹');return;}if(lock.current){setStatus('当前操作未结束，请稍后再次拖入');return;}if(paths.length!==1){setError('请一次拖入一个素材文件夹。');return;}setImportLibrary(library);setOpen(true);void action(()=>validatePath(paths[0]));};
 const clearViews=()=>{setFavoritesOnly(false);setFavoriteGroup(null);setFavoriteUngrouped(false);setTag(null);setHistoryOnly(false);setPlaylistId(null);setTagIndex(false);setDuration({min:null,max:null});setExtraTags([]);setOffset(0);setSelected(null);};
 const selectFolder=(path:string|null)=>{setFolder(path);setSearchScope('global');clearViews();};
 const showCollection=(kind:'favorites'|'tag'|'history'|'playlist',value:string|null=null)=>{clearViews();setFolder(null);setQuery('');setDebouncedQuery('');if(kind==='favorites')setFavoritesOnly(true);else if(kind==='tag')setTag(value);else if(kind==='history')setHistoryOnly(true);else setPlaylistId(value);};
 const contextChanged=(clearSelection=false)=>{treeStamp.current='';generation.current++;if(clearSelection){setSelected(null);setSelection([]);setFolder(null);}void refresh().catch(e=>setError(String(e)));};
 const contextAssets=(asset:CatalogAsset,event:React.MouseEvent)=>{setColumnMenu(null);event.preventDefault();event.stopPropagation();dragOrigin.current=null;const assets=selection.includes(asset.id)?rows.filter(item=>selection.includes(item.id)):[asset];if(!selection.includes(asset.id))choose(asset);setContext({x:event.clientX,y:event.clientY,assets});};
 const start=()=>action(async()=>{const id=await invoke<string>('catalog_start',{root:confirmedRoot,library:importLibrary});setTrackedId(id);setOpen(false);setStatus(`正在添加至${pageNames[importLibrary]}`);notify('info','已开始添加素材',`${pageNames[importLibrary]} · ${displayPath(confirmedRoot)}`);await refresh();});
 const checkMissing=()=>action(async()=>{const report=await invoke<{checked:number;available:number;missing:number;unavailable:number;changed:number}>('catalog_check_availability',{folder});const empty=report.missing+report.unavailable+report.changed===0;setStatus(`检查 ${report.checked} 条：可用 ${report.available}，缺失 ${report.missing}，不可访问 ${report.unavailable}，变化 ${report.changed}`);notify(empty?'success':'warning',empty?`检查完成：全部可用`:`检查完成：${report.missing+report.unavailable+report.changed} 条需复核`,`共 ${report.checked} 条 · 可用 ${report.available} · 缺失 ${report.missing} · 变化 ${report.changed}`);treeStamp.current='';await refresh();});
 const assignOther=()=>action(async()=>{const target:Library=library==='sound'?'music':'sound';const count=await invoke<number>('catalog_assign_library',{ids:selectedIds,library:target});setStatus(`已添加 ${count} 条引用至${pageNames[target]}，源文件保持原位`);notify('success',`已加入${pageNames[target]}`,`${count} 条引用 · 源文件未移动`);treeStamp.current='';await refresh();});
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
 /** 拖出进行中与其后短暂宽限期内，窗口内的拖入事件视为「拖回取消」，不触发导入。 */
 const dragOutActive=useRef(false),dragOutGraceUntil=useRef(0);
 const dragMove=(event:React.PointerEvent<HTMLTableRowElement>,asset:CatalogAsset)=>{
  const origin=dragOrigin.current;if(!(event.buttons&1)){dragOrigin.current=null;return;}
  if(!origin||origin.id!==asset.id||origin.pointer!==event.pointerId||Math.hypot(event.clientX-origin.x,event.clientY-origin.y)<6)return;
  dragOrigin.current=null;event.preventDefault();if(!native||dragOutLock.current)return;dragOutLock.current=true;dragOutActive.current=true;setSelected(asset);setError('');
  const ids=selection.includes(asset.id)?selection:[asset.id];
  // 图片#2：拖到 DAW 前停止本软件试听，避免与宿主同时占用输出设备。
  if(preferences.stopOnDragToDaw)void invoke('player_control',{action:'stop',value:null}).catch(()=>{});
  // 音频工具激活态（0.1.34）：命中的素材已生成对应处理文件时，拖出的是处理后的 WAV；
  // 未就绪（还在生成、或结果失效）就退回拖原文件，并说明原因，绝不错拖。
  const armed=getArmed();
  const armedHit=armed&&armed.jobId&&armed.assetId===asset.id&&ids.length===1?armed:null;
  if(armedHit){
   void invoke<string>('catalog_drag_processed',{jobId:armedHit.jobId,index:0}).then(message=>{if(alive.current)setStatus(`${armedLabels[armedHit.op]}结果 · ${message}`);}).catch(()=>{
    if(alive.current)setStatus(`${armedLabels[armedHit.op]}结果未就绪，本次拖出为原文件`);
    void invoke<string>('catalog_drag_many',{ids}).then(message=>{if(alive.current)setStatus(message);}).catch(e=>{if(alive.current)setError(String(e));});
   }).finally(()=>{dragOutLock.current=false;dragOutGraceUntil.current=Date.now()+1500;dragOutActive.current=false;});
   return;
  }
  void invoke<string>('catalog_drag_many',{ids}).then(message=>{if(alive.current)setStatus(message);}).catch(e=>{if(alive.current)setError(String(e));}).finally(()=>{dragOutLock.current=false;dragOutGraceUntil.current=Date.now()+1500;dragOutActive.current=false;});
 };
 const sortState=safeSort(sort);
 /** 单元格内容：所有列都走同一入口，列设置里勾选/隐藏/排序后立即生效。 */
 const renderCell=(id:ColumnId,asset:CatalogAsset,rowIndex:number):React.ReactNode=>{
  switch(id){
   case 'index':return <span className="row-index">{offset+rowIndex+1}</span>;
   case 'name':{
    const bad=asset.status&&asset.status!=='ready',label=statusText[asset.status]??asset.status;
    return <button className="asset-name" tabIndex={-1} title={`${asset.name}\n${displayPath(asset.path)}\n按住名称拖到目标文件夹以复制`}>
     {bad&&<span className="asset-status-dot" title={`${label}${asset.error?' · '+asset.error:''}`} aria-label={label}>!</span>}
     <strong>{shortName(asset.name)}</strong>
    </button>;
   }
   case 'wave':return <MiniWaveform asset={asset} onChoose={chooseExternal} simple={preferences.simpleWaveform}/>;
   case 'category':{
    const values=categoryOf(asset);
    if(!values.length)return <span className="cell-empty">—</span>;
    return <span className="cell-tags">{values.slice(0,2).map(value=><span key={value} className="cell-tag" title={value}>{value}</span>)}{values.length>2&&<span className="cell-tag-more" title={values.slice(2).join('、')}>+{values.length-2}</span>}</span>;
   }
   case 'duration':return asset.duration==null?'未知':timeLabel(asset.duration);
   case 'bpm':return tagValue(asset.tags,'BPM');
   case 'key':return tagValue(asset.tags,'调性');
   case 'channels':return channelLabel(asset.channels);
   case 'rate':return asset.rate?`${asset.rate/1000} kHz`:'—';
   case 'format':return <span className="format-badge">{asset.format}</span>;
   case 'favorite':return <button className={`row-icon-button row-favorite${asset.favorite?' is-on':''}`} aria-pressed={asset.favorite} aria-label={asset.favorite?'取消收藏':'加入收藏'} title={asset.favorite?'取消收藏':'加入收藏'} onPointerDown={event=>event.stopPropagation()} onClick={event=>{event.stopPropagation();toggleRowFavorite(asset);}}>{asset.favorite?'♥':'♡'}</button>;
   case 'playlist':return <button className="row-icon-button" aria-label={`把 ${shortName(asset.name)} 加入播放列表`} title="加入播放列表" onPointerDown={event=>event.stopPropagation()} onClick={event=>{const rect=event.currentTarget.getBoundingClientRect();event.stopPropagation();setPlaylistMenu({x:rect.right,y:rect.bottom,ids:selection.includes(asset.id)&&selection.length?selection:[asset.id]});}}>≡</button>;
   default:return null;
  }
 };
 /** 展开态 2×3 网格、浮动展开态图标网格、折叠态纯图标条共用同一套导航行为。 */
 const showAll=()=>{setSidebarPanel(null);selectFolder(null);setQuery('');setFormat('全部格式');};
 const showFavorites=()=>{setSidebarPanel(null);showCollection('favorites');};
 /** 标签页（0.1.34）：先给索引，点某个标签才筛到列表；和历史页同一层次，不再是小弹窗。 */
 const showTagsIndex=()=>{setSidebarPanel(null);clearViews();setFolder(null);setQuery('');setDebouncedQuery('');setTagIndex(true);};
 const showHistory=()=>{setSidebarPanel(null);showCollection('history');};
 const selectFavoriteGroup=(group:string|null,ungrouped:boolean)=>{setSidebarPanel(null);setFavoritesOnly(true);setFavoriteGroup(group);setFavoriteUngrouped(ungrouped);setOffset(0);};
 const toggleJobs=()=>{setSidebarPanel(null);setJobsOpen(value=>!value);};
 const openKind=(kind:SidebarPanelKind,event:React.MouseEvent<HTMLButtonElement>)=>{const rect=event.currentTarget.getBoundingClientRect();setSidebarPanel(old=>old?.kind===kind?null:{kind,x:rect.right+6,y:rect.top});};
 const railItems:RailItem[]=[
  {id:'favorites',label:'收藏',glyph:'favorites',active:favoritesOnly,onClick:event=>openKind('favorites',event)},
  {id:'tags',label:'标签',glyph:'tags',active:!!tag,onClick:event=>openKind('tags',event)},
  {id:'history',label:'试听历史',glyph:'history',active:historyOnly,onClick:showHistory},
  {id:'playlists',label:'播放列表',glyph:'playlists',active:!!playlistId,onClick:event=>openKind('playlists',event)}
 ];
 const railExtra:RailItem[]=[
  {id:'all',label:'全部素材',glyph:'all',active:!folder&&!favoritesOnly&&!tag&&!historyOnly&&!playlistId,onClick:showAll},
  {id:'jobs',label:`任务 · ${jobs.length}项`,glyph:'jobs',active:jobsOpen,onClick:toggleJobs}
 ];
 // 同一时刻只挂载一份播放器：按落位插到底部、左侧栏或浮窗，避免重复轮询与重复波形分析。
 const playerPanel=<NativePlayer selected={selected} collapsed={playerView.placement==='bottom'&&layout.bottomCollapsed} onToggle={playerView.placement==='bottom'?()=>toggle('bottomCollapsed'):undefined} defaultVolume={preferences.volume} placement={playerView.placement} onPlacementChange={changePlayerPlacement}/>;
 const playerSnapSize=Math.max(120,layout.wave+90);
 // 导入任务改为状态栏胶囊 + 抽屉（task-center.tsx），不再有常驻浮窗。
 // 目录树行高由偏好决定（紧凑 24 / 标准 28），用 CSS 变量注入根节点，避免改 CSS 默认值影响旧存档。
 return <div className={`app-shell catalog-shell workspace-v2 wave-${preferences.waveColor}`} style={{...style,'--tree-row':preferences.treeDensity==='standard'?'28px':'24px'} as React.CSSProperties}>
  {dragging&&<div className="folder-drop-overlay"><strong>松开以检查素材文件夹</strong><span>添加至{pageNames[library]} · 确认后才导入</span></div>}
  <header className={`workspace-header ${layout.topCollapsed?'is-collapsed':''}`}>
   <div className="workspace-navigation"><div className="compact-brand"><BrandMark/></div><nav aria-label="工作台页面">{(['sound','music'] as WorkspacePage[]).map(item=><button key={item} aria-current={workspacePage===item?'page':undefined} onClick={()=>switchPage(item)}>{pageNames[item]}</button>)}<button className="workspace-plugin-entry" aria-haspopup="dialog" aria-expanded={pluginOpen} title="插件工作区：AAF／OMF 导入、PT 叠层探测、批量导出预览（只读）" onClick={()=>setPluginOpen(true)}>扩展</button></nav><div className="header-actions"><AccountMenu native={native} onManage={()=>setAccountOpen(true)} notify={setStatus}/><button className="header-manage" onClick={()=>{setActiveTool(null);setManagerOpen(true);}}>管理</button><button onClick={()=>setSettingsOpen(true)} title="设置" aria-label="打开设置">⚙<span>设置</span></button><button aria-label={layout.topCollapsed?'展开顶栏':'折叠顶栏'} aria-expanded={!layout.topCollapsed} onClick={()=>toggle('topCollapsed')}>{layout.topCollapsed?'⌄':'⌃'}</button></div></div>
   {!layout.topCollapsed&&<div className="workspace-commandbar"><button className="add-source" disabled={!native||busy||running} onClick={openImport}>＋ 添加{pageNames[library]==='音乐库'?'音乐':'音效'}</button><label className="source-location"><span>素材位置</span><select aria-label="素材存放位置（原路径筛选）" value={folder??''} onChange={event=>selectFolder(event.target.value||null)}><option value="">全部{pageNames[library]}位置</option>{folders.map(node=><option key={node.path} value={node.path}>{displayPath(node.path)}</option>)}</select></label><div className="top-search"><button className="search-scope" aria-label="切换全局或当前文件夹搜索" aria-pressed={searchScope==='folder'} disabled={!folder} title={folder?'点击切换全局/当前文件夹':'未选择文件夹，搜索当前库全部位置'} onClick={()=>{setSearchScope(value=>value==='global'?'folder':'global');setOffset(0);}}>{searchScope==='folder'&&folder?'当前文件夹':'全局'}</button><span aria-hidden="true">⌕</span><button className="search-toggle" aria-pressed={preferences.multilingualSearch} title="跨语言搜索：输一种语言，同时搜中/英/日；公式与分类搜索不受影响" onClick={()=>setPreferences(old=>({...old,multilingualSearch:!old.multilingualSearch}))}>跨语言</button><button className="search-toggle" aria-pressed={preferences.metadataSearch} title="含元数据：搜索时一并匹配 WAV 内嵌的 BWF / iXML 描述、场景、备注、关键词、分类与许可证等；关闭后只搜文件名。此开关与设置 > 素材库联动，改动会保存" onClick={()=>setPreferences(old=>({...old,metadataSearch:!old.metadataSearch}))}>含元数据</button><input ref={search} aria-label={preferences.metadataSearch?'搜索素材名称、路径、标签和内嵌元数据':'搜索素材文件名'} placeholder={preferences.metadataSearch?`搜索${pageNames[library]}名称、路径、标签、元数据…`:`搜索${pageNames[library]}文件名…`} value={query} maxLength={1024} onChange={event=>setQuery(event.target.value)}/>{query?<button aria-label="清除搜索" onClick={()=>setQuery('')}>×</button>:<kbd>Ctrl K</kbd>}</div></div>}
  </header>
  {error&&<div className="catalog-error shell-error" role="alert">{error}<button onClick={()=>setError('')} aria-label="关闭错误提示">×</button></div>}
  <div className="workspace">
   <SidebarDockFrame dock={dock} onChange={setDock} collapsed={layout.leftCollapsed}>
    {({floating,toggleDock})=>layout.leftCollapsed
     ?<SidebarIconRail items={railItems} extra={railExtra} onExpand={()=>toggle('leftCollapsed')} floating={floating} toggleDock={toggleDock}/>
     :<>
      <aside className="sidebar">
       <div className="panel-heading" title="按住标题栏拖动侧栏；双击可停靠 / 浮动">
        <span>{pageNames[library]}</span>
        <div className="panel-heading-actions">
         <button onClick={toggleDock} title={floating?'停靠到左侧':'浮动为独立窗口'} aria-label={floating?'停靠到左侧':'浮动为独立窗口'}>{floating?'⇤':'⤢'}</button>
         <button onClick={()=>setShortcutsCompact(value=>!value)} aria-pressed={shortcutsCompact} title={shortcutsCompact?'展开快捷入口文字':'把快捷入口折叠为图标'} aria-label={shortcutsCompact?'展开快捷入口文字':'把快捷入口折叠为图标'}>{shortcutsCompact?'▤':'▦'}</button>
         
        </div>
       </div>
       <nav className={`sidebar-shortcuts${shortcutsCompact?' is-compact':''}`} aria-label="素材导航">
        <button className={favoritesOnly?'active':''} aria-current={favoritesOnly?'page':undefined} title={`收藏 · ${overview.favorite_count}条`} aria-label={`打开收藏页，${overview.favorite_count}条`} onClick={showFavorites}><NavGlyph id="favorites"/></button>
        <button className={historyOnly?'active':''} aria-current={historyOnly?'page':undefined} title={`试听历史 · ${overview.history_count}条`} aria-label={`打开试听历史页，${overview.history_count}条`} onClick={showHistory}><NavGlyph id="history"/></button>
        <button className={tag||tagIndex?'active':''} aria-current={tag||tagIndex?'page':undefined} title={`标签 · ${overview.tags.length}项`} aria-label={`打开标签页，${overview.tags.length}项`} onClick={showTagsIndex}><NavGlyph id="tags"/></button>
       </nav>
       <div className="sidebar-label tree-label"><span>路径目录</span><button type="button" className="tree-all" title={`全部素材 · ${totalAssets}条`} onClick={showAll}>全部 {totalAssets}</button></div>
       <FolderTree nodes={folders} selected={folder} onContext={(path,event)=>setContext({x:event.clientX,y:event.clientY,folder:path})} onSelect={selectFolder}/>
       <div className={`sidebar-pins${pinsOpen?' is-open':''}`}><button className="pins-toggle" aria-expanded={pinsOpen} aria-controls="sidebar-pin-list" onClick={()=>setPinsOpen(value=>!value)} title={pinsOpen?'收起常用路径':'展开常用路径'}><span className="pins-toggle-label">常用路径</span><b>{overview.pins.length}</b><span className="pins-toggle-caret" aria-hidden="true">{pinsOpen?'⌄':'⌃'}</span></button>{pinsOpen&&<div className="pins-list" id="sidebar-pin-list" role="list">{overview.pins.map(item=>{const name=displayPath(item.path).split(/[\\/]/).filter(Boolean).at(-1)??item.path;return <div key={item.path} role="listitem" className={`pins-row${folder===item.path?' selected':''}`} onContextMenu={event=>{event.preventDefault();event.stopPropagation();setContext({x:event.clientX,y:event.clientY,folder:item.path});}}><span className="pin-dot" data-color={item.color??'none'} aria-hidden="true"/><button className="pins-select" title={displayPath(item.path)} aria-current={folder===item.path?'location':undefined} onClick={()=>{setSidebarPanel(null);selectFolder(item.path);}}>{name}</button></div>;})}{!overview.pins.length&&<p className="pins-empty">右击目录树中的文件夹，设为常用后可标颜色。</p>}</div>}</div>
       {playerView.placement==='sidebar'&&<section className="sidebar-player" aria-label="波形预览">{playerPanel}</section>}
       <p className="sidebar-footnote">拖入文件夹添加 · 原路径引用</p>
      </aside>
      {!floating&&<ResizeHandle label="侧栏宽度" value={layout.sidebar} onChange={value=>resize('sidebar',value)}/>}
     </>}
   </SidebarDockFrame>
   <main className="browser">
     <div className="library-strip"><div><h1>{tagIndex?'标签':favoritesOnly?(favoriteUngrouped?'收藏 · 未分组':favoriteGroup?`收藏 · ${favoriteGroup}`:'收藏'):historyOnly?'试听历史':tag?`标签 · ${tag}`:playlistId?overview.playlists.find(item=>item.id===playlistId)?.name??'播放列表':debouncedQuery&&searchScope==='global'?'全局搜索':folderInfo?.name??pageNames[library]}</h1><span>{tagIndex?`${overview.tags.length} 个标签`:`${page.total} 条`}</span>{hideUnavailable&&!tagIndex&&<span className="hide-unavailable-badge" title="当前列表已排除文件缺失、不可访问、已变化和读取失败的素材">仅显示可用素材</span>}{folder&&<button title={displayPath(folder)} onClick={()=>selectFolder(null)}>返回全部位置</button>}</div><div className="library-filters"><select aria-label="格式筛选" value={format} onChange={event=>{setFormat(event.target.value);setOffset(0);}}>{['全部格式','WAV','AIFF','FLAC','MP3','M4A'].map(item=><option key={item}>{item}</option>)}</select><select aria-label="排序方式" value={safeSort(sort).field} onChange={event=>{setSort(event.target.value);setOffset(0);}}><option value="name">名称排序</option><option value="duration">时长排序</option><option value="rate">采样率排序</option></select><button title="刷新当前素材列表" disabled={!native||busy} onClick={()=>void action(refresh)}>刷新</button></div></div>
     {tagIndex&&<TagsIndex tags={overview.tags} active={null} busy={busy} onPick={name=>{if(!name)return;setTagIndex(false);setTag(name);setOffset(0);setSelected(null);}}/>}
     {favoritesOnly&&<FavoriteChips groups={overview.favorite_groups} active={favoriteGroup} ungrouped={favoriteUngrouped} onPick={selectFavoriteGroup}/>}
     {library==='music'&&<MusicFilters native={native} selected={selected} tags={overview.tags} filter={tag} filters={extraTags} duration={duration} templates={preferences.filterTemplates} onFilter={value=>{setTag(value);setTagIndex(false);setOffset(0);}} onFilters={value=>{setExtraTags(value);setOffset(0);}} onDuration={value=>{setDuration(value);setOffset(0);}} onChanged={contextChanged}/>}{!tagIndex&&<><div className="selection-tools"><button disabled={selectedIds.length!==1} onClick={()=>openTool('ucs')}>UCS建议</button><button disabled={!selectedIds.length} aria-haspopup="dialog" title="把所选素材放入收藏分组；设分组会同时加入收藏" onClick={event=>{const rect=event.currentTarget.getBoundingClientRect();setGroupMenu({x:rect.right,y:rect.bottom,ids:selectedIds});}}>收藏分组</button><button disabled={!selectedIds.length} onClick={()=>openTool('convert')}>导出 / 转换</button><button className={`hide-unavailable${hideUnavailable?' is-on':''}`} aria-pressed={hideUnavailable} title="开启后与搜索关键词、格式、收藏等条件一起生效，只列出状态为可用的素材" onClick={()=>{setHideUnavailable(value=>!value);setOffset(0);}}>{hideUnavailable?'显示不可用素材':'隐藏不可用素材'}</button><CatalogTools selected={selected} ids={selectedIds} native={native} busy={busy} action={action} refresh={refresh} notify={setStatus} onRelocated={()=>{treeStamp.current='';setSelected(null);setSelection([]);setFolder(null);setOffset(0);}}/><button disabled={!native||busy||!selectedIds.length} onClick={()=>void assignOther()}>加入{pageNames[library==='sound'?'music':'sound']}</button></div>
     <div ref={list} onKeyDown={keyNavigate} className="asset-scroll" tabIndex={0} role="region" aria-label={`${pageNames[library]}素材列表`} aria-busy={pageLoading}>
      <table style={{tableLayout:'fixed',minWidth:shownColumns.reduce((sum,column)=>sum+layout.columns[column.widthIndex],0)}}>
       <colgroup>{shownColumns.map(column=><col key={column.id} style={{width:layout.columns[column.widthIndex]}}/>)}</colgroup>
       <thead onContextMenu={event=>{event.preventDefault();event.stopPropagation();setSidebarPanel(null);setPlaylistMenu(null);setContext(null);setColumnMenu({x:event.clientX,y:event.clientY});}} onKeyDown={event=>{if(event.key==='ContextMenu'||event.shiftKey&&event.key==='F10'){event.preventDefault();event.stopPropagation();const rect=(event.target as HTMLElement).getBoundingClientRect();setSidebarPanel(null);setPlaylistMenu(null);setContext(null);setColumnMenu({x:rect.left,y:rect.bottom});}}}>
        <tr>{shownColumns.map(column=>{
         const sortable=column.sortable===true,active=sortable&&sortState.field===column.id;
         const narrow=column.id==='index'||column.id==='favorite'||column.id==='playlist';
         return <th key={column.id} tabIndex={0} title={sortable?'点击切换升/降序；右键设置列显示、隐藏与顺序':'右键设置列显示、隐藏与顺序'} aria-haspopup="dialog" aria-sort={active?(sortState.dir==='asc'?'ascending':'descending'):undefined} className={`${sortable?'is-sortable':''}${narrow?' is-narrow':''}${active?' is-sorted':''}`} onClick={sortable?()=>{setSort(nextSort(sortState,column.id as SortField));setOffset(0);}:undefined}>
          {column.label}{active&&<span className="sort-arrow" aria-hidden="true">{sortState.dir==='asc'?'▲':'▼'}</span>}
          <ResizeHandle label={`${column.label}列宽`} value={layout.columns[column.widthIndex]} onChange={value=>setLayout(old=>safeLayout({...old,columns:old.columns.map((width,index)=>index===column.widthIndex?value:width)}))}/>
         </th>;
        })}</tr>
       </thead>
       <tbody>{rows.map((asset,rowIndex)=><tr id={`asset-${asset.id}`} key={asset.id} onContextMenu={event=>contextAssets(asset,event)} draggable={false} onDragStart={event=>event.preventDefault()} onPointerDown={event=>{if(native&&event.isPrimary&&event.button===0)dragOrigin.current={id:asset.id,x:event.clientX,y:event.clientY,pointer:event.pointerId};}} onPointerMove={event=>dragMove(event,asset)} onPointerUp={()=>{dragOrigin.current=null;}} onPointerCancel={()=>{dragOrigin.current=null;}} onPointerLeave={()=>{dragOrigin.current=null;}} aria-selected={selection.includes(asset.id)} onClick={event=>choose(asset,event)}>{shownColumns.map(column=><td key={column.id} className={column.id==='wave'?'waveform-cell':['duration','rate','bpm','channels','index'].includes(column.id)?'mono':['favorite','playlist'].includes(column.id)?'is-narrow':''}>{renderCell(column.id,asset,rowIndex)}</td>)}</tr>)}</tbody>
      </table>{!rows.length&&<div className="empty-state"><h2>{pageLoading?'正在读取素材…':query||format!=='全部格式'||favoritesOnly||tag||historyOnly||playlistId||extraTags.length||duration.min!==null||duration.max!==null?'没有匹配素材':`添加你的${library==='music'?'音乐':'音效'}素材`}</h2><p>{favoritesOnly||tag||historyOnly||playlistId||extraTags.length||duration.min!==null||duration.max!==null?'当前筛选下没有素材，可切换视图或清除筛选。':library==='music'?'可添加音乐文件夹，或从音效库把已选素材加入音乐库。':'选择文件夹后，只读扫描WAV、AIFF、FLAC、MP3、M4A。'}</p><button disabled={!native||busy||running} onClick={openImport}>添加素材文件夹</button></div>}</div>
     <div className="list-footer"><span>{rows.length} 条可见 / 共 {page.total} 条</span><div className="pager"><button aria-label="上一页" disabled={offset===0||busy||pageLoading} onClick={()=>{setOffset(Math.max(0,offset-PAGE_SIZE));setSelected(null);}}>‹</button><span>{Math.floor(loadedOffset/PAGE_SIZE)+1} / {Math.max(1,Math.ceil(page.total/PAGE_SIZE))}</span><button aria-label="下一页" disabled={offset+PAGE_SIZE>=page.total||busy||pageLoading} onClick={()=>{setOffset(offset+PAGE_SIZE);setSelected(null);}}>›</button></div></div></>}
   </main>
   {layout.details?<><ResizeHandle label="详情宽度" value={layout.inspector} reverse onChange={value=>resize('inspector',value)}/><aside className="inspector">{selected?<AssetDetail key={selected.id} asset={selected} library={library} groups={overview.favorite_groups.map(item=>item.name)} onCollapse={()=>toggle('details')} onChanged={patch=>{setSelected(old=>old?.id===selected.id?{...old,...patch}:old);void action(refresh);}}/>:<div className="inspector-empty"><div className="panel-heading"><span>素材详情</span><button onClick={()=>toggle('details')} aria-label="折叠右侧详情栏">›</button></div><div className="panel-empty-state"><span className="panel-empty-icon" aria-hidden="true"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round"><path d="M3 12h2m3-5v10m4-14v18m4-13v8m4-5v2"/></svg></span><strong>未选择素材</strong><p>在列表中点选一条素材，这里会显示文件信息、收藏与分组、标签。</p></div></div>}</aside></>:<aside className="panel-rail right-rail"><button onClick={()=>toggle('details')} aria-label="展开右侧详情栏">‹</button><span>详情</span></aside>}
  </div>
  <TaskDrawer jobs={jobs} hide={new Set(hiddenJobs)} open={jobsOpen} busy={busy} onClose={()=>setJobsOpen(false)}
   onCancel={id=>void action(async()=>{await invoke('catalog_cancel',{id});setStatus('已请求取消扫描');notify('warning','已请求取消扫描','正在处理的文件会跑完再停。');await refresh();})}
   onResume={job=>void action(async()=>{const recovering=resumable(job);const id=await invoke<string>(recovering?'catalog_resume':'catalog_start',recovering?{id:job.id}:{root:job.root,library:job.library??'sound'});setTrackedId(id);setStatus('扫描已启动');await refresh();})}
   onReveal={path=>void invoke('reveal_folder',{path}).catch(e=>notify('error','无法打开目录',String(e)))}
   onClearDone={()=>setHiddenJobs(old=>[...old,...jobs.filter(job=>!['running','queued','cancelling'].includes(job.status)).map(job=>job.id)])}/>
  {playerView.placement==='bottom'&&<>
   {!layout.bottomCollapsed&&<ResizeHandle label="波形高度" value={layout.wave} axis="y" reverse onChange={value=>resize('wave',value)}/>}
   {playerPanel}
  </>}
  {playerView.placement==='floating'&&<DockFrame dock={playerView.dock} onChange={setPlayerDock} className="player-dock" edge="bottom" dockedSize={playerSnapSize} snapLabel="松手吸附回底部">{()=>playerPanel}</DockFrame>}
  <footer className="statusbar"><span role="status" aria-live="polite">{liveStatus}</span><span className="status-hint">Space 播放 / 暂停 · 波形上拖动选择片段，单击定位</span><span className="status-version" title="当前构建版本，与 EXE 文件属性里的产品版本一致">音效管理 | v{appVersion||'—'}</span><TaskChip jobs={jobs} hide={new Set(hiddenJobs)} open={jobsOpen} onToggle={()=>setJobsOpen(value=>!value)}/></footer>
  <dialog ref={dialog} onCancel={event=>{if(busy)event.preventDefault();else setOpen(false);}} onClose={()=>setOpen(false)} aria-labelledby="catalog-dialog-title"><div className="dialog-heading"><h2 id="catalog-dialog-title">添加至{pageNames[importLibrary]}</h2><button disabled={busy} aria-label="关闭添加素材" onClick={()=>setOpen(false)}>×</button></div><p>选择文件夹、填写原路径或将文件夹拖入窗口。音频仍保存在原位置。</p><label className="import-label">素材文件夹<input value={root} onChange={event=>{setRoot(event.target.value);setConfirmedRoot('');}} disabled={busy} placeholder="D:\声音素材"/></label><button className="choose-folder" disabled={busy||!native} onClick={()=>void action(async()=>{const chosen=await invoke<string|null>('catalog_pick_folder',{initial:root||history[0]||null});if(chosen)await validatePath(chosen);})}>选择文件夹…</button>{history.length>0&&<label className="import-label">最近使用<select aria-label="最近使用的素材路径" value="" disabled={busy} onChange={event=>{if(event.target.value){setRoot(displayPath(event.target.value));setConfirmedRoot('');}}}><option value="">选择记住的路径…</option>{history.map(path=><option key={path} value={path}>{displayPath(path)}</option>)}</select></label>}{confirmedRoot&&<div className="import-preview"><strong>目录可读取</strong><p className="job-path">{displayPath(confirmedRoot)}</p><p>将登记至{pageNames[importLibrary]}，不会复制源音频。</p></div>}{error&&<p className="asset-error">{error}</p>}<div className="dialog-actions"><button disabled={!root.trim()||busy} onClick={()=>void action(()=>validatePath(root))}>{busy?'处理中…':'检查目录'}</button><button className="primary" disabled={!confirmedRoot||busy||running} onClick={()=>void start()}>开始添加</button></div></dialog>
  {columnMenu&&<ColumnSettings x={columnMenu.x} y={columnMenu.y} layout={layout} library={library} onChange={setLayout} onClose={()=>setColumnMenu(null)}/>}
  {playlistMenu&&<PlaylistPicker x={playlistMenu.x} y={playlistMenu.y} ids={playlistMenu.ids} library={library} playlists={overview.playlists} onClose={()=>setPlaylistMenu(null)} onDone={()=>{treeStamp.current='';void refresh();}} notify={setStatus}/>}
  {groupMenu&&<FavoriteGroupPicker x={groupMenu.x} y={groupMenu.y} ids={groupMenu.ids} groups={overview.favorite_groups} onClose={()=>setGroupMenu(null)} onDone={()=>{treeStamp.current='';void refresh();}} notify={setStatus}/>}
  {sidebarPanel&&<SidebarQuickPanel key={sidebarPanel.kind} panel={sidebarPanel} overview={overview} onClose={()=>setSidebarPanel(null)} onSelect={(kind,id)=>{setSidebarPanel(null);if(kind==='favorites'){if(id==='__all__')showFavorites();else if(id==='__ungrouped__')selectFavoriteGroup(null,true);else selectFavoriteGroup(id,false);}else showCollection(kind==='tags'?'tag':'playlist',id);}}/>}
  <CatalogContext target={context} library={library} overview={overview} onClose={()=>setContext(null)} onChanged={contextChanged} notify={setStatus} onFavoriteGroup={(assets,x,y)=>setGroupMenu({x:x+14,y:y+14,ids:assets.map(asset=>asset.id)})} onLayer={(assets,scatter)=>{try{const result=addToLayer(assets,scatter);setStatus(`已添加 ${result.added} 个叠层片段，跳过 ${result.skipped} 个（需可用且时长已知）`);openTool('layer');}catch(e){setError(String(e));}}}/>
  <Settings open={settingsOpen} onClose={()=>setSettingsOpen(false)} value={preferences} onChange={setPreferences} onResetLayout={()=>setLayout(defaultLayout)} library={library} folder={folder} native={native} onOpenTool={tool=>{setSettingsOpen(false);openTool(tool);}}/>
  <ExtSetup open={extSetupOpen} onClose={()=>setExtSetupOpen(false)} native={native}/>
 <ToolsWindow open={managerOpen} onClose={()=>setManagerOpen(false)} native={native} ids={selectedIds} selected={selected} library={library} tool={activeTool} onTool={setActiveTool}
   onPick={asset=>choose(asset)} onChanged={contextChanged} onCheck={()=>void checkMissing()} onJobs={()=>{setManagerOpen(false);setJobsOpen(true);}} onAccount={()=>{setManagerOpen(false);setAccountOpen(true);}} onExtSetup={()=>{setManagerOpen(false);setExtSetupOpen(true);}} busy={busy||running}/>
  <AccountWindow native={native} open={accountOpen} onClose={()=>setAccountOpen(false)}/>
  <PluginWorkspace native={native} open={pluginOpen} onClose={()=>setPluginOpen(false)} flags={{sfx_aaf_import:preferences.pluginAafImport,sfx_pt_overlay:preferences.pluginPtOverlay,sfx_pt_bouncer:preferences.pluginPtBouncer}}/>
  <NoticeStack/>
 </div>;
}
installCrashHandlers();
createRoot(document.getElementById('root')!).render(<React.StrictMode><ErrorBoundary><Catalog/></ErrorBoundary></React.StrictMode>);
