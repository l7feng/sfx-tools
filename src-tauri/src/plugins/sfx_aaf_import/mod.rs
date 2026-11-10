// AAF／OMF 只读导入模块：只做文件头＋字符串启发式探测，不解析完整对象模型、不写工程。
use super::{
    blocked, contract_only, gate, probe_error, read_only_ok, unsupported, PluginResult,
    EVIDENCE_STATIC_PARSE, PLUGIN_AAF_IMPORT as ID,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const META: &str = include_str!("plugin.toml");

// OLE Compound File / Structured Storage 魔数。
const OLE_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
const MEDIA_EXTENSIONS: [&str; 8] = [
    ".wav", ".aif", ".aiff", ".aifc", ".mp3", ".flac", ".mxf", ".mov",
];
const MARKERS: [(&str, &[u8]); 3] = [("AAF", b"AAF"), ("OMFI", b"OMFI"), ("OMF", b"OMF")];
// 只读探测最多读取的头部字节数，避免把超大工程整份读进内存。
const MAX_PROBE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_CANDIDATES: usize = 200;

#[derive(Clone, Serialize)]
struct Candidate {
    reference: String,
    encoding: &'static str,
    status: &'static str,
    resolved_path: Option<String>,
}

#[derive(Clone, Serialize)]
struct ParseSnapshot {
    parse_id: String,
    container: String,
    format_guess: String,
    file_size: u64,
    modified_unix: Option<u64>,
    markers: Vec<String>,
    candidates: Vec<Candidate>,
    method: &'static str,
}

// 进程内保存最近一次真实探测结果，供 candidates 做只读回显（不重新读文件）。
static LAST_PARSE: OnceLock<Mutex<Option<ParseSnapshot>>> = OnceLock::new();

fn last_parse() -> &'static Mutex<Option<ParseSnapshot>> {
    LAST_PARSE.get_or_init(|| Mutex::new(None))
}

fn simple_hash(input: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in input.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn parse_id_for(path: &str, size: u64, modified: Option<u64>) -> String {
    format!(
        "aaf-{:016x}",
        simple_hash(&format!("{}|{}|{:?}", path, size, modified))
    )
}

fn contains_marker(bytes: &[u8], marker: &[u8]) -> bool {
    if marker.is_empty() || bytes.len() < marker.len() {
        return false;
    }
    bytes.windows(marker.len()).any(|window| window == marker)
}

fn ends_with_media_extension(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    MEDIA_EXTENSIONS.iter().any(|ext| lower.ends_with(ext))
}

/// 路径片段允许出现的字符；用于从可打印串尾部回退截取连续路径。
fn is_path_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            '\\' | '/' | ':' | '.' | '_' | '-' | '(' | ')' | '[' | ']' | ' ' | '#' | '%'
                | '+' | ',' | '@' | '~' | '=' | '\'' | '&' | '!' | '$' | '^' | '`' | '{'
                | '}' | ';'
        )
}

/// 从一段连续可打印 ASCII 字节里截取以媒体扩展名结尾的路径片段。
fn token_from_run(run: &[u8]) -> Option<String> {
    if run.len() < 5 {
        return None;
    }
    let chars: Vec<char> = run.iter().map(|&byte| byte as char).collect();
    let raw: String = chars.iter().collect();
    if !ends_with_media_extension(&raw) {
        return None;
    }
    let mut start = chars.len();
    while start > 0 && is_path_char(chars[start - 1]) {
        start -= 1;
    }
    let token: String = chars[start..].iter().collect();
    if token.len() < 5 {
        return None;
    }
    Some(token)
}

