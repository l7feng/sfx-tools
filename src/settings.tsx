import React,{useEffect,useRef,useState} from 'react';
import type {ToolPage} from './tool-navigation';
import type {Preferences} from './preferences';
import {useAppVersion} from './app-version';
import {THEME_ACCENTS,useTheme} from './theme';
import {BackupSettings,CacheSettings,OutputDeviceSelect,TempSettings} from './settings-panels';

const TABS:[string,string][]=[
 ['display','显示与布局'],['playback','音频试听'],['library','素材库'],
 ['cache','缓存与临时文件'],['backup','数据备份'],['tools','模型与账号'],['about','关于']
];

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
 const check=(key:'rememberLayout'|'simpleWaveform'|'collapseToolsPanels'|'multilingualSearch'|'auditionOnSelect'|'replaySameFromStart'|'stopOnDragToDaw',label:string,title:string)=>
  <label className="checkbox-setting" title={title}><input type="checkbox" checked={value[key]} onChange={event=>set(key,event.target.checked)}/>{label}</label>;
 return <dialog ref={dialog} className="workspace-settings" onCancel={onClose} onClose={onClose} aria-labelledby="settings-title">
  <header><h2 id="settings-title">设置</h2><button onClick={onClose} aria-label="关闭设置">×</button></header>
  <div className="settings-body">
   <nav aria-label="设置分类">{TABS.map(([id,label])=><button key={id} aria-pressed={tab===id} onClick={()=>setTab(id)}>{label}</button>)}</nav>
   <section>
    {tab==='display'&&<>
     <h3>显示与布局</h3>
     <div className="theme-setting"><span className="theme-setting-label">强调色</span><div className="theme-swatches" role="group" aria-label="强调色">{THEME_ACCENTS.map(item=><button key={item.value} type="button" className="theme-swatch" data-accent={item.value} aria-pressed={theme.accent===item.value} aria-label={item.label} title={item.label} onClick={()=>theme.setAccent(item.value)}/>)}</div><span className="theme-setting-value">{THEME_ACCENTS.find(item=>item.value===theme.accent)?.label}</span></div>
     <label>底色<select value={theme.mode} onChange={event=>theme.setMode(event.target.value==='light'?'light':'dark')}><option value="dark">深色</option><option value="light">浅色</option></select></label>
     <label>列表密度<select value={value.density} onChange={event=>set('density',event.target.value as Preferences['density'])}><option value="compact">紧凑</option><option value="comfortable">舒适</option></select></label>
     <label>波形配色<select value={value.waveColor} onChange={event=>set('waveColor',event.target.value as Preferences['waveColor'])}><option value="blue">蓝色</option><option value="white">白色</option></select></label>
     {check('rememberLayout','记住面板折叠、比例和列宽','把工作区布局写入本机设置')}
     {check('simpleWaveform','列表小波形只画一条（更快）','仍显示整条音频，只是压缩成一条 512 峰的单色包络、不分左右声道；不会只画开头一段')}
     {check('collapseToolsPanels','打开音频工具时折叠左右下栏','此窗口用不到素材栏、详情栏和底部播放器；离开工具页自动恢复')}
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
     <label>启动页面<select value={value.startPage} onChange={event=>set('startPage',event.target.value as Preferences['startPage'])}><option value="sound">音效库</option><option value="music">音乐库</option><option value="tools">音频工具</option></select></label>
     {check('multilingualSearch','跨语言搜索（中 / 英 / 日）','输一种语言，同时搜中/英/日同义词；公式、格式、分类、标签、时长、收藏与播放列表筛选都不受影响')}
     <p>本地静态词表，离线、不联网、不调用模型。词与词之间仍是「同时满足」，未登录词按字面匹配。</p>
     <p>音效库和音乐库各自管理来源关系。相同文件可在两库引用，共用素材ID和标注；添加路径不会复制、移动或改写音频。</p>
    </>}
    {tab==='cache'&&(native?<><CacheSettings active={open} library={library} folder={folder}/><hr className="settings-divider"/><TempSettings active={open}/></>:<p>缓存与临时文件设置仅在桌面版可用。</p>)}
    {tab==='backup'&&(native?<BackupSettings active={open}/>:<p>数据备份仅在桌面版可用。</p>)}
    {tab==='tools'&&<><h3>模型与账号</h3><p>从这里可直接进入工具页面，不会启动模型下载或开发服务。</p><button onClick={()=>onOpenTool('acoustic')}>声学索引与模型管理</button><button onClick={()=>onOpenTool('account')}>账号、设备与离线授权</button><p>账号当前只连接手动启动的本地开发服务。</p></>}
    {tab==='about'&&<><h3>SFX Tools · {version||'读取中…'}</h3><p>本地素材工作台。管理音效与音乐原路径引用，支持定位试听、标签、收藏与制作工具。</p><p>版本号读取自应用自身，与 exe 文件属性里的“产品版本”一致；当前为 debug 构建，未生成安装包。</p></>}
   </section>
  </div>
  <footer><button onClick={onClose}>完成</button></footer>
 </dialog>;
}
