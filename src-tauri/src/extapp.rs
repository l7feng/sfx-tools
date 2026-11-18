//! 外部程序的通用定位与启动：管理页的「外部程序」卡片（MUS Prompt / UCS 批量命名 / Audio Dedup）。
//!
//! 与 dedup.rs 的分工：dedup 那条保留它自己的命令行契约探测（`--cli --version` 能拿到版本号），
//! 这里只做「按文件名找 exe + 记住路径 + 启动」，不假设对方支持任何命令行参数。
//!
//! 查找顺序：① 记录（`%APPDATA%\sfx-tools\extapp\<文件名>.txt`）→ ② 受限遍历。
//! 遍历仍有界：只扫少数常见安装根，深度 ≤4、节点 ≤4000，跳过链接、隐藏目录与系统目录。

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

const WALK_MAX_DEPTH: usize = 4;
const WALK_MAX_NODES: u32 = 4000;

#[derive(Serialize)]
pub struct AppProbe {
    pub path: String,
    /// 只有传了 exe 名才会去读文件版本；读不到就是空串，卡片上不显示 `v…`。
    pub version: String,
}

fn record_dir() -> PathBuf {
    let base = std::env::var("APPDATA").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
    base.join("sfx-tools").join("extapp")
}

/// 归档/备份目录里也可能留着旧构建的同名 exe；定位时直接跳过，避免把用户指到过期的程序。
fn is_ignored_dir(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("archive")
        || lower.contains("归档")
        || lower.contains("重建前")
        || lower.contains(".bak")
        || lower.contains("backup")
        || lower.contains("历史版本")
}

fn check_exe(path: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(path.trim().trim_matches('"'));
    if !p.is_absolute() {
        return Err("外部程序必须是绝对路径。".into());
    }
    let meta = std::fs::symlink_metadata(&p).map_err(|e| format!("外部程序不可访问：{e}"))?;
    if !meta.is_file() {
        return Err("外部程序不是文件。".into());
    }
    if p.extension().and_then(|v| v.to_str()).map(|v| v.eq_ignore_ascii_case("exe")) != Some(true) {
        return Err("只接受 .exe 作为外部程序。".into());
    }
    Ok(p)
}

fn record_path(name: &str) -> PathBuf {
    let safe: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' })
        .collect();
    record_dir().join(format!("{safe}.txt"))
}

fn read_record(name: &str) -> Option<PathBuf> {
    let raw = std::fs::read_to_string(record_path(name)).ok()?;
    check_exe(raw.trim()).ok()
}

fn write_record(name: &str, path: &Path) -> Result<(), String> {
    let dir = record_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("无法建立记录目录：{e}"))?;
    std::fs::write(record_path(name), path.to_string_lossy().as_bytes())
        .map_err(|e| format!("无法记住外部程序路径：{e}"))
}

fn walk_for_exe(dir: &Path, name: &str, depth: usize, budget: &mut u32) -> Option<PathBuf> {
    if *budget == 0 || depth > WALK_MAX_DEPTH { return None; }
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        if *budget == 0 { return None; }
        *budget -= 1;
        let path = entry.path();
        let meta = match std::fs::symlink_metadata(&path) { Ok(m) => m, Err(_) => continue };
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 { continue; }
        }
        if meta.is_file() {
            if path.file_name().and_then(|v| v.to_str()).is_some_and(|v| v.eq_ignore_ascii_case(name)) {
                return Some(path);
            }
        } else if meta.is_dir() {
            let dir_name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
            if dir_name.starts_with('.')
                || dir_name.eq_ignore_ascii_case("Windows")
                || dir_name.eq_ignore_ascii_case("node_modules")
                || is_ignored_dir(dir_name)
            {
                continue;
            }
            if let Some(found) = walk_for_exe(&path, name, depth + 1, budget) { return Some(found); }
        }
    }
    None
}

fn search_exe(name: &str) -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    roots.push(PathBuf::from(r"D:\Ai-Files\Tools\Exe"));
    for key in ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Ok(value) = std::env::var(key) {
            let base = PathBuf::from(value);
            roots.push(base.join("Programs"));
            roots.push(base);
        }
    }
    for root in roots {
        if !root.is_dir() { continue; }
        let mut budget = WALK_MAX_NODES;
        if let Some(found) = walk_for_exe(&root, name, 0, &mut budget) { return Some(found); }
    }
    None
}

