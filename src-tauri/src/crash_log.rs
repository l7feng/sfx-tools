//! 前端崩溃日志落盘。
//!
//! 为什么需要：界面跑在 WebView2 里，一旦渲染异常就是整窗白屏，
//! 控制台在打包后看不到，用户只能描述「打不开」。这里把
//! window.onerror / unhandledrejection / React 渲染异常统一追加到一个文本文件，
//! 出问题时直接把这个文件发出来即可。
//!
//! 边界：只追加纯文本，不解析内容、不上报网络、不含素材数据。
//! 体积：超过 1 MB 滚一次（crash.log → crash.log.1），只留一份备份。

use std::io::Write;

fn log_dir() -> Result<std::path::PathBuf, String> {
    let base = std::env::var("APPDATA")
        .map_err(|_| "读不到 APPDATA，无法确定崩溃日志目录。".to_string())?;
    let dir = std::path::Path::new(&base).join("sfx-tools");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

const MAX_BYTES: u64 = 1024 * 1024;

/// 往指定目录追加一条日志，返回写入的绝对路径。
/// 抽成独立函数是为了让测试不碰全局 APPDATA 环境变量（并发跑测试时改环境变量会互相干扰）。
fn append_in(dir: &std::path::Path, text: &str) -> Result<String, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join("crash.log");
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > MAX_BYTES {
            let rotated = dir.join("crash.log.1");
            std::fs::rename(&path, &rotated).map_err(|e| e.to_string())?;
        }
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    file.write_all(b"\n").map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

/// 追加一条日志到 `%APPDATA%\sfx-tools\crash.log`，返回最终路径（供界面显示「日志已写到 …」）。
pub fn append(text: &str) -> Result<String, String> {
    append_in(&log_dir()?, text)
}

#[tauri::command]
pub async fn crash_log(text: String) -> Result<String, String> {
    // 日志可能有几十 KB，放线程池写，不挡界面线程。
    tauri::async_runtime::spawn_blocking(move || append(&text))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 两条日志都落进同一个文件；超过 1MB 时旧文件被滚成 crash.log.1。
    #[test]
    fn append_keeps_both_lines_and_rotates_when_large() {
        let dir = std::env::temp_dir().join(format!("sw-crash-{}", uuid::Uuid::new_v4()));
        let first = append_in(&dir, "[t1] render boom").unwrap();
        let second = append_in(&dir, "[t2] promise boom").unwrap();
        assert_eq!(first, second, "两次写入应指向同一个文件");
        let body = std::fs::read_to_string(&first).unwrap();
        assert!(body.contains("render boom"));
        assert!(body.contains("promise boom"));

        std::fs::write(dir.join("crash.log"), vec![b'x'; (MAX_BYTES + 1) as usize]).unwrap();
        append_in(&dir, "[t3] after rotate").unwrap();
        assert!(dir.join("crash.log.1").exists(), "超限时应先把旧文件滚成 crash.log.1");
        let body = std::fs::read_to_string(dir.join("crash.log")).unwrap();
        assert!(body.contains("after rotate"), "滚动后新日志只含刚写的一条");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
