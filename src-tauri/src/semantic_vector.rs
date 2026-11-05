//! CLAP 音频-文本双塔向量索引与本地语义检索。
//!
//! 只读用户素材（由 Python 桥经本机 FFmpeg 解码），向量写入独立 SQLite，不改写源文件、
//! 不写目录数据库、不打开声卡、不联网。向量空间按固定 revision 隔离，不同版本不混用。
//! 语义边界：这是 CLAP 双塔余弦相似度检索；未做中文相关性评测、未与 HuggingFace 官方
//! ClapFeatureExtractor 逐帧对照验证，长音频确定性取开头 10 秒，不宣称检索质量达标。
use crate::{
    audio_text::{self, AudioTextState, REVISION},
    catalog::CatalogState,
};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, UNIX_EPOCH},
};
use uuid::Uuid;

const VERSION: u32 = 1;
const DIM: usize = 512;
const BYTES: usize = DIM * 4; // f32 little-endian，单条约 2 KiB
const BATCH: usize = 16; // Python 桥单批上限 32
const BRIDGE_TIMEOUT: Duration = Duration::from_secs(900);
const QUERY_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_ROWS: usize = 100_000;
const NOTE: &str = "CLAP 双塔余弦相似度；未做中文相关性评测，未与官方逐帧对照验证，长音频仅取开头10秒，结果不代表语义检索质量达标。";

fn err(e: impl std::fmt::Display) -> String { e.to_string() }
fn library(value: &str) -> Result<&str, String> {
    match value { "sound" | "music" => Ok(value), _ => Err("请选择音效库或音乐库。".into()) }
}
fn clip(text: &str) -> String { text.chars().take(300).collect() }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct Stamp {
    path: String, size: u64, modified: String, version: u32, revision: String,
}
#[derive(Clone, Debug)]
struct Row { id: String, path: String, size: u64, modified: String }

#[derive(Clone, Debug, Serialize)]
pub struct VectorStatus {
    pub job_id: Option<String>,
    pub status: String,
    pub processed: u64,
    pub total: u64,
    pub failed: u64,
    pub indexed: u64,
    pub error: Option<String>,
    pub revision: String,
}
impl Default for VectorStatus {
    fn default() -> Self {
        Self {
            job_id: None, status: "idle".into(), processed: 0, total: 0,
            failed: 0, indexed: 0, error: None, revision: REVISION.into(),
        }
    }
}

#[derive(Clone)]
pub struct SemanticVectorState {
    root: PathBuf,
    inner: Arc<Mutex<Inner>>,
    querying: Arc<AtomicBool>,
}
struct Inner { snapshot: VectorStatus, cancel: Option<Arc<AtomicBool>> }

