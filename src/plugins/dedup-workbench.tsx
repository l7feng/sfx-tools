import React,{useCallback,useEffect,useMemo,useRef,useState} from 'react';
import {createRoot} from 'react-dom/client';
import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {getCurrentWindow} from '@tauri-apps/api/window';
import {formatBytes} from '../catalog-client';
import './dedup-workbench.css';

/* 「重复组工作台」独立窗口：只读复刻去重工具的浏览面。
   三条硬规则：
   1) 这里没有任何写入口——不隔离、不删除、不改文件；处置仍在 Audio Dedup 自己的界面里逐次确认；
   2) 波形与试听走 dedup_waveform / dedup_play，后端按 groups.json 白名单校验路径，界面传不了任意路径；
   3) 窗口参数由 Rust 用 initialization_script 注入 window.__DEDUP_ARG，避免 URL 编码差异。 */

type DedupFile={path:string;size:number;keep:boolean;grade:string};
type DedupGroup={id:string;layer:string;grade:string;count:number;keep:string;reclaimable_bytes:number;files:DedupFile[]};
type DedupSummary={schema:number;generated_at:string;root:string;workdir:string;l1_groups:number;l2_groups:number;l3_groups:number;edges:number;total_groups:number;reclaimable_bytes:number;scanned_files:number;errors:string[];groups:DedupGroup[];truncated:boolean};
type Waveform={peaks:number[];duration:number|null;analyzed_duration:number;truncated:boolean;channels:number;channel_peaks:number[][]};
type Snapshot={id:string|null;state:string;position:number;duration:number|null;volume:number;looping:boolean;error:string|null};

const MAX_LANES=5;

function arg(){const injected=(window as unknown as {__DEDUP_ARG?:{workdir:string;group:string}}).__DEDUP_ARG;if(injected)return injected;const q=new URLSearchParams(location.search);return {workdir:q.get('workdir')??'',group:q.get('group')??''};}

function baseName(path:string){const parts=path.split(/[\\/]/);return parts[parts.length-1]??path;}
function dirName(path:string){const parts=path.split(/[\\/]/);parts.pop();return parts.slice(-2).join('/');}

function clock(seconds:number|null|undefined){
  if(seconds==null||!isFinite(seconds)||seconds<0)return '—';
  const m=Math.floor(seconds/60),s=seconds-m*60;
  return `${m}:${s.toFixed(2).padStart(5,'0')}`;
}

/** 峰值 → 居中对折的折线：只画包络，不做插值，避免看错瞬态。 */
function peakPath(peaks:number[],width=900,height=48){
  if(!peaks.length)return '';
  const mid=height/2;const scale=mid-1;
  const step=width/Math.max(1,peaks.length-1);
  const top:string[]=[];const bottom:string[]=[];
  for(let i=0;i<peaks.length;i++){
    const x=i*step;top.push(`${x.toFixed(1)},${(mid-peaks[i]*scale).toFixed(1)}`);
  }
  for(let i=peaks.length-1;i>=0;i--){
    const x=i*step;bottom.push(`${x.toFixed(1)},${(mid+peaks[i]*scale).toFixed(1)}`);
  }
  return `M${top.join(' L')} L${bottom.join(' L')} Z`;
}