fn flush_run(run: &mut Vec<u8>, out: &mut Vec<(String, &'static str)>, encoding: &'static str) {
    if let Some(token) = token_from_run(run) {
        out.push((token, encoding));
    }
    run.clear();
}

/// ASCII 编码的连续可打印串。
fn extract_ascii_references(bytes: &[u8]) -> Vec<(String, &'static str)> {
    let mut out = Vec::new();
    let mut run: Vec<u8> = Vec::new();
    for &byte in bytes {
        if (0x20..=0x7E).contains(&byte) {
            run.push(byte);
        } else {
            flush_run(&mut run, &mut out, "ascii");
        }
    }
    flush_run(&mut run, &mut out, "ascii");
    out
}

/// UTF-16LE 编码的连续可打印串；同时尝试偶/奇字节对齐，避免漏掉未对齐字符串。
fn extract_utf16le_references(bytes: &[u8]) -> Vec<(String, &'static str)> {
    let mut out = Vec::new();
    for offset in [0usize, 1usize] {
        let mut run: Vec<u8> = Vec::new();
        let mut index = offset;
        while index + 1 < bytes.len() {
            let low = bytes[index];
            let high = bytes[index + 1];
            if high == 0 && (0x20..=0x7E).contains(&low) {
                run.push(low);
            } else {
                flush_run(&mut run, &mut out, "utf16le");
            }
            index += 2;
        }
        flush_run(&mut run, &mut out, "utf16le");
    }
    out
}

fn dedup_references(pairs: Vec<(String, &'static str)>) -> Vec<(String, &'static str)> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for (reference, encoding) in pairs {
        if seen.insert(reference.to_ascii_lowercase()) {
            out.push((reference, encoding));
            if out.len() >= MAX_CANDIDATES {
                break;
            }
        }
    }
    out
}

/// 在 media_roots 下做只读存在性检查：resolved / missing / ambiguous；无根目录则 unresolved。
fn check_reference(reference: &str, roots: &[String]) -> (&'static str, Option<String>) {
    if roots.is_empty() {
        return ("unresolved", None);
    }
    let ref_path = Path::new(reference);
    let mut hits: Vec<String> = Vec::new();
    for root in roots {
        let base = Path::new(root);
        let attempts: Vec<PathBuf> = if ref_path.is_absolute() {
            match ref_path.file_name() {
                Some(name) => vec![base.join(name)],
                None => Vec::new(),
            }
        } else {
            vec![base.join(ref_path)]
        };
        for candidate in attempts {
            // 仅 std::fs 只读元数据查询，不复制、不移动、不创建任何文件。
            if let Ok(metadata) = std::fs::metadata(&candidate) {
                if metadata.is_file() {
                    let value = candidate.to_string_lossy().into_owned();
                    if !hits.contains(&value) {
                        hits.push(value);
                    }
                }
            }
        }
    }
    match hits.len() {
        0 => ("missing", None),
        1 => ("resolved", Some(hits.remove(0))),
        _ => ("ambiguous", None),
    }
}

/// 真实只读探测：读取文件头、识别容器与标记、启发式提取媒体引用。
fn probe_file(source_path: &str, media_roots: &[String]) -> Result<ParseSnapshot, String> {
    let metadata = std::fs::metadata(source_path)
        .map_err(|error| format!("无法读取文件元数据：{error}"))?;
    if !metadata.is_file() {
        return Err("source_path 不是普通文件".to_string());
    }
    let file_size = metadata.len();
    let modified_unix = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs());

    let mut file = std::fs::File::open(source_path)
        .map_err(|error| format!("无法以只读方式打开文件：{error}"))?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_PROBE_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("读取文件头失败：{error}"))?;

    let container = if bytes.starts_with(&OLE_MAGIC) {
        "ole_compound_file"
    } else {
        "unknown_or_flat"
    };
    let mut markers: Vec<String> = Vec::new();
    for (label, needle) in MARKERS {
        if contains_marker(&bytes, needle) {
            markers.push(label.to_string());
        }
    }
    let format_guess = if markers.iter().any(|item| item == "AAF") {
        "AAF"
    } else if markers.iter().any(|item| item == "OMFI" || item == "OMF") {
        "OMF"
    } else {
        "unknown"
    };

    let mut pairs = extract_ascii_references(&bytes);
    pairs.extend(extract_utf16le_references(&bytes));
    let references = dedup_references(pairs);
    let candidates: Vec<Candidate> = references
        .into_iter()
        .map(|(reference, encoding)| {
            let (status, resolved_path) = check_reference(&reference, media_roots);
            Candidate {
                reference,
                encoding,
                status,
                resolved_path,
            }
        })
        .collect();

    Ok(ParseSnapshot {
        parse_id: parse_id_for(source_path, file_size, modified_unix),
        container: container.to_string(),
        format_guess: format_guess.to_string(),
        file_size,
        modified_unix,
        markers,
        candidates,
        method: "heuristic_header_probe",
    })
}

