//! 设置页后端：波形缓存治理、临时产物清理、缓存预生成、数据备份、输出设备枚举。
//!
//! 安全边界（务必保持）：
//! - 只扫描/删除本模块白名单内的缓存目录里的普通文件；拒符号链接，不递归子目录。
//! - 绝不触碰 catalog.sqlite、用户素材、模型目录与任何用户指定路径之外的东西。
//! - 备份每次新建目录、独占创建，永不覆盖旧备份；只在应用数据目录内写入。
//! - 自定义缓存根只影响下次启动，写入前校验绝对路径、目录可写与无符号链接。

use cpal::traits::{DeviceTrait, HostTrait};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::Manager;
use std::{
    collections::HashSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

/// 波形缓存组：目录名相对缓存根。新增缓存目录必须显式登记才会被统计/清理。
pub const WAVE_GROUPS: &[(&str, &str)] = &[
    ("waveforms-v1", "列表波形"),
    ("waveforms-mini-v1", "简化小波形"),
    ("full-wave-v1", "完整波形"),
    ("visual-analysis-v1", "高级频谱/RMS"),
];
/// 可再生临时产物组（预放缓存、拖拽切片、叠层渲染等）。
pub const TEMP_GROUPS: &[(&str, &str)] = &[
    ("audio-tools-v1", "音频工具结果"),
    ("layer-renders-v1", "叠层渲染结果"),
    ("library-maintenance-v1", "库维护临时文件"),
];

static CACHE_ROOT: OnceLock<PathBuf> = OnceLock::new();
static CONFIG: OnceLock<Arc<Mutex<CacheConfig>>> = OnceLock::new();

#[derive(Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub custom_root: Option<String>,
    #[serde(default)]
    pub backup_root: Option<String>,
}
fn default_true() -> bool { true }

#[derive(Clone, Serialize)]
pub struct CacheGroup {
    pub id: String,
    pub label: String,
    pub path: String,
    pub files: u64,
    pub bytes: u64,
}
#[derive(Clone, Serialize)]
pub struct CacheInfo {
    pub enabled: bool,
    pub root: String,
    pub default_root: String,
    pub custom_root: Option<String>,
    pub backup_root: String,
    pub wave_groups: Vec<CacheGroup>,
    pub temp_groups: Vec<CacheGroup>,
    pub database_bytes: u64,
    pub restart_required: bool,
}
#[derive(Clone, Serialize, Default)]
pub struct CleanResult {
    pub files: u64,
    pub bytes: u64,
    pub groups: usize,
}
#[derive(Clone, Serialize)]
pub struct Pregenerate {
    pub status: String,
    pub total: usize,
    pub done: usize,
    pub skipped: usize,
    pub failed: usize,
    pub error: Option<String>,
    pub current: Option<String>,
    pub cancelable: bool,
}
#[derive(Clone, Serialize)]
pub struct OutputDevice {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Serialize)]
pub struct BackupItem {
    pub name: String,
    pub path: String,
    pub bytes: u64,
    pub created: String,
}
#[derive(Clone, Serialize)]
pub struct BackupResult {
    pub name: String,
    pub path: String,
    pub bytes: u64,
    pub catalog_bytes: u64,
    pub integrity: String,
    pub manifest: String,
}

#[derive(Clone)]
pub struct SettingsState {
    pub cache_dir: PathBuf,
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    job: Arc<Mutex<Option<Pregenerate>>>,
    cancel: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
}

impl SettingsState {
    pub fn new(cache_dir: PathBuf, data_dir: PathBuf, db_path: PathBuf) -> Self {
        Self {
            cache_dir,
            data_dir,
            db_path,
            job: Arc::new(Mutex::new(None)),
            cancel: Arc::new(AtomicBool::new(false)),
            running: Arc::new(AtomicBool::new(false)),
        }
    }
}

fn err(error: impl std::fmt::Display) -> String { error.to_string() }
fn config_path(data: &Path) -> PathBuf { data.join("settings-cache.json") }

fn read_config(data: &Path) -> CacheConfig {
    let path = config_path(data);
    let bytes = fs::read(&path).unwrap_or_default();
    serde_json::from_slice(&bytes).unwrap_or(CacheConfig { enabled: true, custom_root: None, backup_root: None })
}

