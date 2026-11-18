//! 外部去重工具（Audio Dedup）的接入层：方案 A「外部工具契约」。
//!
//! 边界：
//! - 只用命令行调用用户选定的 exe，参数逐个作为 argv 传入，**不经 shell、不拼字符串命令**；
//! - 本程序只读它产出的 `groups.json`，不碰源音频、不改索引；
//! - 不设任何删除/隔离能力：隔离仍由去重工具自己的 GUI 里的人类确认流程完成。
//!
//! 契约（Audio Dedup 0.1.x）：
//!   探测  `audio-dedup-gui.exe --cli --version`            → 版本号
//!   扫描  `... --cli scan --root <库根> --workdir <索引目录>`  → 产出 groups.json
//!   L3    `... --cli l3 --workdir <索引目录>`
//!   报告  `... --cli report --workdir <索引目录>`

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

const MAX_JSON_BYTES: u64 = 40 * 1024 * 1024;
const TAIL_CHARS: usize = 60_000;

#[derive(Serialize)]
pub struct DedupProbe {
    pub version: String,
    pub path: String,
}

#[derive(Serialize)]
pub struct DedupRun {
    pub ok: bool,
    pub code: Option<i32>,
    pub mode: String,
    pub workdir: String,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub groups_path: String,
}

#[derive(Serialize,Debug)]
pub struct DedupFile {
    pub path: String,
    pub size: u64,
    pub keep: bool,
    pub grade: String,
}

#[derive(Serialize,Debug)]
pub struct DedupGroup {
    pub id: String,
    pub layer: String,
    pub grade: String,
    pub count: usize,
    pub keep: String,
    pub reclaimable_bytes: u64,
    pub files: Vec<DedupFile>,
}

#[derive(Serialize,Debug)]
pub struct DedupSummary {
    pub schema: u64,
    pub generated_at: String,
    pub root: String,
    pub workdir: String,
    pub l1_groups: u64,
    pub l2_groups: u64,
    pub l3_groups: u64,
    pub edges: u64,
    pub total_groups: u64,
    pub reclaimable_bytes: u64,
    pub scanned_files: u64,
    pub errors: Vec<String>,
    pub groups: Vec<DedupGroup>,
    pub truncated: bool,
}

fn default_workdir() -> PathBuf {
    let base = std::env::var("APPDATA").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
    base.join("sfx-tools").join("dedup")
}

fn check_exe(path: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(path.trim().trim_matches('"'));
    if !p.is_absolute() {
        return Err("去重程序必须是绝对路径。".into());
    }
    let meta = std::fs::symlink_metadata(&p).map_err(|e| format!("去重程序不可访问：{e}"))?;
    if !meta.is_file() {
        return Err("去重程序不是文件。".into());
    }
    if p.extension().and_then(|v| v.to_str()).map(|v| v.eq_ignore_ascii_case("exe")) != Some(true) {
        return Err("只接受 .exe 作为去重程序。".into());
    }
    Ok(p)
}

fn check_dir(path: &str, label: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(path.trim().trim_matches('"'));
    if !p.is_absolute() {
        return Err(format!("{label}必须是绝对路径。"));
    }
    let meta = std::fs::metadata(&p).map_err(|e| format!("{label}不可访问：{e}"))?;
    if !meta.is_dir() {
        return Err(format!("{label}不是目录。"));
    }
    Ok(p)
}

fn tail(text: &[u8]) -> String {
    let s = String::from_utf8_lossy(text).to_string();
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= TAIL_CHARS { s } else { chars[chars.len() - TAIL_CHARS..].iter().collect() }
}

fn probe_exe(path: &Path) -> Result<DedupProbe, String> {
    let out = Command::new(path).arg("--cli").arg("--version").output()
        .map_err(|e| format!("无法启动去重程序：{e}"))?;
    if !out.status.success() {
        return Err(format!("去重程序不支持 --cli（退出码 {:?}）：{}", out.status.code(), tail(&out.stderr)));
    }
    let version = String::from_utf8_lossy(&out.stdout).trim().lines().next().unwrap_or("").trim().to_string();
    if version.is_empty() {
        return Err("去重程序没有返回版本号，无法确认命令行契约。".into());
    }
    Ok(DedupProbe { version, path: path.to_string_lossy().to_string() })
}

