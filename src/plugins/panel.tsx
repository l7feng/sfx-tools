import React from 'react';
import type {Preferences} from '../preferences';
import {PLUGIN_META,type PluginId} from './client';
import {syncAdapterFlags} from './adapter';

/** 只读一次偏好；离线渲染或存储不可用时三个开关全部按关闭处理。 */
function readFlags():Record<PluginId,boolean>{
 const fallback:Record<PluginId,boolean>={sfx_aaf_import:false,sfx_pt_overlay:false,sfx_pt_bouncer:false};
 try{
  const raw=JSON.parse(localStorage.getItem('sound-workbench-preferences-v1')??'{}') as Partial<Preferences>;
  return {
   sfx_aaf_import:raw.pluginAafImport===true,
   sfx_pt_overlay:raw.pluginPtOverlay===true,
   sfx_pt_bouncer:raw.pluginPtBouncer===true,
  };
 }catch{return fallback;}
}

/** 工具窗口内的只读状态面板：不提供写入按钮，不复用既有只读预览文案。 */
export function PluginPanel(){
 const flags=readFlags();
 React.useEffect(()=>{void syncAdapterFlags();},[flags.sfx_aaf_import,flags.sfx_pt_overlay,flags.sfx_pt_bouncer]);
 return <section className="utility-panel plugin-panel" aria-label="可插拔模块">
  <h2>可插拔模块（3）</h2>
  <p>三个模块均为 read_only：只做文件头／本机路径、端口、目录的只读启发式探测，未连接真实 Pro Tools、未经真实工程验证。写入类命令仍恒返回 WRITE_BLOCKED，不写任何工程、音频或素材库记录。</p>
  {PLUGIN_META.map(meta=><article key={meta.id} className="plugin-card">
   <div className="plugin-card-head"><h3>{meta.label}</h3><span className="plugin-badge">只读 · 启发式探测</span></div>
   <code>{meta.id}</code>
   <p>{meta.dependency}</p>
   <p>只读：{meta.readOnly.join(' / ')}</p>
   <p>写入：{meta.write}（阻断）</p>
   <small>evidence: {meta.evidence} · 设置开关：{flags[meta.id]?'已启用':'默认关闭'} · status: {meta.status}</small>
  </article>)}
  <p>双层开关：Cargo feature ＋ 设置页启用项。关闭时命令不执行、入口不显示；启用后仍需真实测试工程与人类授权才允许写入。本窗口不提供写入操作。</p>
  <p>适配层：plugin_adapter_invoke 为统一入口，挂载时把设置页开关同步到后端；未同步则后端按关闭拒绝。九条直连命令仍保留，两者共用同一闸门。</p>
 </section>;
}