fn write_config(data: &Path, config: &CacheConfig) -> Result<(), String> {
    fs::create_dir_all(data).map_err(err)?;
    let bytes = serde_json::to_vec_pretty(config).map_err(err)?;
    let target = config_path(data);
    let temporary = data.join(format!("settings-cache-{}.tmp", Uuid::new_v4()));
    fs::write(&temporary, bytes).map_err(err)?;
    if target.exists() { let _ = fs::remove_file(&target); }
    fs::rename(&temporary, &target).map_err(|e| { let _ = fs::remove_file(&temporary); e.to_string() })?;
    Ok(())
}

/// 路径安全检查：绝对路径、各级祖先均无符号链接。
fn safe_dir(path: &Path) -> bool {
    if !path.is_absolute() { return false; }
    path.ancestors().all(|p| match fs::symlink_metadata(p) {
        Ok(meta) => !meta.file_type().is_symlink(),
        Err(_) => true,
    })
}

/// 在 setup 阶段解析并固定本次进程的缓存根；自定义路径在下次启动才生效。
pub fn init(app: &tauri::App, data: &Path) -> Result<PathBuf, String> {
    let default_root = app.path().app_cache_dir().map_err(err)?;
    let config = read_config(data);
    let root = match config.custom_root.as_deref() {
        Some(raw) if !raw.trim().is_empty() => {
            let candidate = PathBuf::from(raw);
            if fs::create_dir_all(&candidate).is_ok() && safe_dir(&candidate) {
                let probe = candidate.join(format!(".sfx-write-{}", Uuid::new_v4()));
                let writable = fs::write(&probe, b"ok").is_ok();
                let _ = fs::remove_file(&probe);
                if writable { candidate } else { default_root.clone() }
            } else { default_root.clone() }
        }
        _ => default_root.clone(),
    };
    fs::create_dir_all(&root).map_err(err)?;
    let _ = CACHE_ROOT.set(root.clone());
    let _ = CONFIG.set(Arc::new(Mutex::new(config)));
    Ok(root)
}

/// 当前进程实际使用的缓存根（player 波形缓存也走这里）。
pub fn cache_root() -> PathBuf {
    CACHE_ROOT.get().cloned().unwrap_or_else(|| std::env::temp_dir().join("sfx-tools-cache"))
}
pub fn wave_dir(simple: bool) -> PathBuf {
    cache_root().join(if simple { "waveforms-mini-v1" } else { "waveforms-v1" })
}
pub fn cache_enabled() -> bool {
    CONFIG.get().and_then(|c| c.lock().ok().map(|v| v.enabled)).unwrap_or(true)
}
fn shared_config() -> Option<Arc<Mutex<CacheConfig>>> { CONFIG.get().cloned() }

pub fn backup_root(data: &Path) -> PathBuf {
    let custom = shared_config().and_then(|c| c.lock().ok().and_then(|v| v.backup_root.clone()));
    match custom {
        Some(raw) if !raw.trim().is_empty() => {
            let candidate = PathBuf::from(raw);
            if safe_dir(&candidate) || (!candidate.exists() && candidate.is_absolute()) { candidate } else { data.join("backup") }
        }
        _ => data.join("backup"),
    }
}

fn scan_group(root: &Path, id: &str, label: &str) -> CacheGroup {
    let dir = root.join(id);
    let mut files = 0u64;
    let mut bytes = 0u64;
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else { continue; };
            if meta.is_file() && !meta.file_type().is_symlink() {
                files += 1;
                bytes = bytes.saturating_add(meta.len());
            }
        }
    }
    CacheGroup { id: id.to_string(), label: label.to_string(), path: dir.display().to_string(), files, bytes }
}

/// 只删白名单组目录里的普通文件；不递归、不跟随链接、不删目录本身。
fn clear_groups(root: &Path, ids: &HashSet<String>, allowed: &[(&str, &str)]) -> Result<CleanResult, String> {
    let mut out = CleanResult::default();
    for (id, _) in allowed {
        if !ids.contains(*id) { continue; }
        let dir = root.join(id);
        if !safe_dir(&dir) { return Err(format!("缓存目录不安全，已停止清理：{}", dir.display())); }
        let Ok(entries) = fs::read_dir(&dir) else { continue; };
        out.groups += 1;
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else { continue; };
            if !meta.is_file() || meta.file_type().is_symlink() { continue; }
            if fs::remove_file(&path).is_ok() {
                out.files += 1;
                out.bytes = out.bytes.saturating_add(meta.len());
            }
        }
    }
    Ok(out)
}

