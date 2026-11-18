//! Windows桌面集成：独占启动、路径剪贴板和资源管理器定位。
use std::{fs, path::PathBuf};

pub fn startup_guard() -> Result<Option<fs::File>,String> {
    let root=PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("无法取得本机应用数据目录")?).join("local.sfxtools.desktop");
    fs::create_dir_all(&root).map_err(|e|e.to_string())?;
    match acquire_lock(root.join("startup.lock"))?{
        Some(file)=>Ok(Some(file)),
        None=>{
            #[cfg(windows)] {
                use windows::Win32::UI::WindowsAndMessaging::*;
                // 明确弹窗：以前这里只在「找不到窗口」时提示，结果是新版双击后静默退出、
                // 用户以为程序坏了。现在两种情况都告知，并说明该怎么做。
                if let Some(window)=find_existing_window(){
                    unsafe{let _=ShowWindow(window,SW_RESTORE);let _=SetForegroundWindow(window);}
                    message("SFX TOOLS 已经在运行，已把已有窗口切到前台。\n\n如果窗口还是没有出现，请在任务栏里找回它；或先结束旧实例（含旧版本），再启动本次的程序。");
                }else{
                    message("SFX TOOLS 已经在运行或正在启动（可能是旧版本实例）。\n\n请在任务栏里找到它；或先结束旧实例（含旧版本），再启动本次的程序。");
                }
            }
            Ok(None)
        },
    }
}

// 标题随页面切换（SFX TOOLS · 音效库/音乐库/音频工具），精确匹配会失配；
// 浏览器标签页可能同名，故前缀命中后再核对进程 exe 名。
#[cfg(windows)]
fn find_existing_window()->Option<windows::Win32::Foundation::HWND>{
    use windows::{Win32::{Foundation::{CloseHandle,FALSE,HWND,LPARAM,TRUE},System::Threading::{OpenProcess,QueryFullProcessImageNameW,PROCESS_NAME_FORMAT,PROCESS_QUERY_LIMITED_INFORMATION},UI::WindowsAndMessaging::{EnumWindows,GetWindowTextW,GetWindowThreadProcessId}},core::{BOOL,PWSTR}};
    struct Search{exe:String,found:Option<HWND>}
    unsafe extern "system" fn inspect(window:HWND,param:LPARAM)->BOOL{
        let search=&mut *(param.0 as *mut Search);
        let mut title=[0u16;512];
        let len=GetWindowTextW(window,&mut title);
        if len<=0{return TRUE;}
        if !String::from_utf16_lossy(&title[..len as usize]).starts_with("SFX TOOLS"){return TRUE;}
        let mut pid=0u32;GetWindowThreadProcessId(window,Some(&mut pid));
        if pid==0{return TRUE;}
        if let Ok(process)=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,false,pid){
            let mut buffer=[0u16;1024];let mut size=buffer.len() as u32;
            if QueryFullProcessImageNameW(process,PROCESS_NAME_FORMAT(0),PWSTR(buffer.as_mut_ptr()),&mut size).is_ok(){
                use std::os::windows::ffi::OsStringExt;
                let path=std::path::PathBuf::from(std::ffi::OsString::from_wide(&buffer[..size as usize]));
                if path.file_name().is_some_and(|name|name.to_string_lossy().to_lowercase()==search.exe){
                    search.found=Some(window);let _=CloseHandle(process);return FALSE;
                }
            }
            let _=CloseHandle(process);
        }
        TRUE
    }
    let exe=std::env::current_exe().ok()?.file_name()?.to_string_lossy().to_lowercase();
    let mut search=Search{exe,found:None};
    unsafe{let _=EnumWindows(Some(inspect),LPARAM(&mut search as *mut Search as isize));}
    search.found
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
        let _=MessageBoxW(None,&HSTRING::from(text),&HSTRING::from("SFX TOOLS"),MB_OK|MB_ICONINFORMATION);
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

/// 打开外链工具：只放行 http/https，交给系统默认浏览器，不在应用内嵌网页。
/// 用 ShellExecuteW 而不是 cmd /c start，避免 URL 里的 & 被命令行二次解释。
#[tauri::command]
pub fn open_external(url:String)->Result<(),String>{
    let value=url.trim();
    if value.len()>2048{return Err("链接过长。".into());}
    if value.chars().any(|c|c.is_control()||c=='"'){return Err("链接包含非法字符。".into());}
    let scheme=value.split_once("://").map(|parts|parts.0.to_ascii_lowercase()).unwrap_or_default();
    if scheme!="http"&&scheme!="https"{return Err("只允许打开 http/https 链接。".into());}
    #[cfg(windows)] unsafe{
        use windows::{core::HSTRING,Win32::UI::Shell::ShellExecuteW,Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL};
        let result=ShellExecuteW(None,&HSTRING::from("open"),&HSTRING::from(value),None,None,SW_SHOWNORMAL);
        if result.0 as isize<=32{return Err("系统未能打开该链接，请检查默认浏览器设置。".into());}
        return Ok(());
    }
    #[cfg(not(windows))]{let _=value;Err("外链打开目前仅支持 Windows。".into())}
}

/// 打开素材文件夹（任务抽屉里的「打开目录」）：路径必须存在且为目录。
#[tauri::command]
pub fn reveal_folder(path:String)->Result<(),String>{
    let target=PathBuf::from(path.trim());
    if !target.is_absolute(){return Err("请提供绝对路径。".into());}
    if !target.is_dir(){return Err("目录不存在或不可访问。".into());}
    std::process::Command::new("explorer.exe").arg(visible_path(&target))
        .spawn().map_err(|e|format!("无法打开目录：{e}"))?;
    Ok(())
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
