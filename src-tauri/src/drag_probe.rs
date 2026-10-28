//! Windows copy-only drag shared by the sample probe and registered catalog assets.
//! API checked against crabnebula-dev/drag-rs v2, drag 2.1.1.
//! The upstream Windows implementation maps Copy to DROPEFFECT_COPY.

use std::path::{Path, PathBuf};

fn validated_sample(path: &str) -> Result<PathBuf, String> {
    let requested = Path::new(path);
    if !requested.is_absolute() {
        return Err("请提供测试样本的完整路径。".into());
    }
    let sample = requested
        .canonicalize()
        .map_err(|e| format!("测试样本不可访问：{e}"))?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../build/s1-samples")
        .canonicalize()
        .map_err(|e| format!("专用测试样本目录不可访问：{e}"))?;
    if !sample.starts_with(&root) || !sample.is_file() {
        return Err("此验证仅允许拖出工程 build/s1-samples 内的普通测试文件。".into());
    }
    Ok(sample)
}

#[cfg(target_os = "windows")]
mod windows {
    use std::path::PathBuf;
    use std::sync::{atomic::{AtomicBool, Ordering}, mpsc};

    static DRAG_ACTIVE: AtomicBool = AtomicBool::new(false);

    struct DragGuard;
    impl Drop for DragGuard {
        fn drop(&mut self) {
            DRAG_ACTIVE.store(false, Ordering::Release);
        }
    }

    // Invoke on pointer-down while the left mouse button is still pressed.
    // OLE must run on Tauri's UI thread; only waiting for its result runs elsewhere.
    pub(super) async fn start(
        app: tauri::AppHandle,
        window: tauri::Window,
        samples: Vec<PathBuf>,
    ) -> Result<String, String> {
        DRAG_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "已有拖出操作进行中。".to_string())?;
        let _guard = DragGuard;
        let (tx, rx) = mpsc::channel();
        app.run_on_main_thread(move || {
            let callback_tx = tx.clone();
            let result = drag::start_drag(
                &window,
                drag::DragItem::Files(samples),
                // Windows treats an unavailable preview as optional; retain the OS cursor.
                drag::Image::Raw(Vec::new()),
                move |result, _position| {
                    let message = match result {
                        drag::DragResult::Dropped => "系统报告已放下文件；请核对目标文件是否完整。",
                        drag::DragResult::Cancel => "拖出已取消；未报告目标接收成功。",
                    };
                    let _ = callback_tx.send(Ok(message.to_string()));
                },
                drag::Options {
                    mode: drag::DragMode::Copy,
                    ..Default::default()
                },
            );
            if let Err(error) = result {
                let _ = tx.send(Err(format!("原生拖出失败：{error}")));
            }
        })
        .map_err(|e| format!("无法调度原生拖出：{e}"))?;
        tauri::async_runtime::spawn_blocking(move || {
            rx.recv().map_err(|_| "拖出结果通道已关闭。".to_string())?
        })
        .await
        .map_err(|e| format!("等待拖出结果失败：{e}"))?
    }
}

#[tauri::command]
pub async fn drag_sample(
    app: tauri::AppHandle,
    window: tauri::Window,
    path: String,
) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    { windows::start(app, window, vec![validated_sample(&path)?]).await }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, window, path);
        Err("此阶段仅验证Windows普通文件拖出。".into())
    }
}

#[tauri::command]
pub async fn catalog_drag_out(
    app: tauri::AppHandle,
    window: tauri::Window,
    catalog: tauri::State<'_, crate::catalog::CatalogState>,
    id: String,
) -> Result<String, String> {
    let catalog=catalog.inner().clone();
    let path = tauri::async_runtime::spawn_blocking(move||catalog.resolve_asset(&id)).await.map_err(|e|e.to_string())??;
    #[cfg(target_os = "windows")]
    { windows::start(app, window, vec![path]).await }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, window, path);
        Err("素材拖出目前仅支持Windows。".into())
    }
}

fn registered_paths(catalog:&crate::catalog::CatalogState,ids:Vec<String>)->Result<Vec<PathBuf>,String>{
    if ids.is_empty()||ids.len()>100{return Err("请一次拖出1至100条素材。".into());}
    let mut unique=std::collections::HashSet::new();
    ids.into_iter().filter(|id|unique.insert(id.clone())).map(|id|catalog.resolve_asset(&id)).collect()
}

#[tauri::command]
pub async fn catalog_drag_many(app:tauri::AppHandle,window:tauri::Window,catalog:tauri::State<'_,crate::catalog::CatalogState>,ids:Vec<String>)->Result<String,String>{
    let catalog=catalog.inner().clone();
    let paths=tauri::async_runtime::spawn_blocking(move||registered_paths(&catalog,ids)).await.map_err(|e|e.to_string())??;
    #[cfg(windows)] {windows::start(app,window,paths).await}
    #[cfg(not(windows))] {let _=(app,window,paths);Err("素材拖出目前仅支持Windows。".into())}
}

#[cfg(test)]
mod tests {
    use super::validated_sample;

    #[test]
    fn accepts_generated_fixture() {
        let sample = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../build/s1-samples/pcm-44100.wav");
        assert!(validated_sample(sample.to_str().unwrap()).is_ok());
    }

    #[test]
    fn rejects_relative_paths() {
        assert!(validated_sample("pcm-44100.wav").is_err());
    }

    #[test]
    fn rejects_existing_files_outside_fixture_directory() {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert!(validated_sample(manifest.to_str().unwrap()).is_err());
    }
}