fn file_size(path: &Path) -> u64 { fs::metadata(path).map(|m| m.len()).unwrap_or(0) }
fn dir_size(path: &Path) -> u64 {
    fs::read_dir(path).map(|entries| entries.flatten().filter_map(|e| fs::metadata(e.path()).ok()).map(|m| m.len()).sum()).unwrap_or(0)
}

/// UTC+8 的本地时间戳；避免为格式化时间引入 chrono。
fn stamp() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64 + 8 * 3600;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Howard Hinnant civil_from_days
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{:04}{:02}{:02}-{:02}{:02}{:02}", year, m, d, rem / 3600, (rem % 3600) / 60, rem % 60)
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(err)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer).map_err(err)?;
        if n == 0 { break; }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

// ─────────────────────────── Tauri 命令 ───────────────────────────

#[tauri::command]
pub fn settings_cache_info(state: tauri::State<'_, SettingsState>) -> CacheInfo {
    let root = cache_root();
    let config = shared_config().and_then(|c| c.lock().ok().map(|v| v.clone()));
    let wave_groups = WAVE_GROUPS.iter().map(|(id, label)| scan_group(&root, id, label)).collect();
    let temp_groups = TEMP_GROUPS.iter().map(|(id, label)| scan_group(&root, id, label)).collect();
    CacheInfo {
        enabled: config.as_ref().map(|c| c.enabled).unwrap_or(true),
        root: root.display().to_string(),
        default_root: state.cache_dir.display().to_string(),
        custom_root: config.and_then(|c| c.custom_root),
        backup_root: backup_root(&state.data_dir).display().to_string(),
        wave_groups,
        temp_groups,
        database_bytes: file_size(&state.db_path),
        restart_required: false,
    }
}

#[tauri::command]
pub fn settings_cache_set_enabled(enabled: bool, state: tauri::State<'_, SettingsState>) -> Result<CacheInfo, String> {
    if let Some(shared) = shared_config() {
        if let Ok(mut config) = shared.lock() { config.enabled = enabled; write_config(&state.data_dir, &config)?; }
    }
    Ok(settings_cache_info(state))
}

#[tauri::command]
pub fn settings_cache_set_root(path: Option<String>, state: tauri::State<'_, SettingsState>) -> Result<CacheInfo, String> {
    let validated = match path.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(raw) => {
            let candidate = PathBuf::from(raw);
            if !candidate.is_absolute() { return Err("缓存路径必须是绝对路径。".into()); }
            fs::create_dir_all(&candidate).map_err(err)?;
            if !safe_dir(&candidate) { return Err("缓存路径包含符号链接，已拒绝。".into()); }
            let probe = candidate.join(format!(".sfx-write-{}", Uuid::new_v4()));
            fs::write(&probe, b"ok").map_err(err)?;
            let _ = fs::remove_file(&probe);
            Some(candidate.display().to_string())
        }
    };
    if let Some(shared) = shared_config() {
        if let Ok(mut config) = shared.lock() { config.custom_root = validated; write_config(&state.data_dir, &config)?; }
    }
    let mut info = settings_cache_info(state);
    // 自定义根只在下次启动生效，这里明确告知界面。
    info.restart_required = true;
    Ok(info)
}

#[tauri::command]
pub fn settings_cache_clear(groups: Vec<String>) -> Result<CleanResult, String> {
    let ids: HashSet<String> = groups.into_iter().collect();
    clear_groups(&cache_root(), &ids, WAVE_GROUPS)
}

#[tauri::command]
pub fn settings_temp_clear() -> Result<CleanResult, String> {
    let ids: HashSet<String> = TEMP_GROUPS.iter().map(|(id, _)| id.to_string()).collect();
    clear_groups(&cache_root(), &ids, TEMP_GROUPS)
}