impl SemanticVectorState {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            inner: Arc::new(Mutex::new(Inner { snapshot: VectorStatus::default(), cancel: None })),
            querying: Arc::new(AtomicBool::new(false)),
        }
    }
    fn db(&self) -> PathBuf { self.root.join("vectors.sqlite") }

    fn bump(&self, processed: u64, indexed: u64, error: Option<&str>) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.snapshot.processed += processed;
            inner.snapshot.indexed += indexed;
            if indexed == 0 && processed > 0 { inner.snapshot.failed += processed; }
            if let Some(reason) = error {
                if inner.snapshot.error.is_none() { inner.snapshot.error = Some(clip(reason)); }
            }
        }
    }

    fn start(&self, scope: String, catalog: CatalogState, audio_text: AudioTextState) -> Result<String, String> {
        library(&scope)?;
        if !audio_text.contract_ready() {
            return Err("CLAP 模型文件契约未就位，请先在模型下载队列完成固定 revision 下载。".into());
        }
        let bridge_guard = audio_text.begin()?; // 整批构建持锁，避免与自检/查询并发拉起 ONNX
        let id = Uuid::new_v4().to_string();
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut inner = self.inner.lock().map_err(|_| "语义向量状态锁不可用")?;
            if inner.cancel.is_some() { return Err("语义向量索引正在处理，请等待或取消。".into()); }
            inner.snapshot = VectorStatus { job_id: Some(id.clone()), status: "preparing".into(), ..Default::default() };
            inner.cancel = Some(cancel.clone());
        }
        let state = self.clone();
        let spawned = std::thread::Builder::new()
            .name("sound-workbench-semantic-vector".into())
            .spawn(move || {
                let _bridge_guard = bridge_guard;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    state.build(&scope, &catalog, &audio_text, &cancel)
                })).unwrap_or_else(|_| Err("语义向量索引线程异常，源素材未修改。".into()));
                if let Ok(mut inner) = state.inner.lock() {
                    inner.snapshot.status = if cancel.load(Ordering::Acquire) { "cancelled" }
                        else if result.is_ok() { "completed" } else { "failed" }.into();
                    if let Err(e) = result {
                        if !cancel.load(Ordering::Acquire) { inner.snapshot.error = Some(e); }
                    }
                    inner.cancel = None;
                }
            });
        if let Err(e) = spawned {
            let mut inner = self.inner.lock().map_err(|_| "语义向量状态锁不可用")?;
            inner.cancel = None;
            inner.snapshot.status = "failed".into();
            inner.snapshot.error = Some(e.to_string());
            return Err(e.to_string());
        }
        Ok(id)
    }

    fn build(&self, scope: &str, catalog: &CatalogState, audio_text: &AudioTextState, cancel: &AtomicBool) -> Result<(), String> {
        let rows = rows(catalog, scope)?;
        let conn = connection(&self.root)?;
        // 固定 revision 隔离：模型一变，旧向量全部作废，绝不与新向量混用。
        conn.execute("DELETE FROM vectors WHERE revision<>?1", [REVISION]).map_err(err)?;
        {
            let mut inner = self.inner.lock().map_err(|_| "语义向量状态锁不可用")?;
            inner.snapshot.total = rows.len() as u64;
            inner.snapshot.status = "running".into();
        }
        let mut buffer: Vec<(Row, Stamp)> = Vec::with_capacity(BATCH);
        for row in &rows {
            if cancel.load(Ordering::Acquire) { return Err("语义向量索引已取消，已入库向量保留。".into()); }
            let checked: Result<Option<(Row, Stamp)>, String> = (|| {
                let identity = stamp(catalog, row)?;
                if load(&conn, &row.id, &identity)?.is_some() { return Ok(None); }
                Ok(Some((row.clone(), identity)))
            })();
            match checked {
                Ok(None) => self.bump(1, 1, None),
                Ok(Some(pair)) => {
                    buffer.push(pair);
                    if buffer.len() >= BATCH { self.flush(&conn, catalog, audio_text, &mut buffer)?; }
                }
                Err(e) => self.bump(1, 0, Some(&e)),
            }
        }
        if !buffer.is_empty() { self.flush(&conn, catalog, audio_text, &mut buffer)?; }
        Ok(())
    }

    /// 一批最多 16 条交给 Python 桥（一次加载 audio ONNX session）。
    /// 单文件解码/推理失败只影响自身，绝不拖垮整批，也不会写入半截向量。
    fn flush(&self, conn: &Connection, catalog: &CatalogState, audio_text: &AudioTextState, buffer: &mut Vec<(Row, Stamp)>) -> Result<(), String> {
        let mut active: Vec<usize> = Vec::with_capacity(buffer.len());
        let mut paths: Vec<String> = Vec::with_capacity(buffer.len());
        for (index, (row, stamp)) in buffer.iter().enumerate() {
            let usable = catalog.resolve_asset(&row.id).ok()
                .filter(|path| *path == PathBuf::from(&stamp.path));
            if let Some(path) = usable {
                active.push(index);
                paths.push(path.to_string_lossy().replace('\\', "/"));
            } else {
                self.bump(1, 0, Some("素材在编码前已变化或不可访问，请复扫后重建。"));
            }
        }
        if !paths.is_empty() {
            let request = json!({ "op": "encode_audio_batch", "audio_paths": paths });
            match audio_text::run_bridge_op(audio_text, request, BRIDGE_TIMEOUT) {
                Ok(value) => {
                    let list = value.get("vectors").and_then(Value::as_array);
                    match list.filter(|v| v.len() == active.len()) {
                        Some(list) => {
                            for (position, index) in active.iter().enumerate() {
                                let (row, stamp) = &buffer[*index];
                                let outcome = parse_vector(&list[position])
                                    .and_then(|vector| save(conn, &row.id, stamp, &vector));
                                match outcome {
                                    Ok(()) => self.bump(1, 1, None),
                                    Err(e) => self.bump(1, 0, Some(&e)),
                                }
                            }
                            if let Some(failures) = value.get("failures").and_then(Value::as_array) {
                                if !failures.is_empty() {
                                    let reason = format!("本批 {} 条音频解码或推理失败，已跳过。", failures.len());
                                    if let Ok(mut inner) = self.inner.lock() {
                                        if inner.snapshot.error.is_none() { inner.snapshot.error = Some(reason); }
                                    }
                                }
                            }
                        }
                        None => {
                            for _ in &active { self.bump(1, 0, Some("批量编码返回条数不匹配，已跳过本批。")); }
                        }
                    }
                }
                Err(e) => {
                    let reason = format!("批量编码失败：{}", clip(&e));
                    for _ in &active { self.bump(1, 0, Some(&reason)); }
                }
            }
        }
        buffer.clear();
        Ok(())
    }

    fn status(&self, scope: &str, catalog: &CatalogState) -> Result<VectorStatus, String> {
        let mut result = self.inner.lock().map_err(|_| "语义向量状态锁不可用")?.snapshot.clone();
        result.revision = REVISION.into();
        if !self.db().is_file() { result.indexed = 0; result.total = 0; return Ok(result); }
        let conn = connection(&self.root)?;
        let mut stmt = conn.prepare("SELECT asset_id FROM vectors WHERE revision=?1").map_err(err)?;
        let ids: HashSet<String> = stmt.query_map([REVISION], |r| r.get(0)).map_err(err)?
            .collect::<Result<HashSet<_>, _>>().map_err(err)?;
        let rows = rows(catalog, scope)?;
        result.total = rows.len() as u64;
        result.indexed = rows.iter().filter(|row| ids.contains(&row.id)).count() as u64;
        Ok(result)
    }

    fn cancel(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|_| "语义向量状态锁不可用")?;
        let flag = inner.cancel.as_ref().ok_or("当前没有语义向量索引任务。")?;
        flag.store(true, Ordering::Release);
        inner.snapshot.status = "cancelling".into();
        Ok(())
    }
}

