import {invoke} from '@tauri-apps/api/core';

export type PluginId='sfx_aaf_import'|'sfx_pt_overlay'|'sfx_pt_bouncer';
export type PluginEvidence='static_parse'|'local_probe'|'pt_live'|'contract_only'|'none';
export type PluginResult={
 plugin_id:PluginId;command:string;ok:boolean;code:string;message:string;
 component_epoch:number;data:unknown;evidence:PluginEvidence;warnings:string[];
};
export type PluginMeta={
 id:PluginId;label:string;dependency:string;status:'planned'|'read_only';
 readOnly:string[];write:string;evidence:PluginEvidence;feature:string;
};

export const PLUGIN_META:PluginMeta[]=[
 {id:'sfx_aaf_import',label:'AAF／OMF 导入',dependency:'文件头只读探测 · 未解析时间线',status:'read_only',readOnly:['parse','candidates'],write:'to_session',evidence:'static_parse',feature:'plugin-aaf-import'},
 {id:'sfx_pt_overlay',label:'PT 集成叠层',dependency:'本机只读探测 · 未连接 PT',status:'read_only',readOnly:['capabilities','status'],write:'spot',evidence:'local_probe',feature:'plugin-pt-overlay'},
 {id:'sfx_pt_bouncer',label:'PT 批量导出',dependency:'输出目录只读预览 · 不渲染',status:'read_only',readOnly:['preview','cancel'],write:'run',evidence:'local_probe',feature:'plugin-pt-bouncer'},
];

export const PLUGIN_COMMANDS=[
 'plugin_sfx_aaf_import_parse','plugin_sfx_aaf_import_candidates','plugin_sfx_aaf_import_to_session',
 'plugin_sfx_pt_overlay_capabilities','plugin_sfx_pt_overlay_spot','plugin_sfx_pt_overlay_status',
 'plugin_sfx_pt_bouncer_preview','plugin_sfx_pt_bouncer_run','plugin_sfx_pt_bouncer_cancel',
] as const;

/** 九条命令的统一入口。写入类命令在后端恒返回 WRITE_BLOCKED，前端不提供触发按钮。 */
export const pluginApi={
 aafParse:(args:{sourcePath?:string;mediaRoots?:string[];readOnly?:boolean})=>invoke<PluginResult>('plugin_sfx_aaf_import_parse',args),
 aafCandidates:(args:{parseId?:string;reviewed?:boolean})=>invoke<PluginResult>('plugin_sfx_aaf_import_candidates',args),
 aafToSession:(args:{parseId?:string;allowWrite?:boolean})=>invoke<PluginResult>('plugin_sfx_aaf_import_to_session',args),
 overlayCapabilities:()=>invoke<PluginResult>('plugin_sfx_pt_overlay_capabilities'),
 overlaySpot:(args:{assetIds?:string[];timecode?:string;track?:string;allowWrite?:boolean})=>invoke<PluginResult>('plugin_sfx_pt_overlay_spot',args),
 overlayStatus:()=>invoke<PluginResult>('plugin_sfx_pt_overlay_status'),
 bouncerPreview:(args:{outputDir?:string;targets?:string[]})=>invoke<PluginResult>('plugin_sfx_pt_bouncer_preview',args),
 bouncerRun:(args:{previewId?:string;allowWrite?:boolean})=>invoke<PluginResult>('plugin_sfx_pt_bouncer_run',args),
 bouncerCancel:(args:{jobId?:string})=>invoke<PluginResult>('plugin_sfx_pt_bouncer_cancel',args),
};