/// 只打开应用自己的缓存/数据/备份目录，拒绝任意路径。
#[tauri::command]
pub fn settings_open_folder(path: String, state: tauri::State<'_, SettingsState>) -> Result<(), String> {
    let candidate = PathBuf::from(&path);
    let allowed = [cache_root(), state.data_dir.clone(), backup_root(&state.data_dir)];
    if !allowed.iter().any(|root| candidate == *root || candidate.starts_with(root)) {
        return Err("只能打开本应用的缓存、数据或备份目录。".into());
    }
    fs::create_dir_all(&candidate).map_err(err)?;
    #[cfg(windows)]
    std::process::Command::new("explorer.exe").arg(&candidate).spawn().map_err(err)?;
    #[cfg(not(windows))]
    return Err("打开目录目前仅支持 Windows。".into());
    #[cfg(windows)]
    Ok(())
}

// ── 缓存预生成 ──

fn set_job(state: &SettingsState, update: impl FnOnce(&mut Pregenerate)) {
    if let Ok(mut slot) = state.job.lock() {
        let job = slot.get_or_insert_with(|| Pregenerate {
            status: "running".into(), total: 0, done: 0, skipped: 0, failed: 0,
            error: None, current: None, cancelable: true,
        });
        update(job);
    }
}

#[tauri::command]
pub fn settings_cache_pregenerate(scope: String, library: Option<String>, folder: Option<String>, state: tauri::State<'_, SettingsState>) -> Result<(), String> {
    if !["all", "library", "folder"].contains(&scope.as_str()) { return Err("预生成范围无效。".into()); }
    if state.running.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
        return Err("已有缓存预生成任务运行，请稍后重试。".into());
    }
    state.cancel.store(false, Ordering::Release);
    let inner = state.inner().clone();
    let db = state.db_path.clone();
    let dir = wave_dir(true);
    {
        let mut slot = inner.job.lock().map_err(|_| "任务状态锁不可用".to_string())?;
        *slot = Some(Pregenerate { status: "running".into(), total: 0, done: 0, skipped: 0, failed: 0, error: None, current: None, cancelable: true });
    }
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| -> Result<(), String> {
            let conn = rusqlite::Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(err)?;
            conn.busy_timeout(Duration::from_secs(5)).map_err(err)?;
            let mut sql = String::from("SELECT a.path FROM assets a WHERE a.status='ready'");
            let mut params: Vec<String> = Vec::new();
            if let Some(lib) = library.as_deref().filter(|v| *v == "sound" || *v == "music") {
                params.push(lib.to_string());
                sql.push_str(&format!(" AND EXISTS(SELECT 1 FROM asset_libraries l WHERE l.asset_id=a.id AND l.library=?{})", params.len()));
            }
            if scope == "folder" {
                if let Some(prefix) = folder.as_deref().filter(|v| !v.is_empty()) {
                    params.push(prefix.to_string());
                    let n = params.len();
                    sql.push_str(&format!(" AND substr(a.path,1,length(?{n}))=?{n}"));
                }
            }
            sql.push_str(" ORDER BY a.id LIMIT 100000");
            let mut statement = conn.prepare(&sql).map_err(err)?;
            let rows: Vec<String> = statement
                .query_map(rusqlite::params_from_iter(params.iter()), |r| r.get(0))
                .map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
            set_job(&inner, |job| job.total = rows.len());
            for path in rows {
                if inner.cancel.load(Ordering::Acquire) { set_job(&inner, |job| job.status = "cancelled".into()); break; }
                let source = Path::new(&path);
                set_job(&inner, |job| job.current = Some(source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.clone())));
                match crate::player::pregenerate_wave_cache(source, &dir, true) {
                    Ok(true) => set_job(&inner, |job| job.done += 1),
                    Ok(false) => set_job(&inner, |job| job.skipped += 1),
                    Err(_) => set_job(&inner, |job| job.failed += 1),
                }
            }
            set_job(&inner, |job| {
                if job.status == "running" { job.status = "completed".into(); }
                job.current = None;
                job.cancelable = false;
            });
            Ok(())
        })();
        if let Err(error) = result {
            set_job(&inner, |job| { job.status = "failed".into(); job.error = Some(error); job.cancelable = false; });
        }
        inner.running.store(false, Ordering::Release);
    });
    Ok(())
}