/// 探测：跑一次 `--cli --version`，确认这个 exe 真的支持命令行契约。
#[tauri::command]
pub async fn dedup_probe(exe: String) -> Result<DedupProbe, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = check_exe(&exe)?;
        probe_exe(&path)
    }).await.map_err(|e| e.to_string())?
}

/// 运行：扫描（L1/L2）、L3 感知、或重写报告；返回退出码与输出尾部。
#[tauri::command]
pub async fn dedup_run(exe: String, root: String, workdir: Option<String>, mode: String) -> Result<DedupRun, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let exe = check_exe(&exe)?;
        let workdir = match workdir.filter(|v| !v.trim().is_empty()) {
            Some(v) => v.trim().trim_matches('"').to_string(),
            None => default_workdir().to_string_lossy().to_string(),
        };
        let wd = Path::new(&workdir);
        if !wd.is_absolute() { return Err("索引目录必须是绝对路径。".into()); }
    std::fs::create_dir_all(wd).map_err(|e| format!("无法建立索引目录：{e}"))?;
        let mut cmd = Command::new(&exe);
        cmd.arg("--cli");
        match mode.as_str() {
            "scan" | "l1" => {
                let root_dir = check_dir(&root, "扫描根")?;
                cmd.arg("scan").arg("--root").arg(&root_dir).arg("--workdir").arg(wd);
                if mode == "l1" { cmd.arg("--skip-l2"); }
            }
            "l3" => { cmd.arg("l3").arg("--workdir").arg(wd); }
            "report" => { cmd.arg("report").arg("--workdir").arg(wd); }
            other => return Err(format!("未知的去重动作：{other}")),
        }
        // 不设外壳、不拼命令行字符串；cwd 放在索引目录，避免产物落到别处。
        cmd.current_dir(wd);
        let out = cmd.output().map_err(|e| format!("运行去重程序失败：{e}"))?;
        let groups_path = wd.join("groups.json");
        Ok(DedupRun {
            ok: out.status.success(),
            code: out.status.code(),
            mode,
            workdir: wd.to_string_lossy().to_string(),
            stdout_tail: tail(&out.stdout),
            stderr_tail: tail(&out.stderr),
            groups_path: groups_path.to_string_lossy().to_string(),
        })
    }).await.map_err(|e| e.to_string())?
}

fn num(value: &serde_json::Value, key: &str) -> u64 {
    value.get(key).and_then(|v| v.as_u64()).unwrap_or(0)
}