/// Windows 文件版本（资源里的 ProductVersion）；取不到就返回空串，不当作错误。
fn file_version(path: &Path) -> String {
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::{
            GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
        };

        // VS_FIXEDFILEINFO 的布局固定：dwProductVersionMS 在 +16，LS 在 +20（各 4 字节）。
        const PRODUCT_MS: usize = 16;
        const PRODUCT_LS: usize = 20;

        let wide: Vec<u16> = OsStr::new(path).encode_wide().chain(Some(0)).collect();
        let root: Vec<u16> = vec![0x005C, 0x0000]; // "\"
        unsafe {
            let mut handle: u32 = 0;
            let size = GetFileVersionInfoSizeW(PCWSTR(wide.as_ptr()), Some(&mut handle));
            if size == 0 {
                return String::new();
            }
            let mut buf = vec![0u8; size as usize];
            if GetFileVersionInfoW(PCWSTR(wide.as_ptr()), None, size, buf.as_mut_ptr().cast()).is_err() {
                return String::new();
            }
            let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
            let mut len: u32 = 0;
            if !VerQueryValueW(buf.as_ptr().cast(), PCWSTR(root.as_ptr()), &mut ptr, &mut len).as_bool()
                || ptr.is_null()
                || (len as usize) < PRODUCT_LS + 4
            {
                return String::new();
            }
            let base = ptr as *const u8;
            let ms = u32::from_ne_bytes([
                *base.add(PRODUCT_MS),
                *base.add(PRODUCT_MS + 1),
                *base.add(PRODUCT_MS + 2),
                *base.add(PRODUCT_MS + 3),
            ]);
            let ls = u32::from_ne_bytes([
                *base.add(PRODUCT_LS),
                *base.add(PRODUCT_LS + 1),
                *base.add(PRODUCT_LS + 2),
                *base.add(PRODUCT_LS + 3),
            ]);
            return format!(
                "{}.{}.{}.{}",
                (ms >> 16) & 0xFFFF,
                ms & 0xFFFF,
                (ls >> 16) & 0xFFFF,
                ls & 0xFFFF
            );
        }
        #[allow(unreachable_code)]
        String::new()
    }
    #[cfg(not(windows))]
    { let _ = path; String::new() }
}

/// 自动定位：先读记录，再按文件名做有界遍历；找到就顺手读一次文件版本。
#[tauri::command]
pub async fn extapp_locate(name: String) -> Result<AppProbe, String> {
    let name = name.trim().to_string();
    if name.is_empty() { return Err("缺少程序文件名。".into()); }
    tauri::async_runtime::spawn_blocking(move || {
        let path = read_record(&name)
            .or_else(|| search_exe(&name))
            .ok_or_else(|| format!("没有找到 {name}。点一次选择即可，之后会自动记住。"))?;
        let _ = write_record(&name, &path);
        let version = file_version(&path);
        Ok(AppProbe { path: path.to_string_lossy().to_string(), version })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 记住手动选中的路径。
#[tauri::command]
pub fn extapp_remember(name: String, path: String) -> Result<(), String> {
    let checked = check_exe(&path)?;
    write_record(&name, &checked)
}

/// 启动：工作目录设成 exe 所在目录，避免 Electron/Python 打包程序找不到自己的资源。
#[tauri::command]
pub fn extapp_launch(exe: String) -> Result<(), String> {
    let path = check_exe(&exe)?;
    let mut cmd = Command::new(&path);
    if let Some(dir) = path.parent() { cmd.current_dir(dir); }
    cmd.spawn().map_err(|e| format!("无法启动外部程序：{e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_relative_and_missing_paths() {
        assert!(check_exe("relative.exe").is_err());
        assert!(check_exe(r"D:\__none__\nope.exe").is_err());
    }

    #[test]
    fn record_path_is_sanitized_and_absolute() {
        let p = record_path("UCS Namer.exe");
        assert!(p.is_absolute());
        assert!(p.to_string_lossy().contains("sfx-tools"));
        assert!(!p.to_string_lossy().contains(' '));
    }

    #[test]
    fn search_finds_mus_prompt_when_tools_root_exists() {
        if !Path::new(r"D:\Ai-Files\Tools\Exe").is_dir() { return; }
        let found = search_exe("MUS Prompt.exe");
        assert!(found.is_some(), "受限遍历应该能在工具目录里找到 MUS Prompt.exe");
    }

    #[test]
    fn search_skips_archive_and_backup_directories() {
        if !Path::new(r"D:\Ai-Files\Tools\Exe").is_dir() { return; }
        let found = search_exe("audio-dedup-gui.exe").expect("工具目录里应该有 Audio Dedup");
        let path = found.to_string_lossy().to_lowercase();
        assert!(!path.contains("archive") && !path.contains("归档") && !path.contains("重建前"),
            "定位不应当落到归档目录：{path}");
    }
}