#[tauri::command]
pub fn settings_cache_progress(state: tauri::State<'_, SettingsState>) -> Option<Pregenerate> {
    state.job.lock().ok().and_then(|slot| slot.clone())
}

#[tauri::command]
pub fn settings_cache_cancel(state: tauri::State<'_, SettingsState>) {
    state.cancel.store(true, Ordering::Release);
}

// ── 输出设备 ──

#[tauri::command]
pub fn settings_output_devices() -> Vec<OutputDevice> {
    let mut out = vec![OutputDevice { id: "default".into(), name: "系统默认".into() }];
    if let Ok(devices) = cpal::default_host().output_devices() {
        for device in devices {
            if let Ok(name) = device.name() {
                if !name.trim().is_empty() && !out.iter().any(|item| item.name == name) {
                    out.push(OutputDevice { id: name.clone(), name });
                }
            }
        }
    }
    out
}

/// 供 player 按名称查找设备；找不到就回落系统默认。
pub fn find_output_device(name: &str) -> Option<cpal::Device> {
    if name.trim().is_empty() || name == "default" { return None; }
    cpal::default_host().output_devices().ok()?.find(|device| {
        device.name().map(|value| value == name).unwrap_or(false)
    })
}

#[tauri::command]
pub fn settings_player_set_device(name: Option<String>, player: tauri::State<'_, crate::player::PlayerState>) -> Result<(), String> {
    player.set_output_device(name.unwrap_or_default());
    Ok(())
}

// ── 数据备份 ──

#[tauri::command]
pub fn settings_backup_list(state: tauri::State<'_, SettingsState>) -> Vec<BackupItem> {
    let root = backup_root(&state.data_dir);
    let Ok(entries) = fs::read_dir(&root) else { return vec![]; };
    let mut items: Vec<BackupItem> = entries.flatten().filter_map(|entry| {
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).ok()?;
        if !meta.is_dir() || meta.file_type().is_symlink() { return None; }
        let name = path.file_name()?.to_string_lossy().into_owned();
        if !name.starts_with("backup-") { return None; }
        let manifest = path.join("manifest.json");
        let created = fs::read(&manifest).ok().and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|value| value.get("created").and_then(|v| v.as_str()).map(str::to_owned))
            .unwrap_or_else(|| name.clone());
        Some(BackupItem { name, path: path.display().to_string(), bytes: dir_size(&path), created })
    }).collect();
    items.sort_by(|a, b| b.name.cmp(&a.name));
    items
}

