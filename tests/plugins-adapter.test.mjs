// 插件适配器层静态契约核对：只读源码文本，不启动 Tauri、不运行 EXE、不访问工程或素材库。
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';

const read=path=>fs.readFileSync(new URL('../'+path,import.meta.url),'utf8');
const mainRs=read('src-tauri/src/main.rs');
const modRs=read('src-tauri/src/plugins/mod.rs');
const adapterRs=read('src-tauri/src/plugins/adapter.rs');
const adapterTs=read('src/plugins/adapter.ts');
const panel=read('src/plugins/panel.tsx');
const ids=['sfx_aaf_import','sfx_pt_overlay','sfx_pt_bouncer'];
const adapterCommands=['plugin_adapter_describe','plugin_adapter_set_flags','plugin_adapter_invoke'];

test('main.rs 注册三条适配器命令，九条直连命令数量不变',()=>{
 for(const name of adapterCommands)assert(mainRs.includes(name),name);
 assert.equal((mainRs.match(/plugin_sfx_/g)??[]).length,9);
 assert(modRs.includes('pub mod adapter;'));
});

test('适配器层声明统一 trait 与三个 in_process 适配器',()=>{
 assert(adapterRs.includes('pub trait PluginAdapter'));
 for(const name of ['AafImportAdapter','PtOverlayAdapter','PtBouncerAdapter','impl PluginAdapter'])assert(adapterRs.includes(name),name);
 assert(adapterRs.includes('kind: "in_process_module"'));
 assert(adapterRs.includes('status: "read_only"'));
 assert(!adapterRs.includes('status: "planned"'),'P2 后不得回退到 planned 骨架');
 for(const id of ids)assert(adapterRs.includes(id),id);
});

test('适配器层不加载外部二进制、不运行进程、不写文件',()=>{
 for(const forbidden of ['std::process','Command::new','std::fs::write','std::fs::File','dlopen','libloading','std::thread::spawn']){
  assert(!adapterRs.includes(forbidden),'适配器层不应出现 '+forbidden);
 }
 assert(adapterRs.includes('blocked(id, spec.name)'),'写入类命令必须走 blocked');
 assert(adapterRs.includes('if !preference_enabled'),'偏好层必须是后端闸门');
});

test('三份 plugin.toml 的能力声明与适配器层逐项一致',()=>{
 for(const id of ids){
  const meta=read('src-tauri/src/plugins/'+id+'/plugin.toml');
  const line=meta.split('\n').find(item=>item.startsWith('capabilities ='))??'';
  const declared=[...line.matchAll(/"([a-z_]+)"/g)].map(match=>match[1]);
  assert(declared.length>0,id+' 未声明能力');
  for(const capability of declared)assert(adapterRs.includes('"'+capability+'"'),id+' 能力 '+capability+' 未在适配器层登记');
 }
});

test('前端 adapter.ts 暴露统一入口且写入默认关闭',()=>{
 for(const name of adapterCommands)assert(adapterTs.includes("'"+name+"'"),name);
 for(const name of ['readAdapterFlags','syncAdapterFlags','describe','setFlags','call'])assert(adapterTs.includes(name),name);
 assert(adapterTs.includes('allowWrite:args.allowWrite??false'));
 assert(adapterTs.includes('pluginAafImport')&&adapterTs.includes('pluginPtOverlay')&&adapterTs.includes('pluginPtBouncer'));
 assert(!adapterTs.includes('allowWrite:true'));
});

test('面板挂载时同步偏好开关，且如实标注只读探测边界',()=>{
 assert(panel.includes('syncAdapterFlags'));
 assert(panel.includes('React.useEffect'));
 assert(panel.includes('plugin_adapter_invoke'));
 for(const forbidden of ['已实现','已解析','语义检索'])assert(!panel.includes(forbidden),forbidden);
 assert(panel.includes('read_only'));
 assert(panel.includes('未经真实工程验证'));
 assert(panel.includes('WRITE_BLOCKED'));
});
