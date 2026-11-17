import React,{useEffect,useRef,useState} from 'react';
import type {ToolPage} from './tool-navigation';
import type {Preferences} from './preferences';
import {useAppVersion} from './app-version';
import {THEME_ACCENTS,useTheme} from './theme';
import {MUSIC_DIMENSIONS,defaultMusicFilterTemplates,parseTemplateValues} from './music-filter-defaults';
import {BackupSettings,CacheSettings,OutputDeviceSelect,TempSettings} from './settings-panels';

const TABS:[string,string][]=[
 ['display','显示与布局'],['playback','音频试听'],['library','素材库'],
 ['cache','缓存与临时文件'],['backup','数据备份'],['plugins','模块开关'],['tools','账号'],['about','关于']
];
const templateDrafts=(value:Record<string,string[]>)=>Object.fromEntries(MUSIC_DIMENSIONS.map(item=>[item.key,(value[item.key]??[]).join('，')]));
/** 输入用本地草稿，失焦时解析并保存，避免边输入边去空导致无法输入逗号。 */
function FilterTemplateEditor({value,onChange}:{value:Record<string,string[]>;onChange:(next:Record<string,string[]>)=>void}){
 const [draft,setDraft]=useState<Record<string,string>>(()=>templateDrafts(value));
 useEffect(()=>{setDraft(templateDrafts(value));},[value]);
 const commit=(key:string)=>onChange({...value,[key]:parseTemplateValues(draft[key]??'')});
 return <div className="filter-template-editor">{MUSIC_DIMENSIONS.map(item=><label key={item.key}>{item.label}<input value={draft[item.key]??''} placeholder="用「，」分隔预置值" title="用逗号或顿号分隔多个预置值；清空表示该维度不显示预置按钮" onChange={event=>setDraft(old=>({...old,[item.key]:event.target.value}))} onBlur={()=>commit(item.key)} onKeyDown={event=>{if(event.key==='Enter')event.currentTarget.blur();}}/></label>)}</div>;
}