/// 读 groups.json：只取汇总与分组骨架，每个组只带前 30 个文件，避免一次把整份索引塞进界面。
#[tauri::command]
pub fn dedup_groups(workdir: Option<String>) -> Result<DedupSummary, String> {
    let wd = match workdir.filter(|v| !v.trim().is_empty()) {
        Some(v) => PathBuf::from(v.trim().trim_matches('"')),
        None => default_workdir(),
    };
    let file = wd.join("groups.json");
    let meta = std::fs::symlink_metadata(&file).map_err(|_| "还没有分组结果：先运行一次扫描。".to_string())?;
    if !meta.is_file() { return Err("分组结果不是文件。".into()); }
    if meta.len() > MAX_JSON_BYTES { return Err("分组结果过大（>40MB），请直接用去重工具自己的界面查看。".into()); }
    let raw = std::fs::read_to_string(&file).map_err(|e| format!("读取分组结果失败：{e}"))?;
    let doc: serde_json::Value = serde_json::from_str(&raw).map_err(|e| format!("分组结果不是有效 JSON：{e}"))?;
    let summary = doc.get("summary").cloned().unwrap_or(serde_json::Value::Null);
    let counts = doc.get("counts").cloned().unwrap_or(serde_json::Value::Null);
    let mut groups = Vec::new();
    let mut truncated = false;
    if let Some(list) = doc.get("groups").and_then(|v| v.as_array()) {
        for item in list.iter().take(200) {
            let files_raw = item.get("files").and_then(|v| v.as_array());
            let take = files_raw.map(|f| f.len().min(30)).unwrap_or(0);
            if files_raw.map(|f| f.len() > 30).unwrap_or(false) { truncated = true; }
            let files = files_raw.map(|f| f.iter().take(take).map(|file| DedupFile {
                path: file.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                size: num(file, "size"),
                keep: file.get("keep").and_then(|v| v.as_bool()).unwrap_or(false),
                grade: file.get("ber").and_then(|v| v.as_f64()).map(|b| format!("{b:.3}")).unwrap_or_else(|| "—".into()),
            }).collect()).unwrap_or_default();
            groups.push(DedupGroup {
                id: item.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                layer: item.get("layer").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                grade: item.get("grade").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                count: num(item, "count") as usize,
                keep: item.get("keep").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                reclaimable_bytes: num(item, "reclaimable_bytes"),
                files,
            });
        }
        if list.len() > 200 { truncated = true; }
    }
    Ok(DedupSummary {
        schema: num(&doc, "schema"),
        generated_at: doc.get("generated_at").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        root: doc.get("root").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        workdir: doc.get("workdir").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        l1_groups: num(&summary, "l1_groups"),
        l2_groups: num(&summary, "l2_groups"),
        l3_groups: num(&summary, "l3_groups"),
        edges: num(&summary, "edges"),
        total_groups: num(&summary, "groups"),
        reclaimable_bytes: num(&summary, "reclaimable_bytes"),
        scanned_files: num(&counts, "ok") + num(&counts, "error"),
        errors: doc.get("errors").and_then(|v| v.as_array())
            .map(|e| e.iter().take(20).filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default(),
        groups,
        truncated,
    })
}

// ---------- 重复组工作台：按绝对路径的只读通道 ----------
//
// 波形与试听原本只收素材库 id；去重结果里的文件是库外绝对路径，因此这里补两条只读命令。
// 硬约束：路径必须在 `groups.json` 的 files 里出现过（白名单按 (workdir, groups.json 长度+时间戳) 缓存），
// 且必须是存在的普通文件；不接受界面上随便传路径，也不提供任何写入。

pub struct DedupPathState {
    cache: std::sync::Arc<std::sync::Mutex<Option<AllowCache>>>,
}

struct AllowCache {
    workdir: PathBuf,
    stamp: (u64, u128),
    allow: std::collections::HashSet<String>,
}

impl Default for DedupPathState {
    fn default() -> Self {
        Self { cache: std::sync::Arc::new(std::sync::Mutex::new(None)) }
    }
}

fn norm_path(value: &str) -> String {
    value.trim().trim_matches('"').replace('/', "\\").to_lowercase()
}

fn groups_stamp(file: &Path) -> Result<(u64, u128), String> {
    let meta = std::fs::metadata(file).map_err(|_| "还没有分组结果：先运行一次扫描。".to_string())?;
    if !meta.is_file() { return Err("分组结果不是文件。".into()); }
    if meta.len() > MAX_JSON_BYTES { return Err("分组结果过大（>40MB），请直接用去重工具自己的界面查看。".into()); }
    let ns = meta.modified().ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos()).unwrap_or(0);
    Ok((meta.len(), ns))
}

fn build_allow(file: &Path) -> Result<std::collections::HashSet<String>, String> {
    let raw = std::fs::read_to_string(file).map_err(|e| format!("读取分组结果失败：{e}"))?;
    let doc: serde_json::Value = serde_json::from_str(&raw).map_err(|e| format!("分组结果不是有效 JSON：{e}"))?;
    let mut allow = std::collections::HashSet::new();
    if let Some(groups) = doc.get("groups").and_then(|v| v.as_array()) {
        for group in groups {
            if let Some(files) = group.get("files").and_then(|v| v.as_array()) {
                for file in files {
                    if let Some(path) = file.get("path").and_then(|v| v.as_str()) {
                        if !path.trim().is_empty() { allow.insert(norm_path(path)); }
                    }
                }
            }
        }
    }
    Ok(allow)
}

