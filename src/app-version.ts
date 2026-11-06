import {useEffect,useState} from 'react';

/** 版本号只有一处来源：Cargo.toml 的 version，由 Rust 命令 app_version 返回。 */
let cached='';

export function useAppVersion():string{
 const [version,setVersion]=useState(cached);
 useEffect(()=>{
  if(cached)return;
  let alive=true;
  import('@tauri-apps/api/core')
   .then(({invoke})=>invoke<string>('app_version'))
   .then(value=>{cached=value;if(alive)setVersion(value);})
   .catch(()=>{/* 浏览器预览无 Tauri 内核，保持占位 */});
  return ()=>{alive=false;};
 },[]);
 return version;
}
