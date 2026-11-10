//! 音频-文本模型（CLAP / Xenova/clap-htsat-unfused，固定 revision）契约核验与 ONNX 张量自检。
//!
//! 只做：轻量文件契约检查、把 Python 桥写入系统临时目录后以固定解释器执行一次请求。
//! 不打开声卡、不写用户素材库、不启动 GUI、不联网；Python 桥只读本地音频与模型文件。
//! run_bridge_op 供语义向量模块复用 encode_text / encode_audio_batch，同样受固定 revision 约束。
//! 不宣称语义检索效果：未做中文相关性评测，未与 HuggingFace 官方逐帧对照验证。
//!
//! 许可边界：上游 laion/clap-htsat-unfused 为 Apache-2.0；Xenova ONNX 转换产物的
//! 再分发条款尚未单独核验，本模块不对其作任何保证，仅按固定 revision 使用本地文件。
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use uuid::Uuid;

const MODEL_ID: &str = "Xenova/clap-htsat-unfused";
pub const REVISION: &str = "c28f2883575e590e04d3146ff0713c2448d691ba";
const AUDIO_ONNX: &str = "onnx/audio_model_quantized.onnx";
const TEXT_ONNX: &str = "onnx/text_model_quantized.onnx";
const TOKENIZER: &str = "tokenizer.json";
const BRIDGE_SOURCE: &str = include_str!("../../audio_text_bridge.py");
const SELFTEST_TIMEOUT: Duration = Duration::from_secs(300);
const OUTPUT_LIMIT: usize = 4 * 1024 * 1024; // 单批 32 条 512 维向量约 370KB，留足余量
const MANIFEST_LIMIT: usize = 128 * 1024;
const READY_DETAIL: &str = "文件契约已核验（清单、两个 ONNX 与分词器长度匹配），可运行 ONNX 张量自检与本地音频/文本塔编码；已接通 CLAP 双塔余弦检索，但未做中文相关性评测、未与 HuggingFace 官方逐帧对照验证，不等于语义检索质量达标。上游 laion/clap-htsat-unfused 为 Apache-2.0，ONNX 转换产物再分发条款尚未单独核验。";

fn err(e: impl std::fmt::Display) -> String { e.to_string() }

#[derive(Clone, Debug, Serialize)]
pub struct AudioTextStatus {
    pub available: bool,
    pub model_ready: bool,
    pub contract_ready: bool,
    pub revision: Option<String>,
    pub detail: String,
    pub selftest: Option<Value>,
}

#[derive(Clone)]
pub struct AudioTextState { inner: Arc<Inner> }
struct Inner {
    paths: Paths,
    running: Arc<AtomicBool>,
    cache: Arc<Mutex<Option<Value>>>,
}
#[derive(Clone)]
struct Paths { python: PathBuf, model_dir: PathBuf, data_dir: PathBuf }

/// 词法上消除 ".."（不访问文件系统），避免把含 ".." 的路径交给 Python 桥。
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => { out.pop(); }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

impl Paths {
    fn new(data_dir: PathBuf) -> Self {
        // env!("CARGO_MANIFEST_DIR")/../build/models/clap-htsat-unfused
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let model_dir = lexical_normalize(&repo.join("build/models/clap-htsat-unfused"));
        let python = std::env::var_os("SOUND_WORKBENCH_AI_PYTHON")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("D:/Ai-Files/Tools/BuildEnv/sfx-tools-ai/Scripts/python.exe"));
        Self { python, model_dir, data_dir }
    }
}