/// 校验一个路径确实是本次分组结果里的成员；返回可打开的实体路径。
fn check_member(workdir: &Path, raw: &str, cache: &std::sync::Arc<std::sync::Mutex<Option<AllowCache>>>) -> Result<PathBuf, String> {
    if raw.trim().is_empty() { return Err("文件路径为空。".into()); }
    let candidate = PathBuf::from(raw.trim().trim_matches('"'));
    if !candidate.is_absolute() { return Err("只接受绝对路径。".into()); }
    let file = workdir.join("groups.json");
    let stamp = groups_stamp(&file)?;
    let mut guard = cache.lock().map_err(|_| "路径白名单状态不可用".to_string())?;
    let allow = match guard.as_ref() {
        Some(cache) if cache.workdir == workdir && cache.stamp == stamp => cache.allow.clone(),
        _ => {
            let allow = build_allow(&file)?;
            *guard = Some(AllowCache { workdir: workdir.to_path_buf(), stamp, allow: allow.clone() });
            allow
        }
    };
    if !allow.contains(&norm_path(raw)) {
        return Err("该文件不在当前 groups.json 的分组里，无法读取。".into());
    }
    let meta = std::fs::symlink_metadata(&candidate).map_err(|e| format!("文件不可访问：{e}"))?;
    if !meta.is_file() { return Err("目标不是普通文件。".into()); }
    // 目录联接/符号链接一律拒绝，避免白名单校验被链接绕过。
    for part in candidate.ancestors() {
        if let Ok(meta) = std::fs::symlink_metadata(part) {
            #[cfg(windows)]
            { use std::os::windows::fs::MetadataExt; if meta.file_attributes() & 0x400 != 0 { return Err("不接受链接或目录联接。".into()); } }
            #[cfg(not(windows))]
            if meta.file_type().is_symlink() { return Err("不接受符号链接。".into()); }
        }
    }
    Ok(candidate)
}

fn resolve_workdir(workdir: &str) -> Result<PathBuf, String> {
    let trimmed = workdir.trim().trim_matches('"');
    if trimmed.is_empty() { return Ok(default_workdir()); }
    check_dir(trimmed, "索引目录")
}

/// 只读波形：外部绝对路径 → 峰值包络（走现有解码与缓存管线）。
#[tauri::command]
pub async fn dedup_waveform(workdir: String, path: String, simple: Option<bool>, state: tauri::State<'_, DedupPathState>)
    -> Result<crate::player::Waveform, String> {
    let wd = resolve_workdir(&workdir)?;
    let cache = state.cache.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let file = check_member(&wd, &path, &cache)?;
        let simple = simple.unwrap_or(false);
        let cache = crate::settings_ops::cache_enabled().then(|| crate::settings_ops::wave_dir(simple));
        crate::player::waveform_path(&file, cache.as_deref(), simple)
    }).await.map_err(|e| e.to_string())?
}

/// 只读试听：外部绝对路径 → 交给单声卡播放线程，不写素材试听历史。
#[tauri::command]
pub async fn dedup_play(workdir: String, path: String, position: Option<f64>, state: tauri::State<'_, DedupPathState>, player: tauri::State<'_, crate::player::PlayerState>)
    -> Result<crate::player::PlayerSnapshot, String> {
    let wd = resolve_workdir(&workdir)?;
    let player = player.inner().clone();
    let cache = state.cache.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let file = check_member(&wd, &path, &cache)?;
        let id = format!("dedup:{}", norm_path(&file.to_string_lossy()));
        crate::player::load_external(&player, id, file, position.unwrap_or(0.0))
    }).await.map_err(|e| e.to_string())?
}

