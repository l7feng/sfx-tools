import {invoke} from '@tauri-apps/api/core';

export type CacheGroup={id:string;label:string;path:string;files:number;bytes:number};
export type CacheInfo={
 enabled:boolean;root:string;default_root:string;custom_root:string|null;backup_root:string;
 wave_groups:CacheGroup[];temp_groups:CacheGroup[];database_bytes:number;restart_required:boolean;
};
export type CleanResult={files:number;bytes:number;groups:number};
export type Pregenerate={
 status:string;total:number;done:number;skipped:number;failed:number;
 error:string|null;current:string|null;cancelable:boolean;
};
export type OutputDevice={id:string;name:string};
export type BackupItem={name:string;path:string;bytes:number;created:string};
export type BackupResult={name:string;path:string;bytes:number;catalog_bytes:number;integrity:string;manifest:string};

export const settingsApi={
 cacheInfo:()=>invoke<CacheInfo>('settings_cache_info'),
 setCacheEnabled:(enabled:boolean)=>invoke<CacheInfo>('settings_cache_set_enabled',{enabled}),
 setCacheRoot:(path:string|null)=>invoke<CacheInfo>('settings_cache_set_root',{path}),
 pickFolder:(initial:string|null)=>invoke<string|null>('settings_pick_folder',{initial}),
 clearCache:(groups:string[])=>invoke<CleanResult>('settings_cache_clear',{groups}),
 clearTemp:()=>invoke<CleanResult>('settings_temp_clear'),
 openFolder:(path:string)=>invoke<void>('settings_open_folder',{path}),
 pregenerate:(scope:string,library:string|null,folder:string|null)=>invoke<void>('settings_cache_pregenerate',{scope,library,folder}),
 progress:()=>invoke<Pregenerate|null>('settings_cache_progress'),
 cancel:()=>invoke<void>('settings_cache_cancel'),
 outputDevices:()=>invoke<OutputDevice[]>('settings_output_devices'),
 setDevice:(name:string)=>invoke<void>('settings_player_set_device',{name}),
 backupList:()=>invoke<BackupItem[]>('settings_backup_list'),
 backupCreate:(snapshot:unknown)=>invoke<BackupResult>('settings_backup_create',{snapshot}),
};

export function formatSize(bytes:number){
 if(!Number.isFinite(bytes)||bytes<=0)return '0 B';
 const units=['B','KB','MB','GB','TB'];
 const index=Math.min(units.length-1,Math.floor(Math.log(bytes)/Math.log(1024)));
 const value=bytes/1024**index;
 return `${value>=10||index===0?Math.round(value):value.toFixed(1)} ${units[index]}`;
}

/** 备份里带上全部本应用的 localStorage：主题、偏好、布局、列设置、工具页草稿等。 */
export function settingsSnapshot():Record<string,string|null>{
 const out:Record<string,string|null>={};
 try{
  for(let index=0;index<localStorage.length;index++){
   const key=localStorage.key(index);
   if(key&&/^(sound-workbench|sfx-tools)/.test(key))out[key]=localStorage.getItem(key);
  }
 }catch{/* 存储不可用时只备份数据库，不阻断 */}
 return out;
}

export function totalOf(groups:CacheGroup[]){
 return groups.reduce((sum,group)=>({files:sum.files+group.files,bytes:sum.bytes+group.bytes}),{files:0,bytes:0});
}