impl AudioTextState {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            inner: Arc::new(Inner {
                paths: Paths::new(data_dir),
                running: Arc::new(AtomicBool::new(false)),
                cache: Arc::new(Mutex::new(None)),
            }),
        }
    }

    /// 轻量文件契约检查：清单存在且 model_id/revision 正确、两个 ONNX 与分词器存在且长度匹配。
    /// 不加载模型、不跑推理、不联网，供 model_registry_list 的 audio_text_ready 真实判定。
    pub fn contract_ready(&self) -> bool { self.contract_detail().is_ok() }

    fn contract_detail(&self) -> Result<String, String> {
        let dir = &self.inner.paths.model_dir;
        let manifest_path = dir.join("model-manifest.json");
        if !manifest_path.is_file() {
            return Err(format!("音频-文本模型未配置：缺少模型清单（{}）。", manifest_path.display()));
        }
        let value = read_json_file(&manifest_path, MANIFEST_LIMIT)
            .map_err(|e| format!("模型清单不可用：{e}"))?;
        if value.get("model_id").and_then(Value::as_str) != Some(MODEL_ID) {
            return Err("模型清单 model_id 与 Xenova/clap-htsat-unfused 不一致。".into());
        }
        let revision = value.get("revision").and_then(Value::as_str)
            .ok_or("模型清单缺少固定 revision。")?;
        if revision != REVISION {
            return Err("模型清单 revision 与固定值 c28f2883575e590e04d3146ff0713c2448d691ba 不一致。".into());
        }
        if value.get("license").and_then(Value::as_str).unwrap_or("").trim().is_empty() {
            return Err("模型清单缺少 license 声明。".into());
        }
        let files = value.get("files").and_then(Value::as_array)
            .ok_or("模型清单缺少 files 记录。")?;
        // 任何清单路径都必须安全（禁止 .. / 绝对子路径 / 反斜杠 / 冒号）。
        for entry in files {
            let relative = entry.get("path").and_then(Value::as_str).unwrap_or("");
            safe_join(dir, relative)?;
        }
        for relative in [AUDIO_ONNX, TEXT_ONNX, TOKENIZER] {
            let entry = files.iter()
                .find(|f| f.get("path").and_then(Value::as_str) == Some(relative))
                .ok_or_else(|| format!("模型清单缺少 {relative} 记录。"))?;
            let size = entry.get("size").and_then(Value::as_u64)
                .ok_or_else(|| format!("{relative} 缺少文件大小。"))?;
            if size == 0 { return Err(format!("{relative} 文件大小无效。")); }
            let path = safe_join(dir, relative)?;
            reject_links(&path)?;
            let meta = fs::metadata(&path).map_err(err)?;
            if !meta.is_file() || meta.len() != size {
                return Err(format!("本地模型文件缺失或长度与清单不匹配：{relative}"));
            }
        }
        Ok(REVISION.to_owned())
    }

    fn snapshot(&self) -> AudioTextStatus {
        let (contract_ready, revision, detail) = match self.contract_detail() {
            Ok(_) => (true, Some(REVISION.to_owned()), READY_DETAIL.to_owned()),
            Err(reason) => (false, None, reason),
        };
        let selftest = self.inner.cache.lock().ok().and_then(|c| c.clone());
        AudioTextStatus {
            available: contract_ready,
            model_ready: contract_ready,
            contract_ready,
            revision,
            detail,
            selftest,
        }
    }

    /// 单任务互斥：自检、单条查询、整批索引构建共用，避免同时拉起多个 ONNX 进程。
    pub fn begin(&self) -> Result<RunningGuard, String> {
        if self.inner.running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
            return Err("已有音频-文本模型任务运行，请稍后重试。".into());
        }
        Ok(RunningGuard(self.inner.running.clone()))
    }
}

pub struct RunningGuard(Arc<AtomicBool>);
impl Drop for RunningGuard {
    fn drop(&mut self) { self.0.store(false, Ordering::Release); }
}
struct ScriptGuard(PathBuf);
impl Drop for ScriptGuard {
    fn drop(&mut self) { let _ = fs::remove_file(&self.0); }
}
struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn safe_join(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if relative.is_empty() || relative.len() > 512 || path.is_absolute()
        || relative.contains(':') || relative.contains('\\') || relative.contains("..")
        || path.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(format!("清单文件路径不安全或越界：{relative}"));
    }
    Ok(root.join(path))
}

fn reject_links(path: &Path) -> Result<(), String> {
    for part in path.ancestors() {
        if let Ok(meta) = fs::symlink_metadata(part) {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    return Err("模型路径不允许链接或目录联接。".into());
                }
            }
            #[cfg(not(windows))]
            if meta.file_type().is_symlink() { return Err("模型路径不允许链接。".into()); }
        }
    }
    Ok(())
}

fn read_json_file(path: &Path, limit: usize) -> Result<Value, String> {
    reject_links(path)?;
    let file = fs::File::open(path).map_err(err)?;
    let mut bytes = Vec::new();
    let mut reader = file.take(limit as u64 + 1);
    reader.read_to_end(&mut bytes).map_err(err)?;
    if bytes.len() > limit { return Err("模型清单/登记超出允许大小。".into()); }
    serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes)).map_err(err)
}

fn read_bounded(mut reader: impl Read, limit: usize) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    let mut exceeded = false;
    loop {
        let n = reader.read(&mut buffer).map_err(err)?;
        if n == 0 { break; }
        let take = n.min(limit.saturating_sub(output.len()));
        output.extend_from_slice(&buffer[..take]);
        if take < n { exceeded = true; }
    }
    if exceeded { Err("本地推理进程输出超过允许大小。".into()) } else { Ok(output) }
}