fn readonly(catalog: &CatalogState) -> Result<Connection, String> {
    let conn = Connection::open_with_flags(catalog.database_path(), OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(err)?;
    conn.busy_timeout(Duration::from_secs(5)).map_err(err)?;
    Ok(conn)
}
fn rows(catalog: &CatalogState, scope: &str) -> Result<Vec<Row>, String> {
    library(scope)?;
    let conn = readonly(catalog)?;
    let mut stmt = conn.prepare("SELECT a.id,a.path,a.size,a.modified FROM assets a JOIN asset_libraries l ON l.asset_id=a.id WHERE a.status='ready' AND l.library=?1 ORDER BY a.id LIMIT 100001").map_err(err)?;
    let result = stmt.query_map([scope], |r| Ok(Row {
        id: r.get(0)?, path: r.get(1)?, size: r.get(2)?, modified: r.get(3)?,
    })).map_err(err)?.collect::<Result<Vec<_>, _>>().map_err(err)?;
    if result.len() > MAX_ROWS {
        return Err("当前语义向量索引单次上限十万条，专项容量验收尚未完成。".into());
    }
    Ok(result)
}
fn stamp(catalog: &CatalogState, row: &Row) -> Result<Stamp, String> {
    let path = catalog.resolve_asset(&row.id)?;
    let meta = fs::metadata(&path).map_err(err)?;
    let modified = meta.modified().map_err(err)?.duration_since(UNIX_EPOCH).map_err(err)?.as_nanos().to_string();
    if path != PathBuf::from(&row.path) || meta.len() != row.size || modified != row.modified {
        return Err("素材已变化，请重新扫描后重建语义向量索引。".into());
    }
    Ok(Stamp { path: row.path.clone(), size: row.size, modified, version: VERSION, revision: REVISION.into() })
}
fn reject_links(path: &Path) -> Result<(), String> {
    for parent in path.ancestors() {
        if let Ok(meta) = fs::symlink_metadata(parent) {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 { return Err("索引路径不允许链接或目录联接。".into()); }
            }
            #[cfg(not(windows))]
            if meta.file_type().is_symlink() { return Err("索引路径不允许链接。".into()); }
        }
    }
    Ok(())
}
fn create_table(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS vectors(asset_id TEXT PRIMARY KEY,revision TEXT NOT NULL,stamp TEXT NOT NULL,payload BLOB NOT NULL); CREATE INDEX IF NOT EXISTS vectors_revision ON vectors(revision);").map_err(err)
}
fn connection(root: &Path) -> Result<Connection, String> {
    if !root.is_absolute() { return Err("语义向量索引目录必须是绝对路径。".into()); }
    reject_links(root)?;
    fs::create_dir_all(root).map_err(err)?;
    let file = root.join("vectors.sqlite");
    reject_links(&file)?;
    let conn = Connection::open(file).map_err(err)?;
    conn.busy_timeout(Duration::from_secs(5)).map_err(err)?;
    conn.execute_batch("PRAGMA journal_mode=WAL;").map_err(err)?;
    create_table(&conn)?;
    Ok(conn)
}
fn save(conn: &Connection, id: &str, stamp: &Stamp, vector: &[f32]) -> Result<(), String> {
    if vector.len() != DIM { return Err(format!("向量维度必须为 {DIM}")); }
    let mut bytes = Vec::with_capacity(BYTES);
    for value in vector { bytes.extend_from_slice(&value.to_le_bytes()); }
    conn.execute("INSERT INTO vectors(asset_id,revision,stamp,payload) VALUES(?1,?2,?3,?4) ON CONFLICT(asset_id) DO UPDATE SET revision=excluded.revision,stamp=excluded.stamp,payload=excluded.payload",
        params![id, REVISION, serde_json::to_string(stamp).map_err(err)?, bytes]).map_err(err)?;
    Ok(())
}
fn load(conn: &Connection, id: &str, stamp: &Stamp) -> Result<Option<Vec<f32>>, String> {
    let found: Option<(String, Vec<u8>)> = conn.query_row(
        "SELECT stamp,payload FROM vectors WHERE asset_id=?1 AND revision=?2 AND length(payload)=?3 AND length(stamp)<=4096",
        params![id, REVISION, BYTES as i64], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(err)?;
    let Some((identity, payload)) = found else { return Ok(None) };
    let Ok(saved) = serde_json::from_str::<Stamp>(&identity) else { return Ok(None) };
    if saved != *stamp { return Ok(None) }
    let mut values = Vec::with_capacity(DIM);
    for chunk in payload.chunks_exact(4) {
        let value = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        if !value.is_finite() { return Ok(None) }
        values.push(value);
    }
    if values.len() != DIM { return Ok(None) }
    Ok(Some(values))
}
fn parse_vector(value: &Value) -> Result<Vec<f32>, String> {
    let items = value.as_array().ok_or("编码返回了空向量（解码或推理失败）。")?;
    if items.len() != DIM { return Err(format!("向量维度应为 {DIM}，实际 {}", items.len())); }
    let mut out = Vec::with_capacity(DIM);
    for item in items {
        let number = item.as_f64().ok_or("向量含非数值元素。")?;
        let value = number as f32;
        if !value.is_finite() { return Err("向量含非有限值（NaN/Inf）。".into()); }
        out.push(value);
    }
    Ok(out)
}
fn dot(a: &[f32], b: &[f32]) -> f64 {
    a.iter().zip(b).map(|(x, y)| *x as f64 * *y as f64).sum()
}

#[derive(Clone, Debug, Serialize)]
pub struct SimilarItem { pub id: String, pub score: f64 }
#[derive(Clone, Debug, Serialize)]
pub struct SearchResult {
    pub items: Vec<SimilarItem>, pub stale: u64, pub indexed: u64,
    pub algorithm: String, pub note: String,
}
fn top(items: &mut Vec<SimilarItem>, limit: usize) {
    items.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
    items.truncate(limit);
}
fn rank(state: &SemanticVectorState, catalog: &CatalogState, query: &[f32], scope: &str, limit: usize, exclude: Option<&str>) -> Result<SearchResult, String> {
    if !(1..=100).contains(&limit) { return Err("检索数量限1至100。".into()); }
    let rows = rows(catalog, scope)?;
    let conn = connection(&state.root)?;
    let mut items = Vec::new();
    let mut stale = 0u64;
    let mut indexed = 0u64;
    for row in &rows {
        if Some(row.id.as_str()) == exclude { continue; }
        let identity = match stamp(catalog, row) { Ok(v) => v, Err(_) => { stale += 1; continue; } };
        let vector = match load(&conn, &row.id, &identity) {
            Ok(Some(v)) => v, _ => { stale += 1; continue; }
        };
        indexed += 1;
        let score = dot(query, &vector);
        if score.is_finite() { items.push(SimilarItem { id: row.id.clone(), score }); }
    }
    top(&mut items, limit);
    Ok(SearchResult {
        items, stale, indexed,
        algorithm: format!("clap-audio-text-v1@{}", &REVISION[..12]), note: NOTE.into(),
    })
}

struct QueryGuard(Arc<AtomicBool>);
impl Drop for QueryGuard {
    fn drop(&mut self) { self.0.store(false, Ordering::Release); }
}
fn begin_query(state: &SemanticVectorState) -> Result<QueryGuard, String> {
    state.querying.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "已有语义查询进行中，请稍后重试。".to_string())?;
    Ok(QueryGuard(state.querying.clone()))
}
fn search_text(state: &SemanticVectorState, catalog: &CatalogState, audio_text: &AudioTextState, text: &str, scope: &str, limit: usize) -> Result<SearchResult, String> {
    let _query = begin_query(state)?;
    library(scope)?;
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 1024 { return Err("检索文本需为 1 至 1024 个非空字符。".into()); }
    if !(1..=100).contains(&limit) { return Err("检索数量限1至100。".into()); }
    let vector = {
        let _bridge = audio_text.begin()?;
        let value = audio_text::run_bridge_op(audio_text, json!({ "op": "encode_text", "text": trimmed }), QUERY_TIMEOUT)?;
        parse_vector(value.get("vector").ok_or("文本塔未返回向量。")?)?
    }; // 释放桥互斥再做纯本地余弦排序，避免长时间占用模型任务
    rank(state, catalog, &vector, scope, limit, None)
}
fn search_audio(state: &SemanticVectorState, catalog: &CatalogState, audio_text: &AudioTextState, id: &str, scope: &str, limit: usize) -> Result<SearchResult, String> {
    let _query = begin_query(state)?;
    library(scope)?;
    if !(1..=100).contains(&limit) { return Err("检索数量限1至100。".into()); }
    let rows = rows(catalog, scope)?;
    if !rows.iter().any(|row| row.id == id) { return Err("参考素材不属于当前库。".into()); }
    let path = catalog.resolve_asset(id)?;
    let vector = {
        let _bridge = audio_text.begin()?;
        let value = audio_text::run_bridge_op(audio_text,
            json!({ "op": "encode_audio_batch", "audio_paths": [path.to_string_lossy().replace('\\', "/")] }), QUERY_TIMEOUT)?;
        let list = value.get("vectors").and_then(Value::as_array).ok_or("音频塔未返回向量。")?;
        parse_vector(list.first().ok_or("音频塔未返回向量。")?)?
    };
    rank(state, catalog, &vector, scope, limit, Some(id))
}