export function Settings({open,onClose,value,onChange,onResetLayout,onOpenTool,library,folder,native}:{
 onOpenTool:(tool:ToolPage)=>void;
 open:boolean;onClose:()=>void;value:Preferences;onChange:(value:Preferences)=>void;
 onResetLayout:()=>void;library:string;folder:string|null;native:boolean;
}){
 const version=useAppVersion();
 const theme=useTheme();
 const dialog=useRef<HTMLDialogElement>(null),[tab,setTab]=useState('display');
 useEffect(()=>{const element=dialog.current;if(!element)return;if(open&&!element.open)element.showModal();else if(!open&&element.open)element.close();},[open]);
 const set=<K extends keyof Preferences>(key:K,item:Preferences[K])=>onChange({...value,[key]:item});
 const check=(key:'rememberLayout'|'simpleWaveform'|'multilingualSearch'|'metadataSearch'|'auditionOnSelect'|'replaySameFromStart'|'stopOnDragToDaw'|'pluginAafImport'|'pluginPtOverlay'|'pluginPtBouncer',label:string,title:string)=>
  <label className="checkbox-setting" title={title}><input type="checkbox" checked={value[key]} onChange={event=>set(key,event.target.checked)}/>{label}</label>;
 return <dialog ref={dialog} className="workspace-settings" onCancel={onClose} onClose={onClose} aria-labelledby="settings-title">
  <header><h2 id="settings-title">设置</h2><button onClick={onClose} aria-label="关闭设置">×</button></header>
  <div className="settings-body">
   <nav aria-label="设置分类">{TABS.map(([id,label])=><button key={id} aria-pressed={tab===id} onClick={()=>setTab(id)}>{label}</button>)}</nav>
   <section>
    {tab==='display'&&<>
     <h3>显示与布局</h3>
     <div className="theme-setting"><span className="theme-setting-label">强调色</span>{THEME_ACCENTS.length>1?<div className="theme-swatches" role="group" aria-label="强调色">{THEME_ACCENTS.map(item=><button key={item.value} type="button" className="theme-swatch" data-accent={item.value} aria-pressed={theme.accent===item.value} aria-label={item.label} title={item.label} onClick={()=>theme.setAccent(item.value)}/>)}</div>:<span className="theme-swatch-static" data-accent={theme.accent} aria-hidden="true"/>}<span className="theme-setting-value">{THEME_ACCENTS.find(item=>item.value===theme.accent)?.label??THEME_ACCENTS[0].label}</span></div>
     <label>底色<select value={theme.mode} onChange={event=>theme.setMode(event.target.value==='light'?'light':'dark')}><option value="dark">深色</option><option value="light">浅色</option></select></label>
     <label>波形配色<select value={value.waveColor} onChange={event=>set('waveColor',event.target.value as Preferences['waveColor'])}><option value="blue">蓝色</option><option value="white">白色</option></select></label>
     {check('rememberLayout','记住面板折叠、比例和列宽','把工作区布局写入本机设置')}
     {check('simpleWaveform','列表小波形只画一条（更快）','仍显示整条音频，只是压缩成一条 512 峰的单色包络、不分左右声道；不会只画开头一段')}
     <button onClick={onResetLayout}>恢复默认面板布局</button>
     <p>四边折叠后仍保留恢复按钮；分隔条可拖动，也可用方向键调整。波形始终覆盖整条音频，提速来自单条包络、更少的峰值点和缓存复用，不靠截断时长。</p>
    </>}
    {tab==='playback'&&<>
     <h3>音频试听</h3>
     <label>试听音量 <strong>{value.volume}%</strong><input type="range" min="0" max="100" value={value.volume} onChange={event=>set('volume',Number(event.target.value))}/></label>
     <OutputDeviceSelect value={value.outputDevice} onChange={item=>set('outputDevice',item)}/>
     <p>试听走哪个声卡 / 接口。只影响本软件试听，不改系统设置；所选设备拔掉后自动回落系统默认。</p>
     <h4 className="settings-subhead">试听行为</h4>
     {check('auditionOnSelect','选中即试听','单击素材行或从列表键盘导航选中可用素材时立即播放')}
     {check('replaySameFromStart','重新选中同一文件时从头播放','再次选中当前素材会重新从头播放，而不是保持当前进度')}
     {check('stopOnDragToDaw','拖拽到 DAW 时停止试听','把素材拖出到 Pro Tools 等宿主前先停止本软件试听，避免同时占用声卡')}
     <p>小波形只在已分析范围内定位；点击小波形或播放按钮始终会试听，不受上面开关影响。</p>
    </>}
    {tab==='library'&&<>
     <h3>素材库</h3>
     <label>启动页面<select value={value.startPage} onChange={event=>set('startPage',event.target.value as Preferences['startPage'])}><option value="sound">音效库</option><option value="music">音乐库</option></select></label>
     {check('multilingualSearch','跨语言搜索（中 / 英 / 日）','输一种语言，同时搜中/英/日同义词；公式、格式、分类、标签、时长、收藏与播放列表筛选都不受影响')}
     {check('metadataSearch','检索时包含内嵌元数据','默认开启；与搜索栏的「含元数据」按钮联动，任一处改动都会保存。关闭后只搜文件名。仅 WAV/BWF 的 bext 与 iXML 字段参与匹配，MP3、FLAC、AIFF、M4A 没有这些内嵌字段')}
     <p>本地静态词表，离线、不联网。词与词之间仍是「同时满足」，未登录词按字面匹配。</p>
     <h4 className="settings-subhead">音乐库筛选模板</h4>
     <p>音乐库筛选条每个维度的预置按钮，用逗号或顿号分隔；失焦即保存，清空表示该维度不显示预置。时长区间为内置值，不在此编辑。</p>
     <FilterTemplateEditor value={value.filterTemplates} onChange={next=>set('filterTemplates',next)}/>
     <button onClick={()=>set('filterTemplates',defaultMusicFilterTemplates())}>恢复默认筛选模板</button>
     <p>内嵌元数据索引在导入／重扫时建立，BWF／iXML 回写后自动刷新；旧版本建立的素材库需重扫一次才能参与检索。两个搜索开关可以叠加，格式、分类、标签、时长、收藏与播放列表筛选都不受影响。</p>
     <p>音效库和音乐库各自管理来源关系。相同文件可在两库引用，共用素材ID和标注；添加路径不会复制、移动或改写音频。</p>
    </>}
    {tab==='cache'&&(native?<><CacheSettings active={open} library={library} folder={folder}/><hr className="settings-divider"/><TempSettings active={open}/></>:<p>缓存与临时文件设置仅在桌面版可用。</p>)}
    {tab==='backup'&&(native?<BackupSettings active={open}/>:<p>数据备份仅在桌面版可用。</p>)}
    {tab==='plugins'&&<><h3>可插拔模块</h3>
     <p>三个模块均为 planned 骨架，未实测。开关默认全关；启用只打开只读命令，写入类命令仍返回 WRITE_BLOCKED，不写任何工程、音频或素材库记录。</p>
     {check('pluginAafImport','AAF／OMF 导入（sfx_aaf_import）','只读解析骨架；需真实测试工程与许可审计，未提供前不解析任何工程')}
     {check('pluginPtOverlay','PT 集成叠层（sfx_pt_overlay）','需运行中的 Pro Tools 与能力探测；本骨架不连接，不执行素材落点')}
     {check('pluginPtBouncer','PT 批量导出（sfx_pt_bouncer）','命名预览骨架；渲染需 Pro Tools，拒绝同名覆盖')}
     <p>此处为设置页开关（偏好层）；编译层 feature 默认全关，两层都启用才显示工具窗口状态。本页不删除任何现有窗口。</p>
    </>}
    {tab==='tools'&&<><h3>账号</h3><button onClick={()=>onOpenTool('account')}>账号、设备与离线授权</button><p>账号当前只连接手动启动的本地开发服务。</p></>}
    {tab==='about'&&<><h3>SFX TOOLS · {version||'读取中…'}</h3><p>本地素材工作台。管理音效与音乐原路径引用，支持定位试听、标签、收藏与制作工具。</p><p>版本号读取自应用自身，与 exe 文件属性里的“产品版本”一致；当前为 debug 构建，未生成安装包。</p></>}
   </section>
  </div>
  <footer><button onClick={onClose}>完成</button></footer>
 </dialog>;
}
