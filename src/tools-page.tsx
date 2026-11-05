import React from 'react';
import type {CatalogAsset} from './catalog-client';
import {findTools,toolPages,toolArea,type ToolPage} from './tool-navigation';
import {AudioIntelligence} from './audio-intelligence';
import {FoundationTools} from './foundation-tools';
import {AccountPanel} from './account-panel';
import {SmartTools} from './smart-tools';
import {LayerTools} from './layer-tools';
import {timeLabel} from './demo-data';
import './tools-page.css';

export function ToolsPage({native,ids,selected,library,query,active,onNavigate,onClearQuery,onBack,onPick,onChanged,onCheck,onJobs,busy,area='library'}:{area?:'production'|'library';native:boolean;ids:string[];selected:CatalogAsset|null;library:'sound'|'music';query:string;active:ToolPage|null;onNavigate:(page:ToolPage|null)=>void;onClearQuery:()=>void;onBack:()=>void;onPick:(asset:CatalogAsset)=>void;onChanged:()=>void;onCheck:()=>void;onJobs:()=>void;busy:boolean}){
 const matches=findTools(query).filter(tool=>toolArea(tool.id)===area),page=toolPages.find(tool=>tool.id===active);
 const searching=!!query.trim();
 return <div className="audio-tools-page tool-hub">
  <div className="tool-hub-heading"><div><h1>{area==='production'?'音频工具':'素材管理与检索'}</h1><p>选择一项工具开始 · {library==='sound'?'音效库':'音乐库'}</p></div><button onClick={()=>{onClearQuery();onNavigate(null);}}>工具总览</button></div>
  <div className="tool-selection"><div><strong>{ids.length?`已选 ${ids.length} 条素材`:'尚未选择素材'}</strong><span>{ids.length===1&&selected?.id===ids[0]?selected.name:ids.length?'使用当前库的所选素材':'批量导出、转换与以声搜声需要先在素材库选中素材。'}</span></div><button onClick={onBack}>{ids.length?'返回素材库 / 更换选择':'去素材库选择'}</button></div>
  <nav className={`tool-directory ${active&&!searching?'is-compact':''}`} aria-label="音频工具分类">
   {matches.map(tool=><button key={tool.id} aria-pressed={active===tool.id} onClick={()=>{onClearQuery();onNavigate(tool.id);}}><strong>{tool.label}</strong>{(!active||searching)&&<span>{tool.description}</span>}</button>)}
  </nav>
  {searching&&<div className="tool-search-feedback" role="status">{matches.length?`找到 ${matches.length} 项工具，点击进入。`:'没有匹配的工具。'}<button onClick={onClearQuery}>清除工具搜索</button></div>}
  {!active&&<section className="tool-overview"><h2>功能与入口</h2><p>分类按钮始终可达，不再把所有工具堆在长页面中。选择素材后进入相应工具；检查重复、模型登记和账号不要求选择素材。</p><p>模型下载队列（固定版本、断点续传、SHA256校验、取消）与CLAP双塔ONNX张量自检已接入“声学内容检索”页；音频内容语义推理、中文相关性评测、AAF/OMF解析和外部插件宿主尚未实现；账号与订单当前仅为本地开发流程。界面中的状态会说明所需条件。</p></section>}
  <div hidden={!active}>
   <header className="tool-content-heading"><h2>{page?.label}</h2><p>{page?.description}</p>{searching&&<p>正在显示已打开的工具；上方搜索只筛选入口，不会中断当前任务。</p>}</header>
   <div hidden={active!=='acoustic'}><AudioIntelligence native={native} ids={ids} library={library} onPick={onPick}/></div>
   <div hidden={!['ucs','duplicates','convert','plugins'].includes(active??'')}><FoundationTools native={native} ids={ids} library={library} onPick={onPick} onChanged={onChanged} section={active==='duplicates'||active==='convert'||active==='plugins'?active:'ucs'}/></div>
   <div hidden={active!=='layer'}><LayerTools/></div>
   <div hidden={active!=='smart'}><SmartTools native={native} ids={ids} onPick={onPick} initiallyOpen mode="semantic"/></div>
   <div hidden={active!=='pt'}><SmartTools native={native} ids={ids} onPick={onPick} initiallyOpen mode="pt"/></div>
   <div hidden={active!=='account'}><AccountPanel native={native} initiallyOpen/></div>
   <section hidden={active!=='maintenance'} className="utility-panel"><h2>素材库维护</h2><p>检查已登记文件是否仍可访问，保留缺失记录和用户标注。</p><button disabled={!native||busy} onClick={onCheck}>检查所有已登记素材</button><button onClick={onJobs}>查看导入任务</button></section>
   <section hidden={active!=='info'} className="utility-panel"><h2>音频信息</h2>{selected?<><p>{selected.name}</p><dl><div><dt>格式 / 采样率</dt><dd>{selected.format} / {selected.rate??'未知'} Hz</dd></div><div><dt>声道 / 时长</dt><dd>{selected.channels??'未知'} / {selected.duration==null?'未知':timeLabel(selected.duration)}</dd></div></dl></>:<p>先在素材库选择音频，再回到此页查看规格。</p>}</section>
  </div>
 </div>;
}
