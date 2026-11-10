// 离线组件渲染测试，不访问浏览器、声卡、应用数据库或原生命令。
import test from 'node:test';
import assert from 'node:assert/strict';
import {build} from 'vite';
import React from 'react';
import {renderToStaticMarkup} from 'react-dom/server';
import {pathToFileURL} from 'node:url';
import path from 'node:path';
globalThis.localStorage={getItem:()=>null};
await build({logLevel:'silent',build:{ssr:'src/tools-page.tsx',outDir:'build/tool-navigation-validation/ssr',emptyOutDir:true,minify:false}});
const {ToolsPage}=await import(pathToFileURL(path.resolve('build/tool-navigation-validation/ssr/tools-page.js')));
const noop=()=>{};
const base={native:false,ids:[],selected:null,library:'sound',query:'',active:null,onNavigate:noop,onClearQuery:noop,onBack:noop,onPick:noop,onChanged:noop,onCheck:noop,onJobs:noop,busy:false};
const render=props=>renderToStaticMarkup(React.createElement(ToolsPage,{...base,...props}));
test('管理窗口渲染六个分类及无选择提示，工具窗口渲染三个制作分类',()=>{
 const html=render({});
 assert.equal((html.match(/aria-pressed="false"/g)??[]).slice(0,6).length,6);
 const nav=html.match(/<nav[^>]*aria-label="音频工具分类"[^>]*>(.*?)<\/nav>/s)?.[1];
 assert(nav);assert.equal((nav.match(/<button/g)??[]).length,6);
 for(const label of ['UCS 分类建议','重复素材','叠层编排','账号与授权'])assert(nav.includes(label));
 assert(html.includes('尚未选择素材'));assert(html.includes('去素材库选择'));assert(html.includes('<h1>管理</h1>'));
 const production=render({area:'production'});
 const prodNav=production.match(/<nav[^>]*aria-label="音频工具分类"[^>]*>(.*?)<\/nav>/s)?.[1];
 assert(prodNav);assert.equal((prodNav.match(/<button/g)??[]).length,3);
 for(const label of ['批量导出与转换','插件与制作连接','Pro Tools 导入预览'])assert(prodNav.includes(label));
 assert(production.includes('<h1>工具</h1>'));
});
test('分类直达展开相应制作操作，账号直达展开而非空壳标题',()=>{
 const convert=render({active:'convert',area:'production'});
 assert.match(convert,/<details open=""><summary>批量导出与高质量转换/);
 assert.match(convert,/<details open="" hidden=""><summary>UCS/);
 assert(convert.includes('请先返回素材库选择1至500条素材'));
 const account=render({active:'account'});
 assert(account.includes('开发服务与受信公钥'));assert(account.includes('测试账号'));
});
test('搜索无结果不吞掉已打开面板，跨分类保留显式选择信息',()=>{
 const html=render({active:'ucs',query:'不存在的工具名123',ids:['x'],selected:{id:'x',name:'参考音效.wav'}});
 assert(html.includes('没有匹配的工具'));assert(html.includes('清除工具搜索'));
 assert(html.includes('不会中断当前任务'));assert(html.includes('已选 1 条素材'));assert(html.includes('参考音效.wav'));
 assert.match(html,/<details open=""><summary>UCS/);
});
