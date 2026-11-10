// PT 批量导出只读模块：只检查输出目录并生成命名预览，不创建目录、不渲染音频、不写文件。
use super::{
    blocked, contract_only, gate, read_only_ok, unsupported, PluginResult, EVIDENCE_LOCAL_PROBE,
    PLUGIN_PT_BOUNCER as ID,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;

pub const META: &str = include_str!("plugin.toml");

// 文件名非法字符；仅做只读检测，不改名、不落盘。
const ILLEGAL_CHARS: [char; 9] = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];

#[derive(Serialize)]
struct NamePreview {
    target: String,
    suggested: String,
    illegal_chars: Vec<String>,
    duplicate_of: Option<String>,
}

fn illegal_in(name: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for ch in name.chars() {
        if ILLEGAL_CHARS.contains(&ch) || ch.is_control() {
            let label = ch.to_string();
            if !found.contains(&label) {
                found.push(label);
            }
        }
    }
    found
}

/// 生成建议文件名：把非法字符替换成下划线，不读写任何文件。
fn suggested_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ILLEGAL_CHARS.contains(&ch) || ch.is_control() {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    out.trim().to_string()
}

/// 只读命名预览：逐个生成建议名、检测重名（大小写不敏感）与非法字符。
fn build_previews(targets: &[String]) -> Vec<NamePreview> {
    let mut seen: HashMap<String, String> = HashMap::new();
    let mut previews = Vec::with_capacity(targets.len());
    for target in targets {
        let key = target.to_ascii_lowercase();
        let duplicate_of = seen.get(&key).cloned();
        if duplicate_of.is_none() {
            seen.insert(key, target.clone());
        }
        previews.push(NamePreview {
            target: target.clone(),
            suggested: suggested_name(target),
            illegal_chars: illegal_in(target),
            duplicate_of,
        });
    }
    previews
}

/// 只生成命名映射与冲突检查，不渲染、不写音频；拒绝同名覆盖口径保留。
#[tauri::command]
pub fn plugin_sfx_pt_bouncer_preview(
    output_dir: Option<String>,
    targets: Option<Vec<String>>,
) -> PluginResult {
    if let Some(early) = gate(ID, "preview") {
        return early;
    }
    let dir = match output_dir {
        Some(value) => value,
        None => {
            return contract_only(
                ID,
                "preview",
                json!({
                    "output_dir": Value::Null,
                    "targets": Vec::<String>::new(),
                    "exists": false,
                    "is_dir": false,
                    "names": [],
                    "conflicts": [],
                    "count": 0
                }),
                "未提供 output_dir，未做目录检查；此为只读回显",
            );
        }
    };
    if !Path::new(&dir).is_absolute() {
        return unsupported(ID, "preview", &dir);
    }
    let targets = targets.unwrap_or_default();
    // 真正用 std::fs::metadata 只读检查 output_dir 是否存在、是否为目录。
    let (exists, is_dir) = match std::fs::metadata(&dir) {
        Ok(metadata) => (true, metadata.is_dir()),
        Err(_) => (false, false),
    };
    let previews = build_previews(&targets);
    let conflicts: Vec<Value> = previews
        .iter()
        .filter(|item| item.duplicate_of.is_some() || !item.illegal_chars.is_empty())
        .map(|item| {
            json!({
                "target": item.target,
                "reason": if item.duplicate_of.is_some() { "duplicate" } else { "illegal_chars" },
                "illegal_chars": item.illegal_chars,
                "duplicate_of": item.duplicate_of,
            })
        })
        .collect();
    let count = previews.len();
    read_only_ok(
        ID,
        "preview",
        "输出目录只读预览完成（未创建目录、未渲染音频、未写任何文件）",
        json!({
            "output_dir": dir,
            "exists": exists,
            "is_dir": is_dir,
            "names": previews,
            "conflicts": conflicts,
            "count": count,
            "method": "read_only_directory_and_naming_preview",
        }),
        EVIDENCE_LOCAL_PROBE,
        &[
            "仅以 std::fs::metadata 检查目录存在性与类型，并做只读命名／重名／非法字符预览；不复制、不移动、不创建、不渲染、不写库",
        ],
    )
}

