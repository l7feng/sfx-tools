import React,{useCallback,useEffect,useState} from 'react';
import {settingsApi,formatSize,totalOf,settingsSnapshot,type CacheInfo,type Pregenerate,type BackupItem,type OutputDevice} from './settings-client';

const errorMessage=(error:unknown)=>String(error instanceof Error?error.message:error??'');

function useAction(){
 const [busy,setBusy]=useState(false),[message,setMessage]=useState('');
 const run=useCallback(async(work:()=>Promise<void>)=>{
  setBusy(true);setMessage('');
  try{await work();}catch(error){setMessage(errorMessage(error));}finally{setBusy(false);}
 },[]);
 return {busy,message,setMessage,run};
}

/** 图片 #4：缓存波形图（开关 / 路径 / 统计 / 清空 / 预生成）。 */
export function CacheSettings({active,library,folder}:{active:boolean;library:string;folder:string|null}){
 const [info,setInfo]=useState<CacheInfo|null>(null),[job,setJob]=useState<Pregenerate|null>(null),[polling,setPolling]=useState(false);
 const {busy,message,setMessage,run}=useAction();
 const refresh=useCallback(async()=>setInfo(await settingsApi.cacheInfo()),[]);
 useEffect(()=>{if(active)void refresh().catch(error=>setMessage(errorMessage(error)));},[active,refresh,setMessage]);
 useEffect(()=>{
  if(!active||!polling)return;
  let stopped=false,timer:number;
  const poll=async()=>{
   try{const next=await settingsApi.progress();if(!stopped&&next){setJob(next);if(!next.cancelable)setPolling(false);}}
   catch(error){if(!stopped){setMessage(errorMessage(error));setPolling(false);}}
   finally{if(!stopped)timer=window.setTimeout(poll,400);}
  };
  void poll();return()=>{stopped=true;clearTimeout(timer);};
 },[active,polling,setMessage]);
 const waves=info?totalOf(info.wave_groups):{files:0,bytes:0};
 const startPregenerate=(scope:'folder'|'all')=>run(async()=>{
  await settingsApi.pregenerate(scope,library,scope==='folder'?folder:null);
  setJob(null);setPolling(true);
 });
 return <>
  <h3>缓存波形图</h3>
  <div className="settings-lead"><label className="checkbox-setting"><input type="checkbox" checked={info?.enabled??true} disabled={!info||busy} onChange={event=>{const enabled=event.target.checked;void run(async()=>{setInfo(await settingsApi.setCacheEnabled(enabled));});}}/>缓存波形图</label><p>缓存音频文件的波形图数据，减少重复生成。关闭后每次都重新解码，不写新缓存。</p></div>
  <section className="settings-card"><h4>波形图缓存路径</h4>
   <input className="settings-path" aria-label="波形图缓存路径" readOnly value={info?.root??'读取中…'}/>
   <div className="settings-row">
    <button disabled={!info||busy} onClick={()=>void run(async()=>{const chosen=await settingsApi.pickFolder(info?.root??null);if(chosen){setInfo(await settingsApi.setCacheRoot(chosen));setMessage('缓存路径已保存，下次启动生效。');}})}>选择路径</button>
    <button disabled={!info||busy} onClick={()=>void run(async()=>{setInfo(await settingsApi.setCacheRoot(null));setMessage('已恢复默认路径，下次启动生效。');})}>重置为默认</button>
    <button disabled={!info||busy} onClick={()=>void run(async()=>settingsApi.openFolder(info!.root))}>打开缓存目录</button>
   </div>
   <p>默认：{info?.default_root??'—'}{info?.restart_required&&<strong className="settings-hint"> · 路径改动需重启后生效</strong>}</p>
  </section>
  <section className="settings-card"><h4>波形图缓存统计</h4>
   <p className="settings-stats">缓存数量：<strong>{waves.files}</strong>　缓存大小：<strong>{formatSize(waves.bytes)}</strong>　数据库：<strong>{formatSize(info?.database_bytes??0)}</strong></p>
   {info?.wave_groups.map(group=><p className="settings-group" key={group.id}>{group.label}：{group.files} 个 / {formatSize(group.bytes)}</p>)}
   <div className="settings-row"><button disabled={busy} onClick={()=>void run(refresh)}>刷新缓存统计</button><button className="danger" disabled={!info||busy} onClick={()=>void run(async()=>{const result=await settingsApi.clearCache((info?.wave_groups??[]).map(group=>group.id));await refresh();setMessage(`已清空波形缓存：${result.files} 个文件，释放 ${formatSize(result.bytes)}。`);})}>清空波形图缓存</button></div>
  </section>
  <section className="settings-card"><h4>预生成波形缓存</h4>
   <p>仅处理缺失缓存的音频，已有缓存会自动跳过；任务可随时取消。生成的是列表简化小波形（单条包络）。</p>
   <div className="settings-row"><button disabled={busy||!folder} title={folder?'只处理当前文件夹':'先在素材库选择一个文件夹'} onClick={()=>void startPregenerate('folder')}>当前文件夹</button><button disabled={busy} onClick={()=>void startPregenerate('all')}>全部素材库</button>{polling&&<button disabled={busy} onClick={()=>void settingsApi.cancel()}>取消任务</button>}</div>
   {job&&<p className="settings-job" role="status">{job.status==='running'?'处理中':job.status==='completed'?'已完成':job.status==='cancelled'?'已取消':job.status==='failed'?'失败':job.status} · {job.done+job.skipped+job.failed}/{job.total}　新增 {job.done} · 跳过 {job.skipped} · 失败 {job.failed}{job.current?` · ${job.current}`:''}</p>}
   {job?.error&&<p className="asset-error">{job.error}</p>}
  </section>
  {message&&<p className="settings-message" role="status">{message}</p>}
 </>;
}

