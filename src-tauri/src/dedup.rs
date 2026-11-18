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
use std::io::Read;
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

/// 探测：跑一次 `--cli --version`，确认这个 exe 真的支持命令行契约。
#[tauri::command]
pub async fn dedup_probe(exe: String) -> Result<DedupProbe, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = check_exe(&exe)?;
        let out = Command::new(&path).arg("--cli").arg("--version").output()
            .map_err(|e| format!("无法启动去重程序：{e}"))?;
        if !out.status.success() {
            return Err(format!("去重程序不支持 --cli（退出码 {:?}）：{}", out.status.code(), tail(&out.stderr)));
        }
        let version = String::from_utf8_lossy(&out.stdout).trim().lines().next().unwrap_or("").trim().to_string();
        if version.is_empty() {
            return Err("去重程序没有返回版本号，无法确认命令行契约。".into());
        }
        Ok(DedupProbe { version, path: path.to_string_lossy().to_string() })
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
