import {invoke} from '@tauri-apps/api/core';
import type {PluginId,PluginResult} from './client';

/** 与 Rust 侧 AdapterDescriptor 对齐。status 恒为 planned，禁止在前端写成可用。 */
export type AdapterDescriptor={
 plugin_id:PluginId;kind:string;status:'planned';feature:string;
 compiled:boolean;runtime_enabled:boolean;requires_pt:boolean;
 capabilities:string[];read_only_commands:string[];write_commands:string[];
 adapter_epoch:number;
};

export type AdapterFlags={aaf:boolean;overlay:boolean;bouncer:boolean};

const PREFERENCE_KEY='sound-workbench-preferences-v1';

/** 复用既有偏好键；读取失败或旧存档缺字段时一律按关闭处理。 */
export function readAdapterFlags():AdapterFlags{
 try{
  const raw=JSON.parse(localStorage.getItem(PREFERENCE_KEY)??'{}') as Record<string,unknown>;
  return {
   aaf:raw.pluginAafImport===true,
   overlay:raw.pluginPtOverlay===true,
   bouncer:raw.pluginPtBouncer===true,
  };
 }catch{return {aaf:false,overlay:false,bouncer:false};}
}

/** 统一入口的 params 使用 snake_case，与 Rust 命令签名一致。 */
function toSnake(params:Record<string,unknown>):Record<string,unknown>{
 const out:Record<string,unknown>={};
 for(const [key,value] of Object.entries(params))out[key.replace(/[A-Z]/g,match=>'_'+match.toLowerCase())]=value;
 return out;
}

export const adapterApi={
 describe:()=>invoke<AdapterDescriptor[]>('plugin_adapter_describe'),
 setFlags:(flags:AdapterFlags)=>invoke<AdapterDescriptor[]>('plugin_adapter_set_flags',flags),
 call:(args:{pluginId:PluginId;command:string;params?:Record<string,unknown>;allowWrite?:boolean})=>
  invoke<PluginResult>('plugin_adapter_invoke',{
   pluginId:args.pluginId,
   command:args.command,
   params:toSnake(args.params??{}),
   allowWrite:args.allowWrite??false,
  }),
};

/**
 * 宿主挂载时把设置页开关同步到后端适配器层，让偏好层成为真闸门。
 * 非 Tauri／离线环境下 invoke 会失败，调用方吞掉即可，不改变默认全关状态。
 */
export async function syncAdapterFlags():Promise<AdapterDescriptor[]|null>{
 try{return await adapterApi.setFlags(readAdapterFlags());}
 catch{return null;}
}