fn run_process(mut command: Command, input: &Value, timeout: Duration) -> Result<Value, String> {
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // 不启动 GUI 窗口
    }
    let mut child = ChildGuard(command.spawn().map_err(|e| format!("无法启动本地音频-文本推理进程：{e}"))?);
    let stdout = child.0.stdout.take().ok_or("无法读取本地推理进程 stdout")?;
    let stderr = child.0.stderr.take().ok_or("无法读取本地推理进程 stderr")?;
    let out = std::thread::spawn(move || read_bounded(stdout, OUTPUT_LIMIT));
    let errors = std::thread::spawn(move || read_bounded(stderr, 64 * 1024));
    let write_result = (|| {
        let mut stdin = child.0.stdin.take().ok_or("无法打开本地推理进程 stdin".to_string())?;
        let bytes = serde_json::to_vec(input).map_err(err)?;
        stdin.write_all(&bytes).map_err(err)?;
        stdin.write_all(b"\n").map_err(err)
    })();
    let deadline = Instant::now() + timeout;
    let status = if let Err(e) = write_result {
        Err(e)
    } else {
        loop {
            if Instant::now() >= deadline {
                break Err(format!("音频-文本本地推理进程超过 {} 秒，已停止本次任务。", timeout.as_secs()));
            }
            match child.0.try_wait() {
                Ok(Some(s)) => break Ok(s),
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => break Err(format!("无法获取本地推理进程状态：{e}")),
            }
        }
    };
    if status.is_err() { let _ = child.0.kill(); let _ = child.0.wait(); }
    let output = out.join().map_err(|_| "推理输出读取线程异常")?;
    let errs = errors.join().map_err(|_| "推理错误读取线程异常")?;
    let status = status?;
    let output = output?;
    let errs = errs?;
    if !status.success() {
        return Err(format!("本地推理进程退出（{}）：{}", status,
            String::from_utf8_lossy(&errs).chars().take(2000).collect::<String>()));
    }
    let value: Value = serde_json::from_slice(&output)
        .map_err(|e| format!("本地推理进程未返回单个有效 JSON：{e}"))?;
    if value.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(value.get("error").and_then(Value::as_str)
            .unwrap_or("本地推理进程返回失败。").to_owned());
    }
    Ok(value)
}

/// 调用方即使传入 model_dir/revision 也会被覆盖，避免向量空间混用。
fn bridge_input(state: &AudioTextState, mut input: Value, revision: &str) -> Result<Value, String> {
    let map = input.as_object_mut().ok_or("桥请求必须是 JSON 对象。")?;
    map.insert("model_dir".to_owned(),
        json!(state.inner.paths.model_dir.to_string_lossy().replace('\\', "/")));
    map.insert("revision".to_owned(), json!(revision));
    Ok(input)
}

/// 一次 stdin JSON 请求 / 一次 stdout JSON 响应。model_dir 与 revision 只由本模块注入。
/// 互斥 guard 由调用方持有（见 AudioTextState::begin），整批索引构建可一次持锁连续请求。
pub fn run_bridge_op(state: &AudioTextState, input: Value, timeout: Duration) -> Result<Value, String> {
    let revision = state.contract_detail()?; // 未就位：中文原因，不写脚本、不 spawn
    let input = bridge_input(state, input, &revision)?;
    let script = std::env::temp_dir().join(format!("sw-audio-text-{}.py", Uuid::new_v4()));
    fs::write(&script, BRIDGE_SOURCE.as_bytes()).map_err(err)?;
    let _script = ScriptGuard(script.clone()); // 结束后清理临时脚本
    let mut command = Command::new(&state.inner.paths.python);
    command.args(["-B", "-X", "utf8"]).arg(&script)
        .current_dir(&state.inner.paths.data_dir)
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .env("HF_HUB_OFFLINE", "1")
        .env("TRANSFORMERS_OFFLINE", "1")
        .env("TOKENIZERS_PARALLELISM", "false");
    run_process(command, &input, timeout)
}

fn run_selftest(state: &AudioTextState) -> Result<Value, String> {
    let _running = state.begin()?; // 单任务互斥
    let value = run_bridge_op(state, json!({ "op": "selftest" }), SELFTEST_TIMEOUT)?;
    if let Ok(mut cache) = state.inner.cache.lock() { *cache = Some(value.clone()); }
    Ok(value)
}

#[tauri::command]
pub async fn audio_text_status(state: tauri::State<'_, AudioTextState>) -> Result<AudioTextStatus, String> {
    let state = state.inner().clone();
    let status = tauri::async_runtime::spawn_blocking(move || state.snapshot()).await.map_err(err)?;
    Ok(status)
}