#[tauri::command]
pub async fn semantic_vector_build(library: String, catalog: tauri::State<'_, CatalogState>, vectors: tauri::State<'_, SemanticVectorState>, audio_text: tauri::State<'_, AudioTextState>) -> Result<String, String> {
    let vectors = vectors.inner().clone(); let catalog = catalog.inner().clone(); let audio_text = audio_text.inner().clone();
    tauri::async_runtime::spawn_blocking(move || vectors.start(library, catalog, audio_text)).await.map_err(err)?
}
#[tauri::command]
pub async fn semantic_vector_status(library: String, catalog: tauri::State<'_, CatalogState>, vectors: tauri::State<'_, SemanticVectorState>) -> Result<VectorStatus, String> {
    let vectors = vectors.inner().clone(); let catalog = catalog.inner().clone();
    tauri::async_runtime::spawn_blocking(move || vectors.status(&library, &catalog)).await.map_err(err)?
}
#[tauri::command]
pub fn semantic_vector_cancel(vectors: tauri::State<'_, SemanticVectorState>) -> Result<(), String> { vectors.cancel() }
#[tauri::command]
pub async fn semantic_vector_search_text(text: String, library: String, limit: usize, catalog: tauri::State<'_, CatalogState>, vectors: tauri::State<'_, SemanticVectorState>, audio_text: tauri::State<'_, AudioTextState>) -> Result<SearchResult, String> {
    let vectors = vectors.inner().clone(); let catalog = catalog.inner().clone(); let audio_text = audio_text.inner().clone();
    tauri::async_runtime::spawn_blocking(move || search_text(&vectors, &catalog, &audio_text, &text, &library, limit)).await.map_err(err)?
}
#[tauri::command]
pub async fn semantic_vector_search_audio(id: String, library: String, limit: usize, catalog: tauri::State<'_, CatalogState>, vectors: tauri::State<'_, SemanticVectorState>, audio_text: tauri::State<'_, AudioTextState>) -> Result<SearchResult, String> {
    let vectors = vectors.inner().clone(); let catalog = catalog.inner().clone(); let audio_text = audio_text.inner().clone();
    tauri::async_runtime::spawn_blocking(move || search_audio(&vectors, &catalog, &audio_text, &id, &library, limit)).await.map_err(err)?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        create_table(&conn).unwrap();
        conn
    }
    fn stamp_of(path: &str) -> Stamp {
        Stamp { path: path.into(), size: 10, modified: "123".into(), version: VERSION, revision: REVISION.into() }
    }
    fn unit(value: f32) -> Vec<f32> { let mut v = vec![0.0; DIM]; v[0] = value; v }

    #[test]
    fn vector_blob_roundtrip_and_stamp_mismatch_invalidates() {
        let conn = memory();
        let stamp = stamp_of("D:/media/a.wav");
        let vector = unit(0.75);
        save(&conn, "asset-1", &stamp, &vector).unwrap();
        let restored = load(&conn, "asset-1", &stamp).unwrap().unwrap();
        assert_eq!(restored.len(), DIM);
        assert!((restored[0] - 0.75).abs() < 1e-6);
        assert!(load(&conn, "asset-1", &stamp_of("D:/media/b.wav")).unwrap().is_none(), "路径变化应作废");
        assert!(load(&conn, "missing", &stamp).unwrap().is_none());
        conn.execute("UPDATE vectors SET payload=zeroblob(8) WHERE asset_id='asset-1'", []).unwrap();
        assert!(load(&conn, "asset-1", &stamp).unwrap().is_none(), "长度不符的 BLOB 应作废");
    }

    #[test]
    fn revision_isolation_rejects_foreign_vectors() {
        let conn = memory();
        let stamp = stamp_of("D:/media/a.wav");
        save(&conn, "asset-1", &stamp, &unit(1.0)).unwrap();
        conn.execute("UPDATE vectors SET revision='other-revision' WHERE asset_id='asset-1'", []).unwrap();
        assert!(load(&conn, "asset-1", &stamp).unwrap().is_none(), "不同模型版本的向量不得混用");
    }

    #[test]
    fn parse_vector_rejects_null_bad_dimension_and_non_finite() {
        assert!(parse_vector(&Value::Null).is_err());
        assert!(parse_vector(&json!([1.0, 2.0])).is_err());
        let mut short = vec![json!(0.0); DIM];
        short[3] = json!("bad");
        assert!(parse_vector(&Value::Array(short)).is_err());
        let mut huge = vec![json!(0.0); DIM];
        huge[3] = json!(1e300);
        assert!(parse_vector(&Value::Array(huge)).is_err(), "f32 溢出为非有限值应拒绝");
        assert_eq!(parse_vector(&Value::Array(vec![json!(0.0); DIM])).unwrap().len(), DIM);
    }

    #[test]
    fn ranking_is_descending_deterministic_and_bounded() {
        let mut items = vec![
            SimilarItem { id: "low".into(), score: -0.2 },
            SimilarItem { id: "high".into(), score: 0.9 },
            SimilarItem { id: "tie-b".into(), score: 0.5 },
            SimilarItem { id: "tie-a".into(), score: 0.5 },
        ];
        top(&mut items, 3);
        assert_eq!(items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["high", "tie-a", "tie-b"]);
        assert!(dot(&unit(1.0), &unit(1.0)) > dot(&unit(1.0), &unit(0.1)));
    }

    #[test]
    fn scope_and_status_defaults_are_explicit() {
        assert!(library("sound").is_ok());
        assert!(library("music").is_ok());
        assert!(library("all").is_err());
        let status = VectorStatus::default();
        assert_eq!(status.revision, REVISION);
        assert_eq!(status.status, "idle");
        let relative = SemanticVectorState::new(PathBuf::from("relative-dir"));
        assert!(connection(&relative.root).is_err(), "索引目录必须绝对路径");
    }

    #[test]
    fn concurrency_guard_releases_after_error_path() {
        let state = SemanticVectorState::new(std::env::temp_dir().join("sw-semantic-guard-test"));
        let guard = begin_query(&state).unwrap();
        assert!(begin_query(&state).is_err(), "并发查询应被拒绝");
        drop(guard);
        assert!(begin_query(&state).is_ok(), "guard 释放后应可再次查询");
    }
}
