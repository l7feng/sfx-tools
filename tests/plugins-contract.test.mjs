// 插件契约静态核对：只读源码文本，不启动 Tauri、不访问任何工程文件或素材库。
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {safePreferences,defaultPreferences} from '../src/preferences.ts';

const read=path=>fs.readFileSync(new URL('../'+path,import.meta.url),'utf8');
const cargo=read('src-tauri/Cargo.toml');
const mainRs=read('src-tauri/src/main.rs');
const client=read('src/plugins/client.ts');
const panel=read('src/plugins/panel.tsx');
const settings=read('src/settings.tsx');
const toolsPage=read('src/tools-page.tsx');
const ids=['sfx_aaf_import','sfx_pt_overlay','sfx_pt_bouncer'];
const commands=[
 'plugin_sfx_aaf_import_parse','plugin_sfx_aaf_import_candidates','plugin_sfx_aaf_import_to_session',
 'plugin_sfx_pt_overlay_capabilities','plugin_sfx_pt_overlay_spot','plugin_sfx_pt_overlay_status',
 'plugin_sfx_pt_bouncer_preview','plugin_sfx_pt_bouncer_run','plugin_sfx_pt_bouncer_cancel',
];

test('三个模块开关默认全关，旧存档不静默开启',()=>{
 for(const key of ['pluginAafImport','pluginPtOverlay','pluginPtBouncer']){
  assert.equal(defaultPreferences[key],false);
  assert.equal(safePreferences({})[key],false);
  assert.equal(safePreferences({[key]:true})[key],true);
 }
 assert.deepEqual(safePreferences(null),defaultPreferences);
});

test('Cargo features 默认全关且三个模块各有一条',()=>{
 assert.match(cargo,/\[features\]/);
 assert.match(cargo,/default = \[\]/);
 for(const flag of ['plugin-aaf-import','plugin-pt-overlay','plugin-pt-bouncer'])assert(cargo.includes(flag+' = []'),flag);
});

test('main.rs 声明 plugins 模块并注册九条命令',()=>{
 assert(mainRs.includes('mod plugins;'));
 for(const name of commands)assert(mainRs.includes(name),name);
 assert.equal((mainRs.match(/plugin_sfx_/g)??[]).length,commands.length);
});

test('三份 plugin.toml 均为 read_only 的进程内模块',()=>{
 for(const id of ids){
  const meta=read('src-tauri/src/plugins/'+id+'/plugin.toml');
  assert(meta.includes('id = "'+id+'"'));
  assert(meta.includes('status = "read_only"'));
  assert(meta.includes('requires_test_project = true'),'写入仍需真实测试工程前置');
  assert(meta.includes('kind = "in_process_module"'));
  assert(meta.includes('min_app_version = "0.1.25"'));
 }
});

test('前端 client 暴露九条命令，面板明确标注只读探测与未经真实工程验证',()=>{
 for(const name of commands)assert(client.includes("'"+name+"'"),name);
 for(const text of ['read_only','WRITE_BLOCKED','evidence:','未经真实工程验证'])assert(panel.includes(text),text);
 assert(!panel.includes('status: planned'),'面板不得回退到骨架口径');
 for(const tag of ['static_parse','local_probe','contract_only'])assert(client.includes("'"+tag+"'"),'client 应声明 '+tag+' 证据标签');
 for(const text of ['已实现','已解析','语义检索'])assert(!panel.includes(text),text);
});

test('设置页含三个开关，工具页挂上面板且不新增工具项',()=>{
 for(const key of ['pluginAafImport','pluginPtOverlay','pluginPtBouncer'])assert(settings.includes("'"+key+"'"),key);
 assert(settings.includes("['plugins','模块开关']"));
 assert(toolsPage.includes('<PluginPanel/>'));
 const nav=read('src/tool-navigation.ts');
 assert.equal((nav.match(/\{id:'/g)??[]).length,9);
});
