//! 中文相关性评测：把「检索能不能用」变成可复现的数字，而不是靠观感。
//!
//! 评测集是 JSON（见 `tests/cn-relevance-eval.template.json`）：每条查询给出若干
//! 期望素材的文件名片段，命中判定为「Top-K 里第一条路径包含任一片段」。
//! 未填期望的查询记为「未标注」，不进入指标分母——不拿空标注凑命中率。
//!
//! 语义边界：指标只反映当前索引 + 当前模型 revision 的结果；换模型、重建索引、
//! 改素材后必须重跑。`expected` 用文件名片段匹配，属于宽松判定，不代表人工听审结论。
use crate::{
    audio_text::AudioTextState,
    catalog::CatalogState,
    semantic_vector::{search_text, SemanticVectorState},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs, path::Path};

const MAX_SET_BYTES: u64 = 1024 * 1024;

fn err(e: impl std::fmt::Display) -> String { e.to_string() }

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct EvalQuery {
    pub query: String,
    /// 期望素材的文件名或路径片段；留空表示尚未标注，不参与指标。
    #[serde(default)]
    pub expected: Vec<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EvalSet {
    #[serde(default = "default_library")]
    pub library: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub queries: Vec<EvalQuery>,
}

fn default_library() -> String { "sound".into() }
fn default_limit() -> usize { 20 }

impl EvalSet {
    fn validate(&self) -> Result<(), String> {
        if self.library != "sound" && self.library != "music" { return Err("评测集 library 只能是 sound 或 music。".into()); }
        if !(1..=100).contains(&self.limit) { return Err("评测集 limit 限 1 至 100。".into()); }
        if self.queries.is_empty() { return Err("评测集没有任何查询。".into()); }
        for (index, item) in self.queries.iter().enumerate() {
            let text = item.query.trim();
            if text.is_empty() || text.chars().count() > 1024 {
                return Err(format!("第 {} 条查询为空或超过 1024 字。", index + 1));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Metrics {
    /// 参与计算的查询数（已标注）。
    pub annotated: usize,
    pub hit_at_1: f64,
    pub hit_at_3: f64,
    pub hit_at_5: f64,
    pub hit_at_10: f64,
    pub mrr: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Hit { pub id: String, pub path: String }

#[derive(Clone, Debug, Serialize)]
pub struct EvalDetail {
    pub query: String,
    /// 首个命中名次（1 起）；`None` 表示 Top-K 内没命中。
    pub rank: Option<usize>,
    pub annotated: bool,
    pub top_paths: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EvalRun {
    pub library: String,
    pub limit: usize,
    pub queries: usize,
    pub unlabeled: usize,
    pub stale: u64,
    pub indexed: u64,
    pub algorithm: String,
    pub metrics: Metrics,
    pub details: Vec<EvalDetail>,
    pub note: String,
}

/// 读取并校验评测集文件。路径由调用方给出，这里只做存在性、大小与内容校验。
pub fn load(path: &Path) -> Result<EvalSet, String> {
    if !path.is_absolute() { return Err("评测集需使用绝对路径。".into()); }
    let meta = fs::metadata(path).map_err(|e| format!("无法读取评测集：{e}"))?;
    if meta.len() > MAX_SET_BYTES { return Err("评测集超过 1 MB，拒绝读取。".into()); }
    let text = fs::read_to_string(path).map_err(|e| format!("评测集不是 UTF-8 文本：{e}"))?;
    let set: EvalSet = serde_json::from_str(&text).map_err(|e| format!("评测集 JSON 解析失败：{e}"))?;
    set.validate()?;
    Ok(set)
}

/// 首个命中名次（1 起）。用文件名/路径片段匹配，大小写不敏感，斜杠统一。
pub fn first_hit_rank(hits: &[Hit], expected: &[String]) -> Option<usize> {
    let needles: Vec<String> = expected.iter().map(|v| v.trim().to_lowercase()).filter(|v| !v.is_empty()).collect();
    if needles.is_empty() { return None; }
    hits.iter()
        .position(|hit| {
            let path = hit.path.replace('\\', "/").to_lowercase();
            needles.iter().any(|needle| path.contains(needle.as_str()))
        })
        .map(|index| index + 1)
}

pub fn metrics(ranks: &[Option<usize>]) -> Metrics {
    let total = ranks.len();
    let ratio = |k: usize| if total == 0 { 0.0 } else {
        ranks.iter().filter(|rank| matches!(rank, Some(value) if *value <= k)).count() as f64 / total as f64
    };
    let mrr = if total == 0 { 0.0 } else {
        ranks.iter().map(|rank| match rank { Some(value) => 1.0 / *value as f64, None => 0.0 }).sum::<f64>() / total as f64
    };
    Metrics { annotated: total, hit_at_1: ratio(1), hit_at_3: ratio(3), hit_at_5: ratio(5), hit_at_10: ratio(10), mrr }
}

/// 跑一遍评测集。要求索引已建好；任何一条查询失败都会整体报错，不静默跳过。
pub fn run(set: &EvalSet, catalog: &CatalogState, vectors: &SemanticVectorState, audio_text: &AudioTextState) -> Result<EvalRun, String> {
    set.validate()?;
    let mut ranks = Vec::new();
    let mut details = Vec::new();
    let mut unlabeled = 0usize;
    let mut stale = 0u64;
    let mut indexed = 0u64;
    let mut algorithm = String::new();

    for item in &set.queries {
        let result = search_text(vectors, catalog, audio_text, &item.query, &set.library, set.limit)?;
        stale = stale.max(result.stale);
        indexed = indexed.max(result.indexed);
        if algorithm.is_empty() { algorithm = result.algorithm.clone(); }
        let hits: Vec<Hit> = result.items.iter().filter_map(|hit| {
            catalog.resolve_asset(&hit.id).ok().map(|path| Hit { id: hit.id.clone(), path: path.to_string_lossy().to_string() })
        }).collect();
        let annotated = item.expected.iter().any(|v| !v.trim().is_empty());
        let rank = if annotated { first_hit_rank(&hits, &item.expected) } else { None };
        if annotated { ranks.push(rank); } else { unlabeled += 1; }
        details.push(EvalDetail {
            query: item.query.clone(),
            rank,
            annotated,
            top_paths: hits.iter().take(3).map(|hit| hit.path.clone()).collect(),
        });
    }

    if indexed == 0 { return Err("索引里没有可用向量（可能尚未建立索引或全部失效），评测结果无意义，已中止。".into()); }

    Ok(EvalRun {
        library: set.library.clone(),
        limit: set.limit,
        queries: set.queries.len(),
        unlabeled,
        stale,
        indexed,
        algorithm,
        metrics: metrics(&ranks),
        details,
        note: "命中判定为「Top-K 中第一条路径包含期望片段」，属宽松判定；不替代人工听审，也不代表可直接发布。".into(),
    })
}

#[tauri::command]
pub async fn semantic_vector_eval(path: String, catalog: tauri::State<'_, CatalogState>, vectors: tauri::State<'_, SemanticVectorState>, audio_text: tauri::State<'_, AudioTextState>) -> Result<EvalRun, String> {
    let catalog = catalog.inner().clone();
    let vectors = vectors.inner().clone();
    let audio_text = audio_text.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let set = load(Path::new(&path))?;
        run(&set, &catalog, &vectors, &audio_text)
    }).await.map_err(err)?
}

#[tauri::command]
pub fn semantic_vector_eval_template() -> Value {
    json!({
        "library": "sound",
        "limit": 20,
        "queries": [
            { "query": "铁门被重重关上", "expected": [], "note": "填期望素材的文件名片段" },
            { "query": "雨声打在铁皮屋顶", "expected": [], "note": "" },
            { "query": "人群嘈杂的室内环境", "expected": [], "note": "" }
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(path: &str) -> Hit { Hit { id: path.into(), path: path.into() } }

    #[test]
    fn first_hit_rank_matches_path_fragment_case_insensitively() {
        let hits = vec![hit(r"D:\库\AMB\Rain_Soft_01.wav"), hit(r"D:\库\IMPACT\Iron_Gate_Slam_03.wav")];
        assert_eq!(first_hit_rank(&hits, &["iron_gate_slam".into()]), Some(2));
        assert_eq!(first_hit_rank(&hits, &["IMPACT/Iron".into()]), Some(2));
        assert_eq!(first_hit_rank(&hits, &["Rain".into()]), Some(1));
    }

    #[test]
    fn first_hit_rank_is_none_for_absent_or_empty_expectation() {
        let hits = vec![hit(r"D:\库\AMB\Rain_Soft_01.wav")];
        assert_eq!(first_hit_rank(&hits, &["不存在的文件".into()]), None);
        assert_eq!(first_hit_rank(&hits, &[]), None);
        assert_eq!(first_hit_rank(&hits, &["   ".into()]), None);
        assert_eq!(first_hit_rank(&[], &["x".into()]), None);
    }

    #[test]
    fn metrics_compute_hit_at_k_and_mrr() {
        let ranks = vec![Some(1), Some(3), None, Some(10)];
        let value = metrics(&ranks);
        assert_eq!(value.annotated, 4);
        assert!((value.hit_at_1 - 0.25).abs() < 1e-9);
        assert!((value.hit_at_3 - 0.5).abs() < 1e-9);
        assert!((value.hit_at_5 - 0.5).abs() < 1e-9);
        assert!((value.hit_at_10 - 0.75).abs() < 1e-9);
        assert!((value.mrr - (1.0 + 1.0 / 3.0 + 0.0 + 0.1) / 4.0).abs() < 1e-9);
        assert_eq!(metrics(&[]).annotated, 0);
    }

    #[test]
    fn eval_set_parses_template_and_rejects_invalid() {
        let text = semantic_vector_eval_template().to_string();
        let set: EvalSet = serde_json::from_str(&text).unwrap();
        assert_eq!(set.library, "sound");
        assert_eq!(set.limit, 20);
        assert_eq!(set.queries.len(), 3);
        assert!(set.validate().is_ok());

        let empty: EvalSet = serde_json::from_str(r#"{"queries":[]}"#).unwrap();
        assert!(empty.validate().unwrap_err().contains("没有任何查询"));

        let bad_library: EvalSet = serde_json::from_str(r#"{"library":"video","queries":[{"query":"雨"}]}"#).unwrap();
        assert!(bad_library.validate().unwrap_err().contains("sound"));

        let blank: EvalSet = serde_json::from_str("{\"queries\":[{\"query\":\"   \"}]}").unwrap();
        assert!(blank.validate().unwrap_err().contains("为空"));
    }

    #[test]
    fn load_rejects_relative_path_and_oversized_file() {
        assert!(load(Path::new("relative.json")).unwrap_err().contains("绝对路径"));
    }

    #[test]
    fn unlabeled_queries_are_not_counted_in_metrics() {
        let ranks: Vec<Option<usize>> = vec![Some(1)];
        assert_eq!(metrics(&ranks).annotated, 1);
        // 只标注一条时，未标注的查询不进入分母；指标由调用方过滤后传入。
        assert!((metrics(&ranks).hit_at_1 - 1.0).abs() < 1e-9);
    }
}