#[tauri::command]
pub fn plugin_sfx_aaf_import_parse(
    source_path: Option<String>,
    media_roots: Option<Vec<String>>,
    read_only: Option<bool>,
) -> PluginResult {
    if let Some(early) = gate(ID, "parse") {
        return early;
    }
    let path = match source_path {
        Some(value) => value,
        None => {
            return contract_only(
                ID,
                "parse",
                json!({
                    "source_path": Value::Null,
                    "media_roots": Vec::<String>::new(),
                    "read_only": read_only.unwrap_or(true),
                    "method": "heuristic_header_probe",
                    "candidates": [],
                    "candidate_count": 0
                }),
                "未提供 source_path，未读取任何文件；此为只读回显",
            );
        }
    };
    if !Path::new(&path).is_absolute() {
        return unsupported(ID, "parse", &path);
    }
    let roots = media_roots.unwrap_or_default();
    match probe_file(&path, &roots) {
        Ok(snapshot) => {
            if let Ok(mut guard) = last_parse().lock() {
                *guard = Some(snapshot.clone());
            }
            let candidate_count = snapshot.candidates.len();
            let resolved = snapshot
                .candidates
                .iter()
                .filter(|item| item.status == "resolved")
                .count();
            let missing = snapshot
                .candidates
                .iter()
                .filter(|item| item.status == "missing")
                .count();
            let ambiguous = snapshot
                .candidates
                .iter()
                .filter(|item| item.status == "ambiguous")
                .count();
            let unresolved = snapshot
                .candidates
                .iter()
                .filter(|item| item.status == "unresolved")
                .count();
            let mut data = serde_json::to_value(&snapshot).unwrap_or(Value::Null);
            if let Some(map) = data.as_object_mut() {
                map.insert("source_path".to_string(), json!(path));
                map.insert("media_roots".to_string(), json!(roots));
                map.insert("read_only".to_string(), json!(read_only.unwrap_or(true)));
                map.insert("candidate_count".to_string(), json!(candidate_count));
                map.insert("resolved".to_string(), json!(resolved));
                map.insert("missing".to_string(), json!(missing));
                map.insert("ambiguous".to_string(), json!(ambiguous));
                map.insert("unresolved".to_string(), json!(unresolved));
            }
            read_only_ok(
                ID,
                "parse",
                "文件头＋字符串启发式探测完成（非完整 AAF／OMF 对象模型解析）",
                data,
                EVIDENCE_STATIC_PARSE,
                &[
                    "容器识别基于 OLE 魔数与 AAF／OMFI／OMF 字符串标记，媒体引用来自 ASCII／UTF-16LE 可打印串扫描，均为启发式结果；候选需人工复核",
                ],
            )
        }
        Err(error) => probe_error(
            ID,
            "parse",
            &format!("只读文件头探测失败：{error}"),
            json!({ "source_path": path, "media_roots": roots }),
            "未读取成功任何文件内容，未写任何文件",
        ),
    }
}

#[tauri::command]
pub fn plugin_sfx_aaf_import_candidates(
    parse_id: Option<String>,
    reviewed: Option<bool>,
) -> PluginResult {
    if let Some(early) = gate(ID, "candidates") {
        return early;
    }
    let cached = last_parse().lock().ok().and_then(|guard| guard.clone());
    match cached {
        Some(snapshot) => {
            let count = snapshot.candidates.len();
            let effective_parse_id =
                parse_id.unwrap_or_else(|| snapshot.parse_id.clone());
            let mut data = serde_json::to_value(&snapshot).unwrap_or(Value::Null);
            if let Some(map) = data.as_object_mut() {
                map.insert("parse_id".to_string(), json!(effective_parse_id));
                map.insert("reviewed".to_string(), json!(reviewed.unwrap_or(false)));
                map.insert("candidate_count".to_string(), json!(count));
                map.insert("items".to_string(), json!(snapshot.candidates));
            }
            read_only_ok(
                ID,
                "candidates",
                "回显最近一次文件头启发式探测的候选清单（本次未重新读取文件）",
                data,
                EVIDENCE_STATIC_PARSE,
                &[
                    "候选数据来自上一次 static_parse 探测，本次为进程内只读回显、未重新读文件；候选需人工复核，不自动写库",
                ],
            )
        }
        None => contract_only(
            ID,
            "candidates",
            json!({
                "parse_id": parse_id,
                "reviewed": reviewed.unwrap_or(false),
                "items": [],
                "candidate_count": 0
            }),
            "尚无已缓存的 parse 结果；本次为纯只读回显，未读取任何文件",
        ),
    }
}

