//! Windows 文件夹选择。只返回用户选中的路径，不枚举、导入或改写文件。

#[cfg(target_os = "windows")]
mod native {
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows::{
        core::{HRESULT, HSTRING, PWSTR},
        Win32::{
            Foundation::{ERROR_CANCELLED, HWND},
            System::Com::{
                CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize,
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
            },
            UI::Shell::{
                FileOpenDialog, IFileOpenDialog, IShellItem, SHCreateItemFromParsingName,
                FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
            },
        },
    };

    static PICKER_ACTIVE: AtomicBool = AtomicBool::new(false);

    struct ActiveGuard;
    impl Drop for ActiveGuard {
        fn drop(&mut self) { PICKER_ACTIVE.store(false, Ordering::Release); }
    }

    // 此对象先于所有 COM 接口构造，因而最后析构，确保接口先释放。
    struct ApartmentGuard;
    impl Drop for ApartmentGuard {
        fn drop(&mut self) { unsafe { CoUninitialize(); } }
    }

    struct ShellString(PWSTR);
    impl Drop for ShellString {
        fn drop(&mut self) { unsafe { CoTaskMemFree(Some(self.0.0.cast())); } }
    }

    fn choose_on_sta(owner: usize, initial: Option<String>) -> Result<Option<String>, String> {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()
                .map_err(|e| format!("初始化目录选择器失败：{e}"))?;
            let _apartment = ApartmentGuard;
            let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("创建目录选择器失败：{e}"))?;
            let options = dialog.GetOptions().map_err(|e| e.to_string())?;
            dialog.SetOptions(options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST)
                .map_err(|e| e.to_string())?;
            dialog.SetTitle(&HSTRING::from("选择音效素材文件夹"))
                .map_err(|e| e.to_string())?;
            if let Some(path) = initial {
                let path = path.trim().trim_matches('"');
                // 上次位置已离线或被移走时，仍允许选择新目录。
                if std::path::Path::new(path).is_dir() {
                    if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(path), None) {
                        let _ = dialog.SetFolder(&item);
                    }
                }
            }
            match dialog.Show(Some(HWND(owner as *mut core::ffi::c_void))) {
                Ok(()) => {},
                Err(e) if e.code() == HRESULT::from_win32(ERROR_CANCELLED.0) => return Ok(None),
                Err(e) => return Err(format!("目录选择失败：{e}")),
            }
            let item = dialog.GetResult().map_err(|e| format!("无法读取选中目录：{e}"))?;
            let path = ShellString(item.GetDisplayName(SIGDN_FILESYSPATH)
                .map_err(|e| format!("选中项不是文件系统目录：{e}"))?);
            let value = path.0.to_string().map_err(|e| format!("目录路径编码无法读取：{e}"))?;
            Ok(Some(value))
        }
    }

    pub(super) async fn choose(window: tauri::Window, initial: Option<String>) -> Result<Option<String>, String> {
        PICKER_ACTIVE.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "目录选择窗口已经打开。".to_string())?;
        let guard = ActiveGuard;
        let owner = window.hwnd().map_err(|e| format!("无法取得工作台窗口：{e}"))?.0 as usize;
        // 不在已有后台线程初始化 COM：该线程可能已使用其他 apartment 模式。
        // 只把纯 Rust 值传入专用 STA 线程，COM 接口在该线程内创建并释放。
        let thread = std::thread::Builder::new().name("catalog-folder-picker".into())
            .spawn(move || { let _guard = guard; choose_on_sta(owner, initial) })
            .map_err(|e| format!("无法启动目录选择器：{e}"))?;
        tauri::async_runtime::spawn_blocking(move || thread.join()
            .map_err(|_| "目录选择器线程异常退出。".to_string())?)
            .await.map_err(|e| format!("等待目录选择结果失败：{e}"))?
    }
}

#[tauri::command]
pub async fn catalog_pick_folder(window: tauri::Window, initial: Option<String>) -> Result<Option<String>, String> {
    #[cfg(target_os = "windows")]
    { native::choose(window, initial).await }
    #[cfg(not(target_os = "windows"))]
    { let _ = (window, initial); Err("当前目录选择器仅支持 Windows。".into()) }
}
