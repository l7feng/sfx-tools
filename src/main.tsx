const params=new URLSearchParams(location.search);
if (params.has('probe')) { void import('./probe'); }
else if(params.has('demo')||!('__TAURI_INTERNALS__' in window)&&!params.has('catalog')) { void import('./workbench'); }
else { void import('./catalog'); }
export {};