/** 图片 #5：临时文件清理，只清白名单内的可再生产物。 */
export function TempSettings({active}:{active:boolean}){
 const [info,setInfo]=useState<CacheInfo|null>(null);
 const {busy,message,setMessage,run}=useAction();
 const refresh=useCallback(async()=>setInfo(await settingsApi.cacheInfo()),[]);
 useEffect(()=>{if(active)void refresh().catch(error=>setMessage(errorMessage(error)));},[active,refresh,setMessage]);
 const temps=info?totalOf(info.temp_groups):{files:0,bytes:0};
 return <>
  <h3>临时文件（预放缓存 / 拖拽切片）</h3>
  <p>只清理可再生的临时产物；不会触碰你的音频文件、资料库索引与个人设置。</p>
  <p className="settings-stats">当前 {temps.files} 个文件 / {formatSize(temps.bytes)}</p>
  <button className="settings-wide" disabled={busy} onClick={()=>void run(async()=>{const result=await settingsApi.clearTemp();await refresh();setMessage(result.files?`已清理 ${result.files} 个临时文件，释放 ${formatSize(result.bytes)}。`:'没有需要清理的临时文件。');})}>🗑 清理临时文件</button>
  {message&&<p className="settings-message" role="status">{message}</p>}
 </>;
}

/** 图片 #6：数据备份，每次生成独立目录，永不覆盖旧备份。 */
export function BackupSettings({active}:{active:boolean}){
 const [items,setItems]=useState<BackupItem[]>([]),[root,setRoot]=useState('');
 const {busy,message,setMessage,run}=useAction();
 const refresh=useCallback(async()=>{const [list,info]=await Promise.all([settingsApi.backupList(),settingsApi.cacheInfo()]);setItems(list);setRoot(info.backup_root);},[]);
 useEffect(()=>{if(active)void refresh().catch(error=>setMessage(errorMessage(error)));},[active,refresh,setMessage]);
 return <>
  <h3>数据备份</h3>
  <p>把 收藏 · 评分 · 笔记 · 分类 · 设置 · 主题 一键打包。升级换版本本就不丢数据，此备份用于换机 / 重装时恢复。每点一次生成新的一份，不会覆盖旧备份。</p>
  <p className="settings-path-line">保存位置：<span>{root||'—'}</span></p>
  <div className="settings-row"><button className="primary" disabled={busy} onClick={()=>void run(async()=>{const result=await settingsApi.backupCreate(settingsSnapshot());await refresh();setMessage(`备份完成：${result.name} · ${formatSize(result.bytes)} · 完整性 ${result.integrity}`);})}>备份我的数据</button><button disabled={busy||!root} onClick={()=>void run(async()=>settingsApi.openFolder(root))}>打开备份文件夹</button></div>
  <h4 className="settings-subhead">已有备份（{items.length}）</h4>
  {items.length?<ul className="settings-backup-list">{items.slice(0,12).map(item=><li key={item.name}><span title={item.path}>{item.name}</span><small>{item.created} · {formatSize(item.bytes)}</small></li>)}</ul>:<p className="settings-group">尚无备份。</p>}
  <p>备份含 catalog.sqlite（VACUUM INTO 一致性快照，含 SHA256 与 integrity_check）与 settings.json（界面设置快照）。恢复前请先退出程序并自行保留现有数据。</p>
  {message&&<p className="settings-message" role="status">{message}</p>}
 </>;
}

/** 图片 #3：输出设备下拉；列表在打开设置时读取一次，设备拔掉后由后端自动回落。 */
export function OutputDeviceSelect({value,onChange}:{value:string;onChange:(value:string)=>void}){
 const [devices,setDevices]=useState<OutputDevice[]>([{id:'default',name:'系统默认'}]);
 useEffect(()=>{let alive=true;void settingsApi.outputDevices().then(list=>{if(alive&&list.length)setDevices(list);}).catch(()=>{});return()=>{alive=false;};},[]);
 return <label>输出设备<select aria-label="试听输出设备" value={devices.some(item=>item.id===value)?value:'default'} onChange={event=>onChange(event.target.value)}>{devices.map(item=><option key={item.id} value={item.id}>{item.name}</option>)}</select></label>;
}
