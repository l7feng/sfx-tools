import React,{useEffect,useMemo,useRef,useState} from 'react';
import {createRoot} from 'react-dom/client';
import {assets,filterAssets,timeLabel} from './demo-data';
import type {Asset} from './demo-data';
import './theme.css';
import './workbench.css';

function Icon({name,size=18}:{name:string;size?:number}) {
 const paths:Record<string,React.ReactNode>={
 search:<><circle cx="10" cy="10" r="6"/><path d="m15 15 5 5"/></>,
 library:<><rect x="3" y="4" width="5" height="16" rx="1"/><path d="M12 4v16M16 4l4 16"/></>,
 folder:<path d="M3 6h6l2 3h10v11H3z"/>,star:<path d="m12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2-5.6-3-5.6 3 1.1-6.2L3 9.6l6.2-.9z"/>,
 wave:<path d="M3 10v4m4-7v10m5-14v22m5-18v10m4-7v4"/>,
 play:<path d="m8 4 12 8-12 8z"/>,pause:<path d="M8 4v16M16 4v16"/>,
 back:<path d="M5 5v14m14-14L7 12l12 7z"/>,
 loop:<path d="M4 9a7 7 0 0 1 12-4l3 3M19 3v5h-5M20 15a7 7 0 0 1-12 4l-3-3M5 21v-5h5"/>,
 volume:<path d="M3 9h4l5-5v16l-5-5H3zM16 8q6 4 0 8"/>,
 panel:<><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M15 4v16"/></>,
 plus:<path d="M12 5v14M5 12h14"/>,close:<path d="m6 6 12 12M18 6 6 18"/>,
 arrow:<path d="M5 12h14m-6-6 6 6-6 6"/>,help:<><circle cx="12" cy="12" r="9"/><path d="M9 8a3 3 0 0 1 6 1c0 2-3 2-3 5m0 3v.1"/></>,
 };
 return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{paths[name]||paths.wave}</svg>;
}
function Wave({seed,large=false,progress=0}:{seed:number;large?:boolean;progress?:number}){
 const count=large?210:32;
 return <svg className={large?'waveform':'mini-wave'} viewBox={`0 0 ${count*4} 100`} preserveAspectRatio="none" aria-hidden="true">{Array.from({length:count},(_,i)=>{
 const height=8+Math.abs(Math.sin(i*1.73+seed)*Math.cos(i*.17+seed))*(large?78:65)*(0.35+0.65*Math.sin(Math.PI*(i+1)/(count+1)));
 return <rect key={i} x={i*4} y={50-height/2} width="2" height={height} rx="1" className={i/count<progress?'played':''}/>;
 })}</svg>;
}
function App(){
 const [scope,setScope]=useState('全部素材'),[query,setQuery]=useState(''),[format,setFormat]=useState('全部格式'),[sort,setSort]=useState('默认顺序');
 const [favorites,setFavorites]=useState(new Set([1,5,9])),[selected,setSelected]=useState(1),[details,setDetails]=useState(true);
 const [playing,setPlaying]=useState(false),[position,setPosition]=useState(0),[loop,setLoop]=useState(false),[volume,setVolume]=useState(65);
 const [notice,setNotice]=useState('就绪 · 选择素材，预览试听流程'),[logs,setLogs]=useState<string[]>([]),[logOpen,setLogOpen]=useState(false);
 const [dialog,setDialog]=useState<'import'|'help'|null>(null),[importChoice,setImportChoice]=useState(''),[preview,setPreview]=useState(false);
 const dialogRef=useRef<HTMLDialogElement>(null),searchRef=useRef<HTMLInputElement>(null),tableRef=useRef<HTMLDivElement>(null);
 const current=assets.find(a=>a.id===selected)!;
 const filtered=useMemo(()=>{
 const result=filterAssets(assets,query,scope,format,favorites);
 return sort==='时长升序'?result.sort((a,b)=>a.duration-b.duration):sort==='名称排序'?result.sort((a,b)=>a.name.localeCompare(b.name,'zh-CN')):result;
 },[query,scope,format,favorites,sort]);
 const report=(message:string)=>{setNotice(message);setLogs(old=>[message,...old].slice(0,20));};
 const select=(a:Asset)=>{setSelected(a.id);setPosition(0);setPlaying(false);};
 const toggleFavorite=(id:number)=>{setFavorites(old=>{const next=new Set(old);next.has(id)?next.delete(id):next.add(id);return next;});};
 const togglePlay=()=>{setPlaying(v=>!v);if(position>=current.duration)setPosition(0);};
 useEffect(()=>{document.title='SFX TOOLS · S1.1 界面样稿';},[]);
 useEffect(()=>{if(dialog)dialogRef.current?.showModal();else dialogRef.current?.close();},[dialog]);
 useEffect(()=>{if(!playing)return;const timer=window.setInterval(()=>setPosition(p=>p+.1>=current.duration?(loop?0:current.duration):p+.1),100);return()=>clearInterval(timer);},[playing,current.duration,loop]);
 useEffect(()=>{if(playing&&!loop&&position>=current.duration)setPlaying(false);},[position,playing,loop,current.duration]);
 useEffect(()=>{
 const handler=(e:KeyboardEvent)=>{
 if(e.key==='Escape'){setDialog(null);return;}
 if(dialog)return;
 if((e.ctrlKey||e.metaKey)&&e.key.toLowerCase()==='k'){e.preventDefault();searchRef.current?.focus();return;}
 if((e.target as HTMLElement).closest('input,textarea,select,summary,a'))return;
 if((e.key==='ArrowDown'||e.key==='ArrowUp')&&tableRef.current?.contains(e.target as Node)){
 e.preventDefault();const i=filtered.findIndex(a=>a.id===selected);const next=filtered[Math.max(0,Math.min(filtered.length-1,i+(e.key==='ArrowDown'?1:-1)))];if(next)select(next);
 return;
 }
 if((e.target as HTMLElement).closest('button'))return;
 if(e.code==='Space'){e.preventDefault();togglePlay();}
 };window.addEventListener('keydown',handler);return()=>window.removeEventListener('keydown',handler);
 });
 useEffect(()=>{tableRef.current?.querySelector('[aria-selected="true"]')?.scrollIntoView({block:'nearest'});},[selected]);
 const categories=['自然环境','城市生活','拟音动作','机械装置','设计音效'];
 return <div className="app-shell">
 <header className="topbar"><div className="brand"><span className="brand-icon"><Icon name="wave" size={24}/></span><strong>SFX<span> TOOLS</span></strong></div><div className="top-divider"/><span className="workspace-title">声音素材工作台</span><span className="prototype">S1.1 · 交互样稿</span><div className="top-right"><a href="?catalog">素材目录</a><span className="demo-label">模拟素材 · 无实际音频输出</span><button aria-label="使用帮助" onClick={()=>setDialog('help')}><Icon name="help"/></button></div></header>
 <div className="workspace">
 <aside className="sidebar"><div className="sidebar-label">工作空间 <span>LOCAL</span></div><nav aria-label="素材范围">{['全部素材','我的收藏'].map((s,i)=><button key={s} className={'nav-item '+(scope===s?'active':'')} aria-current={scope===s?'page':undefined} onClick={()=>setScope(s)}><Icon name={i?'star':'library'}/><span>{s}</span><b>{i?favorites.size:assets.length}</b></button>)}<div className="sidebar-label library-label">素材分类 <span>05</span></div>{categories.map(c=><button key={c} className={'nav-item '+(scope===c?'active':'')} aria-current={scope===c?'page':undefined} onClick={()=>setScope(c)}><Icon name="folder"/><span>{c}</span><b>{assets.filter(a=>a.category===c).length}</b></button>)}</nav>
 <div className="sidebar-bottom"><div className="library-note"><span className="online-dot"/> 本地素材库<p>12 条演示素材<br/>仅演示当前窗口的操作</p></div><button className="add-library" onClick={()=>{setImportChoice('');setPreview(false);setDialog('import');}}><Icon name="plus"/> 添加素材库</button><button className="help-link" onClick={()=>setDialog('help')}>键盘快捷键 <kbd>?</kbd></button></div></aside>
 <main className={'browser '+(!details?'wide':'')}><div className="browser-heading"><div><div className="eyebrow">SOUND LIBRARY / 素材浏览</div><h1>{scope}<span>{filtered.length.toString().padStart(2,'0')}</span></h1></div><button className="panel-toggle" aria-label={details?'收起详情面板':'展开详情面板'} aria-pressed={details} onClick={()=>setDetails(v=>!v)}><Icon name="panel"/></button></div>
 <div className="search-box"><Icon name="search"/><input ref={searchRef} aria-label="搜索素材" placeholder="搜索名称、文件名或标签…" value={query} onChange={e=>setQuery(e.target.value)}/>{query?<button aria-label="清除搜索" onClick={()=>setQuery('')}><Icon name="close" size={16}/></button>:<kbd>Ctrl K</kbd>}</div>
 <div className="filterbar"><label><span>格式</span><select aria-label="格式筛选" value={format} onChange={e=>setFormat(e.target.value)}>{['全部格式','WAV','AIFF','FLAC','MP3'].map(f=><option key={f}>{f}</option>)}</select></label><span className="filter-hint">{query?`“${query}” 的搜索结果`:'所有声音，各就其位。'}</span><select aria-label="素材排序" value={sort} onChange={e=>setSort(e.target.value)}>{['默认顺序','时长升序','名称排序'].map(s=><option key={s}>{s}</option>)}</select></div>
 <div className="asset-scroll" ref={tableRef} tabIndex={0} role="region" aria-label="素材列表，方向键切换选中素材"><table><thead><tr><th className="favorite-col">收藏</th><th>素材名称</th><th className="wave-col">波形</th><th>时长</th><th className="rate-col">采样率</th><th>格式</th></tr></thead><tbody>{filtered.map(a=><tr key={a.id} aria-selected={selected===a.id} onClick={()=>select(a)} onDoubleClick={()=>{select(a);setPlaying(true);}}><td><button className={'favorite '+(favorites.has(a.id)?'is-favorite':'')} aria-label={`${favorites.has(a.id)?'取消收藏':'收藏'} ${a.name}`} aria-pressed={favorites.has(a.id)} onClick={e=>{e.stopPropagation();toggleFavorite(a.id);}}><Icon name="star" size={16}/></button></td><td><button className="asset-name" onClick={e=>{e.stopPropagation();select(a);}}><strong>{a.name}</strong><small>{a.file}</small></button></td><td className="wave-col"><Wave seed={a.id}/></td><td className="mono">{timeLabel(a.duration)}</td><td className="mono rate-col">{a.rate} kHz</td><td><span className="format-badge">{a.format}</span></td></tr>)}</tbody></table>{!filtered.length&&<div className="empty-state"><Icon name="search" size={34}/><h2>没有找到匹配的声音</h2><p>试试其他关键词，或清除当前筛选。</p><button onClick={()=>{setQuery('');setFormat('全部格式');setScope('全部素材');}}>查看全部素材</button></div>}</div>
 <div className="list-footer"><span>{filtered.length} 条素材 <span className="muted">/ 演示集合</span></span><span>↑ ↓ 切换素材 · Space 模拟播放</span></div></main>
 {details&&<aside className="inspector"><div className="inspector-title">素材详情<span>INSPECTOR</span></div><div className="asset-art"><div className="art-grid"/><Icon name="wave" size={56}/><span>{current.category} / {current.format}</span></div><div className="inspector-content"><div className="detail-category">{current.category}</div><h2>{current.name}</h2><p className="description">{current.description}</p><dl><div><dt>时长</dt><dd>{timeLabel(current.duration)}</dd></div><div><dt>采样率</dt><dd>{current.rate} kHz</dd></div><div><dt>声道</dt><dd>{current.channels===2?'Stereo / 立体声':'Mono / 单声道'}</dd></div><div><dt>格式</dt><dd>{current.format}</dd></div></dl><div className="detail-section"><h3>标签</h3><div className="tags">{current.tags.map(tag=><button key={tag} onClick={()=>{setQuery(tag);setScope('全部素材');setFormat('全部格式');}}>{tag}</button>)}</div></div><details className="file-details"><summary>文件信息</summary><p>{current.file}</p><p>演示目录 / {current.category}<br/>样稿未连接真实文件</p></details><button className={'detail-favorite '+(favorites.has(current.id)?'is-favorite':'')} onClick={()=>toggleFavorite(current.id)}><Icon name="star" size={16}/>{favorites.has(current.id)?'已加入收藏':'加入收藏'}</button></div></aside>}
 </div>
 <section className="player" aria-label="模拟播放器"><div className="player-heading"><div><span className="player-status">{playing?'模拟播放中':'试听预览'}</span><strong>{current.name}</strong></div><span className="player-spec">{current.rate} kHz · {current.channels===2?'STEREO':'MONO'} <span> / 示意波形</span></span></div><div className="timeline"><div className="ruler">{Array.from({length:7},(_,i)=><span key={i}>{timeLabel(current.duration*i/6)}</span>)}</div><div className="wave-area"><Wave seed={current.id} large progress={position/current.duration}/><div className="playhead" style={{left:`${position/current.duration*100}%`}}/><input type="range" className="seek" min="0" max={current.duration} step="0.1" value={position} aria-label="模拟播放位置" aria-valuetext={timeLabel(position)} onChange={e=>setPosition(Number(e.target.value))}/></div></div><div className="transport"><div className="transport-buttons"><button aria-label="回到开头" onClick={()=>setPosition(0)}><Icon name="back"/></button><button className="play-button" aria-label={playing?'暂停模拟播放':'开始模拟播放'} onClick={togglePlay}><Icon name={playing?'pause':'play'} size={20}/></button><button className={loop?'enabled':''} aria-label="循环模拟播放" aria-pressed={loop} onClick={()=>setLoop(v=>!v)}><Icon name="loop"/></button><span className="time-readout">{timeLabel(position)}<small> / {timeLabel(current.duration)}</small></span></div><span className="simulation-note">播放进度仅为交互演示，不发声</span><label className="volume"><Icon name="volume"/><input type="range" aria-label="模拟音量" value={volume} onChange={e=>setVolume(Number(e.target.value))}/><span>{volume}%</span></label></div></section>
 <footer className="statusbar"><span role="status" aria-live="polite"><span className="online-dot"/>{notice}</span><button aria-expanded={logOpen} onClick={()=>setLogOpen(v=>!v)}>操作记录 {logs.length} {logOpen?'⌄':'⌃'}</button><span className="version">SFX TOOLS · PREVIEW</span></footer>
 {logOpen&&<section className="log-panel" aria-label="操作记录"><div><strong>本次窗口操作记录</strong><button aria-label="关闭记录" onClick={()=>setLogOpen(false)}><Icon name="close"/></button></div>{logs.length?logs.map((log,i)=><p key={i}>{log}</p>):<p>暂无任务记录。当前筛选与收藏只保留在本窗口。</p>}</section>}
 <dialog ref={dialogRef} onCancel={()=>setDialog(null)} onClose={()=>setDialog(null)} aria-labelledby="dialog-title"><div className="dialog-heading"><h2 id="dialog-title">{dialog==='import'?'添加素材库':'工作台使用指南'}</h2><button aria-label="关闭对话框" onClick={()=>setDialog(null)}><Icon name="close"/></button></div>{dialog==='import'?<><p>预览“选择目录 → 检查内容 → 加入素材库”的流程。此样稿不会读取磁盘或导入文件。</p><label className="import-label">演示素材目录<select value={importChoice} onChange={e=>{setImportChoice(e.target.value);setPreview(false);}}><option value="">选择一个演示目录</option><option>演示目录 / 自然环境</option><option>演示目录 / 拟音动作</option></select></label>{preview&&<div className="import-preview"><strong>目录预览已准备</strong><p>{importChoice} · {importChoice.includes('自然')?4:2} 条现有演示素材</p><p>仅展示流程，不产生重复素材或真实磁盘写入。</p></div>}<div className="dialog-actions"><button disabled={!importChoice} onClick={()=>setPreview(true)}>预览内容</button><button className="primary" disabled={!preview} onClick={()=>{setScope(importChoice.includes('自然')?'自然环境':'拟音动作');setQuery('');setFormat('全部格式');report('演示完成 · 已展示所选分类，未导入真实文件');setDialog(null);}}>完成演示 <Icon name="arrow" size={16}/></button></div></>:<><p>按找声音的顺序组织：选择分类 → 搜索 → 查看详情 → 试听预览。</p><dl className="shortcuts"><div><dt>聚焦搜索</dt><dd>Ctrl K</dd></div><div><dt>列表中切换素材</dt><dd>↑ / ↓</dd></div><div><dt>非输入控件上模拟播放 / 暂停</dt><dd>Space</dd></div><div><dt>关闭弹窗</dt><dd>Esc</dd></div></dl><p>当前 12 条素材、波形和播放进度均为演示数据。收藏与筛选刷新后重置。真实导入、波形、播放和拖出将在后续阶段接入。</p><a href="?probe">打开 S1.0 原生验证窗口 →</a></>}</dialog>
 </div>;
}
createRoot(document.getElementById('root')!).render(<React.StrictMode><App/></React.StrictMode>);