function Detail(){
  const start=useMemo(arg,[]);
  const [workdir,setWorkdir]=useState(start.workdir);
  const [summary,setSummary]=useState<DedupSummary|null>(null);
  const [error,setError]=useState('');
  const [hint,setHint]=useState('');
  const [filter,setFilter]=useState<'all'|'L1'|'L2'|'L3'>('all');
  const [query,setQuery]=useState('');
  const [group,setGroup]=useState(start.group);
  const [waves,setWaves]=useState<Record<string,Waveform|'loading'|'fail'>>({});
  const [snap,setSnap]=useState<Snapshot|null>(null);
  const [busyWave,setBusyWave]=useState(0);
  const waveCache=useRef<Record<string,Waveform>>({});

  const load=useCallback(async(dir:string)=>{
    try{
      const data=await invoke<DedupSummary>('dedup_groups',{workdir:dir||null});
      setSummary(data);setError('');
      if(dir)setWorkdir(dir);
      setGroup(current=>current||data.groups[0]?.id||'');
    }catch(e){setError(String(e))}
  },[]);

  useEffect(()=>{void load(start.workdir)},[load,start.workdir]);

  // 主窗口再次点其它组时，同一窗口换选中项即可，不重复开窗。
  useEffect(()=>{
    let dispose:(()=>void)|undefined;
    void listen<{workdir:string;group:string}>('dedup-workbench-select',event=>{
      if(event.payload.workdir)setWorkdir(event.payload.workdir);
      if(event.payload.group)setGroup(event.payload.group);
    }).then(fn=>{dispose=fn});
    return ()=>{dispose?.()};
  },[]);

  // 播放位置轮询：只在播放中刷新，停下来就不再占用 IPC。
  useEffect(()=>{
    if(!snap||snap.state!=='playing')return;
    const timer=window.setInterval(()=>{void invoke<Snapshot>('player_status').then(setSnap).catch(()=>{})},120);
    return ()=>window.clearInterval(timer);
  },[snap?.state]);

  const active=summary?.groups.find(item=>item.id===group)??null;

  // 只对当前组的头 5 个文件取波形；同一路径一生只算一次。
  useEffect(()=>{
    if(!active)return;
    let alive=true;
    const files=active.files.slice(0,MAX_LANES);
    const missing=files.filter(file=>!waveCache.current[file.path]);
    if(missing.length===0)return;
    setBusyWave(missing.length);
    (async()=>{
      for(const file of missing){
        if(!alive)return;
        setWaves(prev=>({...prev,[file.path]:'loading'}));
        try{
          const wave=await invoke<Waveform>('dedup_waveform',{workdir,path:file.path,simple:false});
          waveCache.current[file.path]=wave;
          if(alive)setWaves(prev=>({...prev,[file.path]:wave}));
        }catch(e){
          if(alive)setWaves(prev=>({...prev,[file.path]:'fail'}));
        }finally{
          if(alive)setBusyWave(n=>Math.max(0,n-1));
        }
      }
    })();
    return ()=>{alive=false};
  },[active?.id,workdir]);

  const filtered=useMemo(()=>{
    if(!summary)return [];
    return summary.groups.filter(item=>{
      if(filter!=='all'&&item.layer!==filter)return false;
      if(!query.trim())return true;
      const needle=query.trim().toLowerCase();
      return item.id.toLowerCase().includes(needle)
        || baseName(item.keep||'').toLowerCase().includes(needle)
        || item.files.some(file=>baseName(file.path).toLowerCase().includes(needle));
    });
  }, [summary,filter,query]);

  const audition=async(file:DedupFile,position=0)=>{
    try{
      const next=await invoke<Snapshot>('dedup_play',{workdir,path:file.path,position});
      setSnap(next);
    }catch(e){setHint(String(e))}
  };

  const toggle=async()=>{
    try{
      const action=snap?.state==='playing'?'pause':'resume';
      const next=await invoke<Snapshot>('player_control',{action,value:null});
      setSnap(next);
    }catch(e){setHint(String(e))}
  };

  const copyList=async()=>{
    if(!active)return;
    const text=active.files.map(file=>`${file.keep?'KEEP':'    '}\t${file.grade}\t${file.path}`).join('\r\n');
    try{
      await navigator.clipboard.writeText(text);
      setHint(`已复制 ${active.files.length} 行清单；处置请到 Audio Dedup 界面逐次确认`);
    }catch{setHint('复制失败：窗口未获得剪贴板权限，请手动选中表格复制')}
  };

  const playhead=(duration:number|null|undefined)=>{
    if(!snap||snap.state==='idle'||!duration||duration<=0)return null;
    return Math.max(0,Math.min(1,snap.position/duration));
  };
  // 还没开始试听时，时长栏先用首条文件的波形时长顶上，避免整页都是「—」。
  const firstWave=active?waves[active.files[0]?.path]:undefined;
  const fallbackDuration=firstWave&&firstWave!=='loading'&&firstWave!=='fail'?firstWave.duration:null;

  return <div className="dw-root">
    <header className="dw-head">
      <span className="dw-title">重复组工作台</span>
      {active&&<span className={`dw-tag lay-${active.layer}`}>{active.layer}</span>}
      <span className="dw-ro">只读 · 无删除入口</span>
      {summary&&<span className="dw-stamp">groups.json {summary.generated_at||'时间未知'}</span>}
      <div className="dw-head-actions">
        <button onClick={()=>void copyList()} disabled={!active}>复制本组清单</button>
        <button onClick={()=>void getCurrentWindow().close()}>关闭 ×</button>
      </div>
    </header>

    {error&&<div className="dw-error">{error}</div>}

    <div className="dw-body">
      <aside className="dw-left">
        <div className="dw-search">
          <input value={query} onChange={e=>setQuery(e.target.value)} placeholder="搜索组 ID 或文件名…"/>
          <span>{filtered.length}</span>
        </div>
        <div className="dw-chips">
          {([['all','全部'],['L1','L1'],['L2','L2'],['L3','L3']] as const).map(([key,label])=>{
            const total=summary?key==='all'?summary.groups.length:(key==='L1'?summary.l1_groups:key==='L2'?summary.l2_groups:summary.l3_groups):0;
            return <button key={key} className={filter===key?'is-on':''} onClick={()=>setFilter(key)}>{label} {total}</button>;
          })}
        </div>
        <div className="dw-groups">
          {filtered.map(item=>(
            <button key={item.id} className={`dw-group ${item.id===group?'is-sel':''}`} onClick={()=>setGroup(item.id)}>
              <span className="dw-gid">{item.id}</span>
              <span className="dw-gname"><i className={`lay-${item.layer}`}>{item.layer}</i>{baseName(item.keep)||'（无保留项）'}</span>
              <span className="dw-gcount">{item.count}</span>
            </button>
          ))}
          {filtered.length===0&&<div className="dw-empty">没有匹配的分组</div>}
        </div>
      </aside>

      <section className="dw-right">
        {active?<>
          <div className="dw-rtitle">
            <strong>{active.id} · {baseName(active.keep)||'未标记保留项'}</strong>
            <span>{active.files.length} / {active.count} 个文件</span>
            <span>可释放 {formatBytes(active.reclaimable_bytes)}</span>
            <span>{active.grade}</span>
            <div className="dw-rtitle-actions">
              <button onClick={()=>void copyList()}>复制清单</button>
              <button className="is-primary" onClick={()=>active.files[0]&&void audition(active.files[0])}>试听首文件</button>
            </div>
          </div>

          <table className="dw-table">
            <thead><tr><th/><th>文件</th><th>时长</th><th>大小</th><th>相似度 BER</th><th>所在目录</th></tr></thead>
            <tbody>
              {active.files.map(file=>{
                const wave=waves[file.path];
                const activeRow=snap?.id===`dedup:${file.path.replace(/\//g,'\\').toLowerCase()}`;
                return <tr key={file.path} className={activeRow?'is-play':''} onClick={()=>void audition(file)}>
                  <td className="dw-dot">{activeRow?'▶':'·'}</td>
                  <td className="dw-name">{baseName(file.path)}{file.keep&&<span className="dw-keep">保留</span>}</td>
                  <td className="dw-num">{wave&&wave!=='loading'&&wave!=='fail'?clock(wave.duration):'—'}</td>
                  <td className="dw-num">{formatBytes(file.size)}</td>
                  <td className="dw-num">{file.grade}</td>
                  <td className="dw-dir">{dirName(file.path)}</td>
                </tr>;
              })}
            </tbody>
          </table>

          <div className="dw-waves">
            {active.files.slice(0,MAX_LANES).map(file=>{
              const wave=waves[file.path];
              const ratio=wave&&wave!=='loading'&&wave!=='fail'?playhead(wave.duration):null;
              return <div key={file.path} className={`dw-lane ${snap?.id===`dedup:${file.path.replace(/\//g,'\\').toLowerCase()}`?'is-sel':''}`}>
                <span className="dw-lname" title={file.path}>{baseName(file.path)}</span>
                {wave==='loading'&&<div className="dw-wave-box">分析中…</div>}
                {wave==='fail'&&<div className="dw-wave-box is-fail">波形不可用</div>}
                {wave&&wave!=='loading'&&wave!=='fail'&&(
                  <svg className="dw-wave" viewBox="0 0 900 48" preserveAspectRatio="none">
                    <path d={peakPath(wave.peaks)}/>
                    {ratio!=null&&<line className="dw-playhead" x1={ratio*900} y1={0} x2={ratio*900} y2={48}/>}
                  </svg>
                )}
              </div>;
            })}
            {active.files.length>MAX_LANES&&<div className="dw-more">只画前 {MAX_LANES} 轨波形；其余文件可在表格里点行试听</div>}
            {busyWave>0&&<div className="dw-more">正在分析 {busyWave} 个波形…</div>}
          </div>

          <div className="dw-foot">
            <button onClick={()=>void toggle()}>{snap?.state==='playing'?'❚❚ 暂停':'▶ 播放'}</button>
            <button onClick={()=>{const first=active.files[0];if(first)void audition(first,0)}}>■ 回到起点</button>
            <span className="dw-clock">{clock(snap?.position)} / {clock(snap?.duration??fallbackDuration)}</span>
            <span className="dw-warn">只读：不做隔离、不改文件；处置请去 Audio Dedup 界面逐次确认</span>
          </div>
          {hint&&<div className="dw-hint">{hint}</div>}
        </>:
        <div className="dw-empty-state">{error?'无法读取分组结果':'没有可显示的分组'}</div>}
      </section>
    </div>
  </div>;
}

createRoot(document.getElementById('root')!).render(<React.StrictMode><Detail/></React.StrictMode>);