#[tauri::command]
pub async fn audio_text_selftest(state: tauri::State<'_, AudioTextState>) -> Result<AudioTextStatus, String> {
    let state = state.inner().clone();
    let status = tauri::async_runtime::spawn_blocking(move || {
        let value = run_selftest(&state)?;
        let mut status = state.snapshot();
        status.selftest = Some(value);
        Ok::<AudioTextStatus, String>(status)
    }).await.map_err(err)??;
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("sw-audio-text-test-{}", Uuid::new_v4()));
            fs::create_dir_all(path.join("model/onnx")).unwrap();
            Self(fs::canonicalize(path).unwrap())
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
    }

    fn state(root: &Path) -> AudioTextState {
        AudioTextState {
            inner: Arc::new(Inner {
                paths: Paths {
                    python: PathBuf::from("missing-python.exe"),
                    model_dir: root.join("model"),
                    data_dir: root.to_path_buf(),
                },
                running: Arc::new(AtomicBool::new(false)),
                cache: Arc::new(Mutex::new(None)),
            }),
        }
    }

    fn write_model(root: &Path, revision: &str, lens: (usize, usize, usize), bad_path: Option<&str>) {
        let dir = root.join("model");
        fs::write(dir.join("onnx/audio_model_quantized.onnx"), vec![b'a'; lens.0]).unwrap();
        fs::write(dir.join("onnx/text_model_quantized.onnx"), vec![b't'; lens.1]).unwrap();
        fs::write(dir.join("tokenizer.json"), vec![b'k'; lens.2]).unwrap();
        let mut files = vec![
            json!({ "path": AUDIO_ONNX, "size": lens.0, "sha256": "0".repeat(64) }),
            json!({ "path": TEXT_ONNX, "size": lens.1, "sha256": "0".repeat(64) }),
            json!({ "path": TOKENIZER, "size": lens.2, "sha256": "0".repeat(64) }),
        ];
        if let Some(bad) = bad_path {
            files.push(json!({ "path": bad, "size": 1, "sha256": "0".repeat(64) }));
        }
        let manifest = json!({
            "model_id": MODEL_ID, "revision": revision, "license": "Apache-2.0", "files": files
        });
        fs::write(dir.join("model-manifest.json"), serde_json::to_vec(&manifest).unwrap()).unwrap();
    }

    #[test]
    fn contract_rejects_missing_manifest_bad_size_and_wrong_revision() {
        let temp = Temp::new();
        let state = state(&temp.0);
        assert!(!state.contract_ready(), "缺清单应为 false");
        write_model(&temp.0, REVISION, (8, 8, 8), None);
        assert!(state.contract_ready(), "齐备应为 true");

        // 文件长度与清单不符
        write_model(&temp.0, REVISION, (8, 8, 8), None);
        fs::write(temp.0.join("model/onnx/audio_model_quantized.onnx"), vec![b'a'; 99]).unwrap();
        assert!(!state.contract_ready(), "长度不符应为 false");

        // revision 错误
        let wrong = "a".repeat(40);
        write_model(&temp.0, &wrong, (8, 8, 8), None);
        assert!(!state.contract_ready(), "revision 错误应为 false");
    }

    #[test]
    fn contract_accepts_minimal_files_and_rejects_path_escape() {
        let temp = Temp::new();
        let state = state(&temp.0);
        write_model(&temp.0, REVISION, (5, 4, 3), None);
        assert!(state.contract_ready(), "占位字节但长度匹配应为 true");

        let dir = temp.0.join("model");
        for bad in ["../evil", "onnx/../../evil", "/absolute/model.onnx", "C:/evil", "onnx\\evil"] {
            assert!(safe_join(&dir, bad).is_err(), "应拒绝越界路径 {bad}");
        }
        assert!(safe_join(&dir, AUDIO_ONNX).is_ok());

        // 清单含越界路径时契约判定为 false
        write_model(&temp.0, REVISION, (5, 4, 3), Some("../evil"));
        assert!(!state.contract_ready(), "清单路径越界应为 false");
    }

    #[test]
    fn bridge_input_overrides_model_dir_and_revision() {
        let temp = Temp::new();
        let state = state(&temp.0);
        let value = bridge_input(&state, json!({
            "op": "encode_audio_batch", "model_dir": "C:/evil", "revision": "f".repeat(40)
        }), REVISION).unwrap();
        assert_eq!(value.get("revision").and_then(Value::as_str), Some(REVISION));
        let injected = value.get("model_dir").and_then(Value::as_str).unwrap();
        assert_ne!(injected, "C:/evil", "调用方传入的目录必须被覆盖");
        assert!(injected.ends_with("/model"), "应注入本状态自己的模型目录：{injected}");
        assert!(bridge_input(&state, json!([]), REVISION).is_err(), "非对象请求应被拒绝");
    }

    #[test]
    fn bridge_guard_is_mutually_exclusive_and_released() {
        let temp = Temp::new();
        let state = state(&temp.0);
        let guard = state.begin().unwrap();
        assert!(state.begin().is_err(), "第二个任务应被互斥拒绝");
        drop(guard);
        assert!(state.begin().is_ok(), "guard 释放后应可再次获取");
    }
}