/// status 为 read_only，但写入类命令仍恒阻断，即使 allow_write 为 true 也不放行。
#[tauri::command]
pub fn plugin_sfx_aaf_import_to_session(
    parse_id: Option<String>,
    allow_write: Option<bool>,
) -> PluginResult {
    if let Some(early) = gate(ID, "to_session") {
        return early;
    }
    let _ = (parse_id, allow_write);
    blocked(ID, "to_session")
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

    fn utf16le(text: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(text.len() * 2);
        for byte in text.as_bytes() {
            out.push(*byte);
            out.push(0);
        }
        out
    }

    #[test]
    fn parse_probes_real_header_and_media_references() {
        let dir = unique_dir("aaf");
        let sample = dir.join("sample.aaf");
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(&OLE_MAGIC);
        bytes.extend_from_slice(b"\x00\x01");
        bytes.extend_from_slice(b"AAF");
        bytes.extend_from_slice(b"\x00\x02");
        bytes.extend_from_slice(b"C:\\Media\\hit.wav"); // ASCII 引用
        bytes.extend_from_slice(b"\x00");
        bytes.extend_from_slice(&utf16le("C:\\Media\\miss.wav")); // UTF-16LE 引用
        bytes.extend_from_slice(b"\x00\x00\x00");
        bytes.extend_from_slice(b"C:\\Media\\hit.wav"); // 重复，验证去重
        bytes.extend_from_slice(b"\x00");
        std::fs::write(&sample, &bytes).expect("write synthetic sample");
        // 让 hit.wav 在 media_root 下真实存在 -> resolved。
        std::fs::write(dir.join("hit.wav"), b"fake wav").expect("write fake media");

        let roots = vec![dir.to_string_lossy().into_owned()];
        let path = sample.to_string_lossy().into_owned();

        // 直接调用内部探测函数：无论 feature 开/关都验证真实只读能力。
        let snapshot = probe_file(&path, &roots).expect("probe should succeed");
        assert_eq!(snapshot.container, "ole_compound_file");
        assert_eq!(snapshot.format_guess, "AAF");
        assert!(snapshot.markers.iter().any(|item| item == "AAF"));
        assert_eq!(snapshot.method, "heuristic_header_probe");
        assert_eq!(snapshot.candidates.len(), 2, "重复引用应被去重");
        assert!(snapshot
            .candidates
            .iter()
            .any(|item| item.status == "resolved"));
        assert!(snapshot
            .candidates
            .iter()
            .any(|item| item.status == "missing"));
        assert!(snapshot
            .candidates
            .iter()
            .any(|item| item.encoding == "ascii"));
        assert!(snapshot
            .candidates
            .iter()
            .any(|item| item.encoding == "utf16le"));

        // 命令层：feature 开启时返回 ok=true / static_parse / warnings 非空。
        let result =
            plugin_sfx_aaf_import_parse(Some(path.clone()), Some(roots.clone()), Some(true));
        if cfg!(feature = "plugin-aaf-import") {
            assert!(result.ok);
            assert_eq!(result.evidence, EVIDENCE_STATIC_PARSE);
            assert!(!result.warnings.is_empty());
            assert_eq!(result.data["method"], "heuristic_header_probe");
            assert_eq!(result.data["candidate_count"], 2);
            let list = plugin_sfx_aaf_import_candidates(None, Some(false));
            assert!(list.ok);
            assert_ne!(list.evidence, "none");
            assert!(!list.warnings.is_empty());
        } else {
            assert!(!result.ok);
            assert_eq!(result.code, "PLUGIN_DISABLED");
        }

        std::fs::remove_dir_all(&dir).expect("cleanup temp dir");
    }

    #[test]
    fn parse_rejects_relative_path() {
        let result =
            plugin_sfx_aaf_import_parse(Some("relative/sample.aaf".to_string()), None, None);
        if cfg!(feature = "plugin-aaf-import") {
            assert!(!result.ok);
            assert_eq!(result.code, "INPUT_UNSUPPORTED");
            assert_eq!(result.evidence, "none");
        }
    }

    #[test]
    fn to_session_is_always_write_blocked() {
        let result = plugin_sfx_aaf_import_to_session(None, Some(true));
        assert!(!result.ok);
        assert!(result.code == "WRITE_BLOCKED" || result.code == "PLUGIN_DISABLED");
        assert_eq!(result.evidence, "none");
        if cfg!(feature = "plugin-aaf-import") {
            assert_eq!(result.code, "WRITE_BLOCKED");
        }
    }
}
