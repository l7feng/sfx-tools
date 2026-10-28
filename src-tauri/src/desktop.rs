//! Windows桌面集成：独占启动、路径剪贴板和资源管理器定位。
use std::{fs, path::PathBuf};

pub fn startup_guard() -> Result<Option<fs::File>,String> {
    let root=PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("无法取得本机应用数据目录")?).join("local.soundworkbench.desktop");
    fs::create_dir_all(&root).map_err(|e|e.to_string())?;
    match acquire_lock(root.join("startup.lock"))?{
        Some(file)=>Ok(Some(file)),
        None=>{
            #[cfg(windows)] unsafe {
                use windows::{core::w,Win32::UI::WindowsAndMessaging::*};
                if let Ok(window)=FindWindowW(None,w!("Sound Workbench · 素材目录")){
                    let _=ShowWindow(window,SW_RESTORE);let _=SetForegroundWindow(window);
                }else{message("Sound Workbench 已在运行或正在启动，请切换到已有窗口。");}
            }
            Ok(None)
        },
    }
}

fn acquire_lock(path:PathBuf)->Result<Option<fs::File>,String>{
    let file=fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path).map_err(|e|e.to_string())?;
    match file.try_lock(){Ok(())=>Ok(Some(file)),Err(std::fs::TryLockError::WouldBlock)=>Ok(None),Err(e)=>Err(format!("无法取得启动锁：{e}"))}
}
fn visible_path(path:&std::path::Path)->String{
    let value=path.display().to_string();
    if let Some(rest)=value.strip_prefix(r"\\?\UNC\"){format!(r"\\{rest}")}else{value.trim_start_matches(r"\\?\").to_string()}
}

pub fn message(text:&str){
    #[cfg(windows)] unsafe{
        use windows::{core::HSTRING,Win32::UI::WindowsAndMessaging::*};
        let _=MessageBoxW(None,&HSTRING::from(text),&HSTRING::from("Sound Workbench"),MB_OK|MB_ICONINFORMATION);
    }
    #[cfg(not(windows))] eprintln!("{text}");
}

#[tauri::command]
pub async fn catalog_reveal(catalog:tauri::State<'_,crate::catalog::CatalogState>,id:String)->Result<(),String>{
    let catalog=catalog.inner().clone();
    let path=tauri::async_runtime::spawn_blocking(move||catalog.resolve_asset(&id)).await.map_err(|e|e.to_string())??;
    std::process::Command::new("explorer.exe").arg(format!("/select,{}",visible_path(&path)))
        .spawn().map_err(|e|format!("无法打开文件位置：{e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn catalog_copy_paths(window:tauri::Window,catalog:tauri::State<'_,crate::catalog::CatalogState>,ids:Vec<String>)->Result<usize,String>{
    if ids.is_empty()||ids.len()>100{return Err("请选择1至100条素材。".into());}
    let catalog=catalog.inner().clone();
    let paths=tauri::async_runtime::spawn_blocking(move||ids.iter().map(|id|catalog.resolve_asset(id)).collect::<Result<Vec<_>,_>>()).await.map_err(|e|e.to_string())??;
    let text=paths.iter().map(|p|visible_path(p)).collect::<Vec<_>>().join("\r\n");
    #[cfg(windows)] unsafe {
        use windows::Win32::{Foundation::HANDLE,System::{DataExchange::*,Memory::*}};
        let data:Vec<u16>=text.encode_utf16().chain(Some(0)).collect();
        OpenClipboard(Some(window.hwnd().map_err(|e|e.to_string())?)).map_err(|e|format!("剪贴板被占用：{e}"))?;
        let result=(||{
            let memory=GlobalAlloc(GMEM_MOVEABLE,data.len()*2).map_err(|e|e.to_string())?;
            let dest=GlobalLock(memory);
            if dest.is_null(){let _=windows::Win32::Foundation::GlobalFree(Some(memory));return Err("无法分配剪贴板内存".to_string());}
            std::ptr::copy_nonoverlapping(data.as_ptr(),dest.cast::<u16>(),data.len());let _=GlobalUnlock(memory);
            if let Err(e)=EmptyClipboard().and_then(|_|SetClipboardData(13,Some(HANDLE(memory.0))).map(|_|())){let _=windows::Win32::Foundation::GlobalFree(Some(memory));return Err(e.to_string());}
            Ok(())
        })();let _=CloseClipboard();result?;
    }
    #[cfg(not(windows))] {let _=text;return Err("路径剪贴板目前仅支持Windows。".into());}
    Ok(paths.len())
}

#[cfg(test)] mod tests{
 use super::*;
 #[test] fn startup_lock_releases_and_does_not_truncate(){
  let dir=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../build/s1.3-next-validation").join(format!("startup-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&dir).unwrap();let path=dir.join("startup.lock");fs::write(&path,b"keep").unwrap();
  let first=acquire_lock(path.clone()).unwrap().unwrap();assert!(acquire_lock(path.clone()).unwrap().is_none());drop(first);assert!(acquire_lock(path.clone()).unwrap().is_some());assert_eq!(fs::read(&path).unwrap(),b"keep");fs::remove_dir_all(dir).unwrap();
 }
 #[test] fn display_paths_keep_unc_and_unicode(){
  assert_eq!(visible_path(std::path::Path::new(r"\\?\UNC\server\share\声音.wav")),r"\\server\share\声音.wav");
  assert_eq!(visible_path(std::path::Path::new(r"\\?\D:\声音.wav")),r"D:\声音.wav");
 }
}
