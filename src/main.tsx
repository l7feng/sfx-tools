const params=new URLSearchParams(location.search);
if (params.has('probe')) { void import('./probe'); }
// 重复组工作台：独立窗口，由 Rust 用 initialization_script 注入 window.__DEDUP_ARG。
else if (params.has('workbench') || '__DEDUP_ARG' in window) { void import('./plugins/dedup-workbench'); }
else if(params.has('demo')||!('__TAURI_INTERNALS__' in window)&&!params.has('catalog')) { void import('./workbench'); }
else { void import('./catalog'); }
export {};