/// 每次生成新目录，独占创建，绝不覆盖旧备份。
/// `snapshot` 是前端收集的 localStorage（主题/偏好/布局/列设置等），原样落盘。
#[tauri::command]
pub fn settings_backup_create(snapshot: Option<serde_json::Value>, state: tauri::State<'_, SettingsState>) -> Result<BackupResult, String> {
    let root = backup_root(&state.data_dir);
    fs::create_dir_all(&root).map_err(err)?;
    if !safe_dir(&root) { return Err("备份目录包含符号链接，已停止。".into()); }
    let name = format!("backup-{}-{}", stamp(), &Uuid::new_v4().to_string()[..8]);
    let target = root.join(&name);
    fs::create_dir(&target).map_err(err)?; // 已存在即失败，不覆盖

    let catalog = target.join("catalog.sqlite");
    // VACUUM INTO 生成一致性快照，且不会改写运行中的主库。
    let connection = rusqlite::Connection::open_with_flags(&state.db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(err)?;
    connection.execute("VACUUM INTO ?1", [catalog.display().to_string()]).map_err(err)?;
    let integrity: String = {
        let check = rusqlite::Connection::open_with_flags(&catalog, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(err)?;
        check.query_row("PRAGMA integrity_check", [], |row| row.get(0)).map_err(err)?
    };
    if integrity != "ok" { return Err("备份完整性校验未通过，已保留文件供调查。".into()); }
    drop(connection);

    let settings = target.join("settings.json");
    fs::write(&settings, serde_json::to_vec_pretty(&snapshot.unwrap_or(serde_json::Value::Null)).map_err(err)?).map_err(err)?;
    let catalog_bytes = file_size(&catalog);
    let digest = sha256_file(&catalog)?;
    let created = stamp();
    let manifest = serde_json::json!({
        "format": "sfx-tools-backup-v1",
        "created": created,
        "app_version": env!("CARGO_PKG_VERSION"),
        "catalog": { "file": "catalog.sqlite", "bytes": catalog_bytes, "sha256": digest, "integrity": integrity },
        "settings": "settings.json",
        "note": "恢复：退出程序后，把 catalog.sqlite 复制回应用数据目录（先自行保留现有文件）；settings.json 为界面设置快照，可按需手动参考。"
    });
    let manifest_path = target.join("manifest.json");
    fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest).map_err(err)?).map_err(err)?;
    fs::write(target.join("README.txt"), "SFX Tools 数据备份\n\n包含：catalog.sqlite（素材库与收藏/评分/笔记/分类/播放列表/历史）、settings.json（主题、偏好、布局、列设置等界面快照）、manifest.json（校验信息）。\n\n每次备份都是独立目录，不会覆盖旧备份。恢复前请先退出程序并自行保留当前数据。\n").map_err(err)?;
    Ok(BackupResult {
        name,
        path: target.display().to_string(),
        bytes: dir_size(&target),
        catalog_bytes,
        integrity,
        manifest: manifest_path.display().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("sfx-settings-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        (root.clone(), root)
    }

    #[test]
    fn clear_only_removes_whitelisted_plain_files() {
        let (root, guard) = fixture();
        let wave = root.join("waveforms-v1");
        let other = root.join("catalog.sqlite");
        fs::create_dir_all(&wave).unwrap();
        fs::write(wave.join("a.json"), b"one").unwrap();
        fs::write(wave.join("b.tmp"), b"two").unwrap();
        fs::create_dir_all(wave.join("nested")).unwrap();
        fs::write(wave.join("nested/keep.json"), b"keep").unwrap();
        fs::write(&other, b"database").unwrap();
        let ids: HashSet<String> = ["waveforms-v1".to_string(), "catalog.sqlite".to_string(), "nope".to_string()].into_iter().collect();
        let result = clear_groups(&root, &ids, WAVE_GROUPS).unwrap();
        assert_eq!(result.files, 2);
        assert_eq!(result.bytes, 6);
        assert!(other.exists(), "不得触碰数据库文件");
        assert!(wave.join("nested/keep.json").exists(), "不递归子目录");
        fs::remove_dir_all(guard).unwrap();
    }

    #[test]
    fn scan_group_reports_only_plain_files() {
        let (root, guard) = fixture();
        let dir = root.join("full-wave-v1");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.json"), b"12345").unwrap();
        fs::create_dir_all(dir.join("sub")).unwrap();
        let group = scan_group(&root, "full-wave-v1", "完整波形");
        assert_eq!(group.files, 1);
        assert_eq!(group.bytes, 5);
        fs::remove_dir_all(guard).unwrap();
    }

    #[test]
    fn stamp_is_sortable_and_local() {
        let a = stamp();
        assert_eq!(a.len(), 15);
        assert!(a.chars().all(|c| c.is_ascii_digit() || c == '-'));
    }

    #[test]
    fn safe_dir_rejects_relative_paths() {
        let (root, guard) = fixture();
        assert!(!safe_dir(Path::new("relative/cache")));
        assert!(safe_dir(&root));
        fs::remove_dir_all(guard).unwrap();
    }

    #[test]
    fn config_roundtrip_preserves_fields() {
        let (root, guard) = fixture();
        let config = CacheConfig { enabled: false, custom_root: Some(root.display().to_string()), backup_root: None };
        write_config(&root, &config).unwrap();
        let loaded = read_config(&root);
        assert!(!loaded.enabled);
        assert_eq!(loaded.custom_root, config.custom_root);
        fs::remove_dir_all(guard).unwrap();
    }

    #[test]
    fn output_devices_always_include_system_default() {
        let devices = settings_output_devices();
        assert_eq!(devices[0].id, "default");
        assert_eq!(devices[0].name, "系统默认");
    }
}