/// 打开「重复组工作台」独立窗口：顶级窗口（不设 parent），可拖到主窗口之外、可缩放。
/// 参数经初始化脚本注入，避免 URL 编码差异；同一时刻只存在一个工作台窗口。
#[tauri::command]
pub fn dedup_open_workbench(app: tauri::AppHandle, workdir: String, group: String, exe: Option<String>) -> Result<(), String> {
    use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
    let wd = resolve_workdir(&workdir)?;
    let label = "dedup-workbench";
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.set_focus();
        let _ = window.unminimize();
        let _ = window.emit("dedup-workbench-select", serde_json::json!({ "workdir": wd, "group": group }));
        return Ok(());
    }
    let args = serde_json::json!({ "workdir": wd, "group": group, "exe": exe.unwrap_or_default() });
    let script = format!("window.__DEDUP_ARG = {};", serde_json::to_string(&args).map_err(|e| e.to_string())?);
    WebviewWindowBuilder::new(&app, label, WebviewUrl::App("index.html".into()))
        .title("重复组工作台 · SFX TOOLS")
        .inner_size(1180.0, 520.0)
        .min_inner_size(900.0, 380.0)
        .initialization_script(&script)
        .build()
        .map_err(|e| format!("无法打开工作台窗口：{e}"))?;
    Ok(())
}

// ---------- 程序定位：不让用户每次都去找 exe 路径 ----------
//
// 查找顺序：① 上次记下的路径（%APPDATA%\sfx-tools\dedup-exe.txt）→ ② 受限目录遍历。
// 遍历是**有界**的：只在少数常见安装根下按名称找 audio-dedup-gui.exe，限制目录深度与节点数，
// 不扫全盘、不进系统目录，避免一次误操作把磁盘读穿。

const EXE_NAME: &str = "audio-dedup-gui.exe";
const WALK_MAX_DEPTH: usize = 4;
const WALK_MAX_NODES: u32 = 4000;

fn exe_record() -> PathBuf { default_workdir().join("exe.txt") }

fn read_record() -> Option<PathBuf> {
    let raw = std::fs::read_to_string(exe_record()).ok()?;
    let path = PathBuf::from(raw.trim().trim_matches('"'));
    check_exe(&path.to_string_lossy()).ok()
}

fn write_record(path: &Path) -> Result<(), String> {
    let dir = default_workdir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("无法建立索引目录：{e}"))?;
    std::fs::write(exe_record(), path.to_string_lossy().as_bytes())
        .map_err(|e| format!("无法记住去重程序路径：{e}"))
}

fn walk_for_exe(dir: &Path, depth: usize, budget: &mut u32) -> Option<PathBuf> {
    if *budget == 0 || depth > WALK_MAX_DEPTH { return None; }
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        if *budget == 0 { return None; }
        *budget -= 1;
        let path = entry.path();
        let meta = match std::fs::symlink_metadata(&path) { Ok(m) => m, Err(_) => continue };
        // 链接与目录联接一律不跟，避免遍历逃逸到预期之外的盘。
        #[cfg(windows)]
        { use std::os::windows::fs::MetadataExt; if meta.file_attributes() & 0x400 != 0 { continue; } }
        if meta.is_file() {
            if path.file_name().and_then(|v| v.to_str()).is_some_and(|name| name.eq_ignore_ascii_case(EXE_NAME)) {
                return Some(path);
            }
        } else if meta.is_dir() {
            let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
            // 跳过明显的系统与缓存目录，减少无意义遍历。
            if name.starts_with('.') || name.eq_ignore_ascii_case("Windows") || name.eq_ignore_ascii_case("node_modules") { continue; }
            if let Some(found) = walk_for_exe(&path, depth + 1, budget) { return Some(found); }
        }
    }
    None
}

fn search_exe() -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    // 用户自己的工作盘布局优先：常见「工具\Exe」聚合目录。
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
        if let Some(found) = walk_for_exe(&root, 0, &mut budget) { return Some(found); }
    }
    None
}

/// 自动定位去重程序：先读记录，再按有界遍历找；找到就顺手验一次契约。
#[tauri::command]
pub async fn dedup_locate() -> Result<DedupProbe, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = match read_record().or_else(search_exe) {
            Some(path) => path,
            None => return Err("没有找到 audio-dedup-gui.exe。手动选一次即可，之后会自动记住。".into()),
        };
        let probe = probe_exe(&path)?;
        let _ = write_record(&path);
        Ok(probe)
    }).await.map_err(|e| e.to_string())?
}