/// 渲染属写入类，read_only 状态下恒阻断。
#[tauri::command]
pub fn plugin_sfx_pt_bouncer_run(
    preview_id: Option<String>,
    allow_write: Option<bool>,
) -> PluginResult {
    if let Some(early) = gate(ID, "run") {
        return early;
    }
    let _ = (preview_id, allow_write);
    blocked(ID, "run")
}

#[tauri::command]
pub fn plugin_sfx_pt_bouncer_cancel(job_id: Option<String>) -> PluginResult {
    if let Some(early) = gate(ID, "cancel") {
        return early;
    }
    contract_only(
        ID,
        "cancel",
        json!({ "job_id": job_id, "completed": [], "cancelled": [] }),
        "只读回显：模块无运行中的真实导出任务，未做任何取消或写入操作",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_dir(tag: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let mut path = std::env::temp_dir();
        path.push(format!("sfx-tools-{}-{}-{}", tag, nanos, sequence));
        std::fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    #[test]
    fn preview_reads_output_dir_and_builds_naming_preview() {
        let dir = unique_dir("bouncer");
        let targets = vec![
            "Kick.wav".to_string(),
            "kick.wav".to_string(),      // 重名（大小写）
            "bad/name?.wav".to_string(), // 非法字符
        ];
        let dir_str = dir.to_string_lossy().into_owned();
        let result = plugin_sfx_pt_bouncer_preview(Some(dir_str), Some(targets));
        if cfg!(feature = "plugin-pt-bouncer") {
            assert!(result.ok);
            assert_eq!(result.evidence, EVIDENCE_LOCAL_PROBE);
            assert!(!result.warnings.is_empty());
            assert_eq!(result.data["exists"], true);
            assert_eq!(result.data["is_dir"], true);
            assert_eq!(result.data["count"], 3);
            assert_eq!(
                result.data["conflicts"]
                    .as_array()
                    .map(|items| items.len())
                    .unwrap_or(0),
                2
            );
        } else {
            assert!(!result.ok);
            assert_eq!(result.code, "PLUGIN_DISABLED");
        }
        std::fs::remove_dir_all(&dir).expect("cleanup temp dir");
    }

    #[test]
    fn preview_reports_missing_directory_as_read_only() {
        let dir = unique_dir("bouncer-miss");
        let missing = dir
            .join("definitely-not-created")
            .to_string_lossy()
            .into_owned();
        std::fs::remove_dir_all(&dir).expect("cleanup temp dir");
        let result = plugin_sfx_pt_bouncer_preview(Some(missing), None);
        if cfg!(feature = "plugin-pt-bouncer") {
            assert!(result.ok);
            assert_eq!(result.evidence, EVIDENCE_LOCAL_PROBE);
            assert_eq!(result.data["exists"], false);
            assert_eq!(result.data["is_dir"], false);
        }
    }

    #[test]
    fn cancel_is_read_only_echo() {
        let result = plugin_sfx_pt_bouncer_cancel(Some("job-1".to_string()));
        if cfg!(feature = "plugin-pt-bouncer") {
            assert!(result.ok);
            assert_ne!(result.evidence, "none");
            assert!(!result.warnings.is_empty());
        }
    }

    #[test]
    fn run_is_always_write_blocked() {
        let result = plugin_sfx_pt_bouncer_run(None, Some(true));
        assert!(!result.ok);
        assert!(result.code == "WRITE_BLOCKED" || result.code == "PLUGIN_DISABLED");
        assert_eq!(result.evidence, "none");
        if cfg!(feature = "plugin-pt-bouncer") {
            assert_eq!(result.code, "WRITE_BLOCKED");
        }
    }
}