/// 记住用户手动选中的程序路径，下次启动直接复用。
#[tauri::command]
pub fn dedup_remember_exe(path: String) -> Result<(), String> {
    let checked = check_exe(&path)?;
    write_record(&checked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_relative_and_missing_paths() {
        assert!(check_exe("relative.exe").is_err());
        assert!(check_exe(r"D:\__none__\nope.exe").is_err());
        assert!(check_dir("relative", "扫描根").is_err());
    }

    #[test]
    fn default_workdir_is_absolute_and_under_appdata() {
        let dir = default_workdir();
        assert!(dir.is_absolute());
        assert!(dir.to_string_lossy().contains("sfx-tools"));
    }

    #[test]
    fn groups_without_file_reports_actionable_error() {
        let temp = std::env::temp_dir().join(format!("sfx-dedup-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp).unwrap();
        let err = dedup_groups(Some(temp.to_string_lossy().to_string())).unwrap_err();
        assert!(err.contains("还没有分组结果"));
        std::fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn member_paths_must_appear_in_groups_json() {
        let temp = std::env::temp_dir().join(format!("sfx-dedup-member-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp).unwrap();
        let inner = temp.join("a.wav");
        std::fs::write(&inner, b"x").unwrap();
        let outside = temp.join("outside.wav");
        std::fs::write(&outside, b"x").unwrap();
        // 用 serde 序列化，避免手写转义把 Windows 反斜杠写坏。
        let body = serde_json::json!({
            "schema": 1,
            "groups": [{
                "id": "L1-1", "layer": "L1", "grade": "确定", "count": 1,
                "keep": inner.to_string_lossy(),
                "reclaimable_bytes": 0,
                "files": [{ "path": inner.to_string_lossy(), "size": 1, "keep": true }]
            }]
        });
        std::fs::write(temp.join("groups.json"), serde_json::to_vec(&body).unwrap()).unwrap();
        let cache = std::sync::Arc::new(std::sync::Mutex::new(None));
        assert!(check_member(&temp, &inner.to_string_lossy(), &cache).is_ok());
        // 不在 groups.json 里的路径一律拒绝：界面无法借这条通道读任意文件。
        assert!(check_member(&temp, &outside.to_string_lossy(), &cache).is_err());
        std::fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn search_finds_the_packaged_exe_when_the_tools_root_exists() {
        // 这台机器上 D:\Ai-Files\Tools\Exe 是工具聚合目录；换机器没有这个根就直接跳过。
        if !Path::new(r"D:\Ai-Files\Tools\Exe").is_dir() { return; }
        let found = search_exe();
        assert!(found.is_some(), "受限遍历应该能在工具目录里找到 {EXE_NAME}");
        let path = found.unwrap();
        assert_eq!(path.file_name().and_then(|v| v.to_str()).unwrap().to_ascii_lowercase(), EXE_NAME);
        assert!(path.is_file());
    }

    #[test]
    fn reads_minimal_groups_json() {
        let temp = std::env::temp_dir().join(format!("sfx-dedup-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp).unwrap();
        let body = r#"{"schema":1,"generated_at":"2026-11-18 10:00:00","root":"E:\\lib","workdir":"C:\\wd","counts":{"ok":3,"error":1},"summary":{"l1_groups":1,"l2_groups":0,"l3_groups":0,"edges":0,"groups":1,"reclaimable_bytes":2048},"groups":[{"id":"L1-1","layer":"L1","grade":"确定","count":2,"keep":"E:\\lib\\a.wav","reclaimable_bytes":2048,"files":[{"path":"E:\\lib\\a.wav","size":1024,"keep":true,"ber":null},{"path":"E:\\lib\\b.wav","size":2048,"keep":false,"ber":null}],"edges":[]}],"errors":["E:\\lib\\bad.wav: decode"]}"#;
        std::fs::write(temp.join("groups.json"), body).unwrap();
        let out = dedup_groups(Some(temp.to_string_lossy().to_string())).unwrap();
        assert_eq!(out.total_groups, 1);
        assert_eq!(out.l1_groups, 1);
        assert_eq!(out.reclaimable_bytes, 2048);
        assert_eq!(out.groups[0].files.len(), 2);
        assert!(out.groups[0].files[0].keep);
        assert_eq!(out.errors.len(), 1);
        std::fs::remove_dir_all(&temp).ok();
    }
}
