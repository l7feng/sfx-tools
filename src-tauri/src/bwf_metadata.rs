//! BWF / iXML 元数据读写：让音效标签随文件走。
//!
//! 只处理 RIFF/WAVE 容器：读取 `bext`（Broadcast Wave Extension）与 `iXML` chunk，
//! 回写时保留其余 chunk 逐字节不变，只替换或插入这两个 chunk，并修正 RIFF 长度。
//!
//! 语义边界（如实记录，不外推）：
//! - 不解析完整 XML。回写按已知标签做外科替换，未知元素与厂商私有块原样保留。
//! - 私有字段统一加 `SW_` 前缀（`SW_CATEGORY` 等），避免与 Soundminer 等同名块冲突。
//! - `RF64`（超过 4GB）只读不写；非 RIFF/WAVE 文件直接拒绝。
//! - 回写先写同目录临时文件、回读校验通过后再原子替换，失败时原文件保持不变。
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

const MAX_BEXT: usize = 64 * 1024;
const MAX_IXML: usize = 8 * 1024 * 1024;
const BEXT_V1: usize = 602;
const IXML_HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>";
const COPY_BUF: usize = 64 * 1024;

fn err(e: impl std::fmt::Display) -> String { e.to_string() }

/// 读取到的元数据。缺失字段一律返回空串或 0，不用 `None` 区分「没有」与「空」。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct BwfMetadata {
    pub file_size: u64,
    pub has_bext: bool,
    pub has_ixml: bool,
    /// 容器内所有 chunk 标识，按文件顺序；便于排查非标准文件。
    pub chunk_ids: Vec<String>,
    pub description: String,
    pub originator: String,
    pub originator_reference: String,
    pub origination_date: String,
    pub origination_time: String,
    pub time_reference: u64,
    pub project: String,
    pub scene: String,
    pub take: String,
    pub tape: String,
    pub note: String,
    pub category: String,
    pub keywords: String,
    pub license: String,
    pub creator: String,
    pub source: String,
    pub ucs: String,
}

/// 回写补丁：只写 `Some` 的字段，`None` 表示保持原值不动。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct MetadataPatch {
    pub description: Option<String>,
    pub originator: Option<String>,
    pub originator_reference: Option<String>,
    pub origination_date: Option<String>,
    pub origination_time: Option<String>,
    pub time_reference: Option<u64>,
    pub project: Option<String>,
    pub scene: Option<String>,
    pub take: Option<String>,
    pub tape: Option<String>,
    pub note: Option<String>,
    pub category: Option<String>,
    pub keywords: Option<String>,
    pub license: Option<String>,
    pub creator: Option<String>,
    pub source: Option<String>,
    pub ucs: Option<String>,
}

impl MetadataPatch {
    fn touches_bext(&self) -> bool {
        self.description.is_some()
            || self.originator.is_some()
            || self.originator_reference.is_some()
            || self.origination_date.is_some()
            || self.origination_time.is_some()
            || self.time_reference.is_some()
    }
    fn touches_ixml(&self) -> bool {
        self.project.is_some()
            || self.scene.is_some()
            || self.take.is_some()
            || self.tape.is_some()
            || self.note.is_some()
            || self.category.is_some()
            || self.keywords.is_some()
            || self.license.is_some()
            || self.creator.is_some()
            || self.source.is_some()
            || self.ucs.is_some()
    }
    fn is_empty(&self) -> bool { !self.touches_bext() && !self.touches_ixml() }
}

/// 回写结果。`rescan_required` 恒为真：文件大小与修改时间已变，目录库需重扫才能刷新。
#[derive(Clone, Debug, Serialize)]
pub struct WriteReport {
    pub metadata: BwfMetadata,
    pub bytes_before: u64,
    pub bytes_after: u64,
    pub bext_written: bool,
    pub ixml_written: bool,
    pub rescan_required: bool,
}

#[derive(Clone, Debug)]
struct Chunk { id: String, raw: [u8; 4], data_start: u64, size: u32 }

impl Chunk { fn is(&self, id: &str) -> bool { self.id == id } }

fn scan_chunks(file: &mut File, len: u64) -> Result<(Vec<Chunk>, bool), String> {
    if len < 12 { return Err("文件小于 12 字节，不是有效的 RIFF/WAVE。".into()); }
    let mut head = [0u8; 12];
    file.seek(SeekFrom::Start(0)).map_err(err)?;
    file.read_exact(&mut head).map_err(err)?;
    let rf64 = match &head[0..4] {
        b"RIFF" => false,
        b"RF64" => true,
        _ => return Err("不是 RIFF/WAVE 文件，未做任何写入。".into()),
    };
    if &head[8..12] != b"WAVE" { return Err("RIFF 类型不是 WAVE，未做任何写入。".into()); }
    let mut chunks = Vec::new();
    let mut pos: u64 = 12;
    while pos + 8 <= len {
        file.seek(SeekFrom::Start(pos)).map_err(err)?;
        let mut hdr = [0u8; 8];
        if file.read_exact(&mut hdr).is_err() { break; }
        let id = String::from_utf8_lossy(&hdr[0..4]).to_string();
        let mut raw = [0u8; 4];
        raw.copy_from_slice(&hdr[0..4]);
        let declared = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]);
        let data_start = pos + 8;
        // 截断文件按实际可读长度收敛，读到多少算多少；回写则另行校验。
        let available = len.saturating_sub(data_start);
        let size = declared.min(available.min(u32::MAX as u64) as u32);
        if size < declared { return Err(format!("chunk `{id}` 声明 {declared} 字节但文件只剩 {available} 字节，文件可能被截断。")); }
        chunks.push(Chunk { id, raw, data_start, size });
        pos = data_start + size as u64 + (size % 2) as u64;
    }
    Ok((chunks, rf64))
}

fn read_chunk(file: &mut File, chunk: &Chunk, limit: usize) -> Result<Vec<u8>, String> {
    if chunk.size as usize > limit {
        return Err(format!("chunk `{}` 超过 {} 字节上限，拒绝读取。", chunk.id, limit));
    }
    file.seek(SeekFrom::Start(chunk.data_start)).map_err(err)?;
    let mut data = vec![0u8; chunk.size as usize];
    file.read_exact(&mut data).map_err(err)?;
    Ok(data)
}

fn text_at(bytes: &[u8], start: usize, len: usize) -> String {
    if start >= bytes.len() { return String::new(); }
    let end = (start + len).min(bytes.len());
    let raw = &bytes[start..end];
    let cut = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..cut]).trim_end().to_string()
}

fn parse_bext(bytes: &[u8], meta: &mut BwfMetadata) {
    meta.has_bext = true;
    meta.description = text_at(bytes, 0, 256);
    meta.originator = text_at(bytes, 256, 32);
    meta.originator_reference = text_at(bytes, 288, 32);
    meta.origination_date = text_at(bytes, 320, 10);
    meta.origination_time = text_at(bytes, 330, 8);
    if bytes.len() >= 346 {
        let mut raw = [0u8; 8];
        raw.copy_from_slice(&bytes[338..346]);
        meta.time_reference = u64::from_le_bytes(raw);
    }
}

fn tag_value(xml: &str, name: &str) -> String {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    if let Some(start) = xml.find(&open) {
        let from = start + open.len();
        if let Some(end) = xml[from..].find(&close) { return unescape(&xml[from..from + end]); }
    }
    String::new()
}

fn parse_ixml(bytes: &[u8], meta: &mut BwfMetadata) {
    meta.has_ixml = true;
    let xml = String::from_utf8_lossy(bytes).to_string();
    meta.project = tag_value(&xml, "PROJECT");
    meta.scene = tag_value(&xml, "SCENE");
    meta.take = tag_value(&xml, "TAKE");
    meta.tape = tag_value(&xml, "TAPE");
    meta.note = tag_value(&xml, "NOTE");
    meta.category = tag_value(&xml, "SW_CATEGORY");
    meta.keywords = tag_value(&xml, "SW_KEYWORDS");
    meta.license = tag_value(&xml, "SW_LICENSE");
    meta.creator = tag_value(&xml, "SW_CREATOR");
    meta.source = tag_value(&xml, "SW_SOURCE");
    meta.ucs = tag_value(&xml, "SW_UCS");
}

/// 读取单个音频文件的 BWF/iXML 元数据；非 RIFF/WAVE 返回错误而不是空结构。
pub fn read(path: &Path) -> Result<BwfMetadata, String> {
    let mut file = File::open(path).map_err(|e| format!("无法打开文件：{e}"))?;
    let len = file.metadata().map_err(err)?.len();
    let (chunks, _) = scan_chunks(&mut file, len)?;
    let mut meta = BwfMetadata { file_size: len, chunk_ids: chunks.iter().map(|c| c.id.clone()).collect(), ..Default::default() };
    if let Some(chunk) = chunks.iter().find(|c| c.is("bext")) {
        let data = read_chunk(&mut file, chunk, MAX_BEXT)?;
        parse_bext(&data, &mut meta);
    }
    if let Some(chunk) = chunks.iter().find(|c| c.is("iXML")) {
        let data = read_chunk(&mut file, chunk, MAX_IXML)?;
        parse_ixml(&data, &mut meta);
    }
    Ok(meta)
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// 只替换已知标签的内容；找不到就在 `</BWFXML>` 前插入，未知元素保持原样。
fn set_tag(xml: &str, name: &str, value: &str) -> String {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let escaped = escape(value);
    if let Some(start) = xml.find(&open) {
        let from = start + open.len();
        if let Some(end) = xml[from..].find(&close) {
            return format!("{}{}{}", &xml[..from], escaped, &xml[from + end..]);
        }
    }
    let self_closing = format!("<{name}/>");
    if let Some(at) = xml.find(&self_closing) {
        return format!("{}<{name}>{escaped}</{name}>{}", &xml[..at], &xml[at + self_closing.len()..]);
    }
    if let Some(at) = xml.rfind("</BWFXML>") {
        return format!("{}<{name}>{escaped}</{name}>{}", &xml[..at], &xml[at..]);
    }
    format!("{xml}<{name}>{escaped}</{name}>")
}

fn new_ixml_document() -> String { format!("{IXML_HEAD}\n<BWFXML>\n<IXML_VERSION>1.5</IXML_VERSION>\n</BWFXML>") }

fn ixml_pairs(patch: &MetadataPatch) -> Vec<(&'static str, Option<&str>)> {
    vec![
        ("PROJECT", patch.project.as_deref()),
        ("SCENE", patch.scene.as_deref()),
        ("TAKE", patch.take.as_deref()),
        ("TAPE", patch.tape.as_deref()),
        ("NOTE", patch.note.as_deref()),
        ("SW_CATEGORY", patch.category.as_deref()),
        ("SW_KEYWORDS", patch.keywords.as_deref()),
        ("SW_LICENSE", patch.license.as_deref()),
        ("SW_CREATOR", patch.creator.as_deref()),
        ("SW_SOURCE", patch.source.as_deref()),
        ("SW_UCS", patch.ucs.as_deref()),
    ]
}

fn put_fixed(data: &mut [u8], start: usize, width: usize, label: &str, value: &str) -> Result<(), String> {
    let raw = value.as_bytes();
    if raw.len() > width {
        return Err(format!("{label} 最长 {width} 字节（UTF-8），当前 {} 字节；请缩短后重试，未做截断。", raw.len()));
    }
    for byte in &mut data[start..start + width] { *byte = 0; }
    data[start..start + raw.len()].copy_from_slice(raw);
    Ok(())
}

fn patch_bext(data: &mut Vec<u8>, patch: &MetadataPatch) -> Result<(), String> {
    if data.len() < 346 { return Err("现有 bext chunk 长度异常（不足 346 字节），已拒绝回写以免破坏文件。".into()); }
    if let Some(value) = &patch.description { put_fixed(data, 0, 256, "bext 描述", value)?; }
    if let Some(value) = &patch.originator { put_fixed(data, 256, 32, "bext 制作者", value)?; }
    if let Some(value) = &patch.originator_reference { put_fixed(data, 288, 32, "bext 制作者参考号", value)?; }
    if let Some(value) = &patch.origination_date { put_fixed(data, 320, 10, "bext 录制日期", value)?; }
    if let Some(value) = &patch.origination_time { put_fixed(data, 330, 8, "bext 录制时间", value)?; }
    if let Some(value) = patch.time_reference { data[338..346].copy_from_slice(&value.to_le_bytes()); }
    Ok(())
}

fn build_bext(patch: &MetadataPatch) -> Result<Vec<u8>, String> {
    let mut data = vec![0u8; BEXT_V1];
    data[346..348].copy_from_slice(&1u16.to_le_bytes());
    patch_bext(&mut data, patch)?;
    Ok(data)
}

fn temp_path(path: &Path) -> PathBuf {
    let name = path.file_name().map(|v| v.to_string_lossy().to_string()).unwrap_or_else(|| "audio".into());
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    dir.join(format!(".{name}.swmeta-{}.tmp", uuid::Uuid::new_v4()))
}

struct TempGuard(PathBuf);
impl TempGuard { fn forget(mut self) { self.0 = PathBuf::new(); } }
impl Drop for TempGuard {
    fn drop(&mut self) { if !self.0.as_os_str().is_empty() { let _ = fs::remove_file(&self.0); } }
}

fn write_chunk(out: &mut File, id: &[u8; 4], data: &[u8]) -> Result<(), String> {
    out.write_all(id).map_err(err)?;
    out.write_all(&(data.len() as u32).to_le_bytes()).map_err(err)?;
    out.write_all(data).map_err(err)?;
    if data.len() % 2 == 1 { out.write_all(&[0]).map_err(err)?; }
    Ok(())
}

fn copy_chunk(input: &mut File, chunk: &Chunk, out: &mut File) -> Result<(), String> {
    // 连 chunk 头一起原样复制，长度字段也沿用原值，避免重算引入偏差。
    out.write_all(&chunk.raw).map_err(err)?;
    out.write_all(&chunk.size.to_le_bytes()).map_err(err)?;
    input.seek(SeekFrom::Start(chunk.data_start)).map_err(err)?;
    let mut left = chunk.size as u64;
    let mut buf = vec![0u8; COPY_BUF];
    while left > 0 {
        let take = left.min(COPY_BUF as u64) as usize;
        input.read_exact(&mut buf[..take]).map_err(err)?;
        out.write_all(&buf[..take]).map_err(err)?;
        left -= take as u64;
    }
    if chunk.size % 2 == 1 { out.write_all(&[0]).map_err(err)?; }
    Ok(())
}

/// 回写元数据。先写临时文件、回读校验，再原子替换；任何一步失败都保持原文件不变。
pub fn write(path: &Path, patch: &MetadataPatch) -> Result<WriteReport, String> {
    if patch.is_empty() { return Err("没有需要写入的字段。".into()); }
    let mut input = File::open(path).map_err(|e| format!("无法打开文件：{e}"))?;
    let len = input.metadata().map_err(err)?.len();
    let (chunks, rf64) = scan_chunks(&mut input, len)?;
    if rf64 { return Err("RF64（超过 4GB）文件的元数据回写尚未实现，未做任何写入。".into()); }
    if !chunks.iter().any(|c| c.is("fmt ")) { return Err("WAVE 缺少 fmt chunk，疑似非标准文件，未做任何写入。".into()); }

    let mut bext_data = match chunks.iter().find(|c| c.is("bext")) {
        Some(chunk) => Some(read_chunk(&mut input, chunk, MAX_BEXT)?),
        None => None,
    };
    let mut ixml_data = match chunks.iter().find(|c| c.is("iXML")) {
        Some(chunk) => Some(read_chunk(&mut input, chunk, MAX_IXML)?),
        None => None,
    };

    let mut bext_written = false;
    if patch.touches_bext() {
        match &mut bext_data {
            Some(data) => patch_bext(data, patch)?,
            None => bext_data = Some(build_bext(patch)?),
        }
        bext_written = true;
    }

    let mut ixml_written = false;
    if patch.touches_ixml() {
        let base = ixml_data.as_ref().map(|v| String::from_utf8_lossy(v).to_string()).unwrap_or_else(new_ixml_document);
        let mut text = base;
        for (name, value) in ixml_pairs(patch) {
            if let Some(value) = value { text = set_tag(&text, name, value); }
        }
        ixml_data = Some(text.into_bytes());
        ixml_written = true;
    }

    let temp = temp_path(path);
    let guard = TempGuard(temp.clone());
    let mut out = File::create(&temp).map_err(|e| format!("无法创建临时文件：{e}"))?;
    out.write_all(b"RIFF").map_err(err)?;
    out.write_all(&0u32.to_le_bytes()).map_err(err)?;
    out.write_all(b"WAVE").map_err(err)?;

    let had_bext = chunks.iter().any(|c| c.is("bext"));
    let mut placed_bext = false;
    let mut placed_ixml = false;
    for chunk in &chunks {
        if chunk.is("bext") {
            if let Some(data) = &bext_data { write_chunk(&mut out, b"bext", data)?; placed_bext = true; }
            continue;
        }
        if chunk.is("iXML") {
            if let Some(data) = &ixml_data { write_chunk(&mut out, b"iXML", data)?; placed_ixml = true; }
            continue;
        }
        copy_chunk(&mut input, chunk, &mut out)?;
        // BWF 约定 bext 紧随 fmt；仅在原文件没有 bext 时补位，避免写出重复 chunk。
        if chunk.is("fmt ") && !had_bext && !placed_bext {
            if let Some(data) = &bext_data { write_chunk(&mut out, b"bext", data)?; placed_bext = true; }
        }
    }
    if !placed_bext {
        if let Some(data) = &bext_data { write_chunk(&mut out, b"bext", data)?; }
    }
    if !placed_ixml {
        if let Some(data) = &ixml_data { write_chunk(&mut out, b"iXML", data)?; }
    }

    let written = out.stream_position().map_err(err)?;
    let riff_size = written.checked_sub(8).ok_or("写出长度异常")?;
    if riff_size > u32::MAX as u64 { return Err("回写后超过 4GB，RIFF 无法表示，未做任何写入。".into()); }
    out.seek(SeekFrom::Start(4)).map_err(err)?;
    out.write_all(&(riff_size as u32).to_le_bytes()).map_err(err)?;
    out.sync_all().map_err(err)?;
    drop(out);

    let check = read(&temp)?;
    if let Some(value) = &patch.description { if &check.description != value { return Err("回写校验失败（bext 描述不一致），原文件未改动。".into()); } }
    if let Some(value) = &patch.note { if &check.note != value { return Err("回写校验失败（iXML NOTE 不一致），原文件未改动。".into()); } }
    if let Some(value) = &patch.category { if &check.category != value { return Err("回写校验失败（SW_CATEGORY 不一致），原文件未改动。".into()); } }
    if let Some(value) = patch.time_reference { if check.time_reference != value { return Err("回写校验失败（timeReference 不一致），原文件未改动。".into()); } }

    fs::rename(&temp, path).map_err(|e| format!("临时文件替换失败：{e}"))?;
    guard.forget();
    let after = fs::metadata(path).map_err(err)?.len();
    Ok(WriteReport {
        metadata: read(path)?,
        bytes_before: len,
        bytes_after: after,
        bext_written,
        ixml_written,
        rescan_required: true,
    })
}

#[tauri::command]
pub async fn bwf_read_metadata(catalog: tauri::State<'_, crate::catalog::CatalogState>, id: String) -> Result<BwfMetadata, String> {
    let catalog = catalog.inner().clone();
    let path = tauri::async_runtime::spawn_blocking(move || catalog.resolve_asset(&id)).await.map_err(err)??;
    tauri::async_runtime::spawn_blocking(move || read(&path)).await.map_err(err)?
}

#[tauri::command]
pub async fn bwf_write_metadata(catalog: tauri::State<'_, crate::catalog::CatalogState>, id: String, patch: MetadataPatch) -> Result<WriteReport, String> {
    let catalog = catalog.inner().clone();
    let path = tauri::async_runtime::spawn_blocking(move || catalog.resolve_asset(&id)).await.map_err(err)??;
    tauri::async_runtime::spawn_blocking(move || write(&path, &patch)).await.map_err(err)?
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("sw-bwf-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, bytes).unwrap();
            path
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
    }

    fn chunk(id: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(id);
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
        if data.len() % 2 == 1 { out.push(0); }
        out
    }

    fn wav(chunks: Vec<Vec<u8>>) -> Vec<u8> {
        let mut body = Vec::new();
        for item in chunks { body.extend_from_slice(&item); }
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(&body);
        out
    }

    fn fmt_chunk() -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&1u16.to_le_bytes()); // PCM
        data.extend_from_slice(&1u16.to_le_bytes()); // 单声道
        data.extend_from_slice(&48000u32.to_le_bytes());
        data.extend_from_slice(&96000u32.to_le_bytes());
        data.extend_from_slice(&2u16.to_le_bytes());
        data.extend_from_slice(&16u16.to_le_bytes());
        chunk(b"fmt ", &data)
    }

    fn bext_chunk(description: &str) -> Vec<u8> {
        let mut data = vec![0u8; BEXT_V1];
        let raw = description.as_bytes();
        data[..raw.len()].copy_from_slice(raw);
        data[346..348].copy_from_slice(&1u16.to_le_bytes());
        chunk(b"bext", &data)
    }

    fn ixml_chunk(xml: &str) -> Vec<u8> { chunk(b"iXML", xml.as_bytes()) }

    #[test]
    fn reads_bext_and_ixml_fields() {
        let temp = Temp::new();
        let xml = format!("{IXML_HEAD}<BWFXML><PROJECT>剧集A</PROJECT><NOTE>铁门撞击</NOTE><SW_CATEGORY>IMPACT</SW_CATEGORY><SW_UCS>IMPACT_Metal_001</SW_UCS></BWFXML>");
        let path = temp.file("a.wav", &wav(vec![fmt_chunk(), bext_chunk("门撞击 01"), ixml_chunk(&xml)]));
        let meta = read(&path).unwrap();
        assert!(meta.has_bext && meta.has_ixml);
        assert_eq!(meta.description, "门撞击 01");
        assert_eq!(meta.project, "剧集A");
        assert_eq!(meta.note, "铁门撞击");
        assert_eq!(meta.category, "IMPACT");
        assert_eq!(meta.ucs, "IMPACT_Metal_001");
        assert_eq!(meta.chunk_ids, vec!["fmt ", "bext", "iXML"]);
    }

    #[test]
    fn missing_metadata_reads_empty_without_error() {
        let temp = Temp::new();
        let path = temp.file("plain.wav", &wav(vec![fmt_chunk()]));
        let meta = read(&path).unwrap();
        assert!(!meta.has_bext && !meta.has_ixml);
        assert_eq!(meta.description, "");
        assert_eq!(meta.time_reference, 0);
    }

    #[test]
    fn write_roundtrip_updates_fields_and_keeps_other_chunks() {
        let temp = Temp::new();
        let junk: Vec<u8> = (0..7u8).collect(); // 奇数长度，验证补位
        let path = temp.file("b.wav", &wav(vec![fmt_chunk(), bext_chunk("旧描述"), chunk(b"junk", &junk)]));
        let patch = MetadataPatch {
            description: Some("新描述".into()),
            project: Some("剧集B".into()),
            note: Some("风声".into()),
            category: Some("AMB".into()),
            ..Default::default()
        };
        let report = write(&path, &patch).unwrap();
        assert!(report.bext_written && report.ixml_written);
        assert!(report.rescan_required);
        let meta = read(&path).unwrap();
        assert_eq!(meta.description, "新描述");
        assert_eq!(meta.project, "剧集B");
        assert_eq!(meta.note, "风声");
        assert_eq!(meta.category, "AMB");
        // junk 数据与补位字节原样保留
        let bytes = fs::read(&path).unwrap();
        let at = bytes.windows(4).position(|w| w == b"junk").unwrap();
        assert_eq!(&bytes[at + 4..at + 8], &(junk.len() as u32).to_le_bytes());
        assert_eq!(&bytes[at + 8..at + 8 + junk.len()], junk.as_slice());
        assert_eq!(bytes[at + 8 + junk.len()], 0);
    }

    #[test]
    fn write_preserves_unknown_ixml_elements() {
        let temp = Temp::new();
        let xml = format!("{IXML_HEAD}<BWFXML><SPEED><TIMECODE_RATE>25/1</TIMECODE_RATE></SPEED><USER><X>保留我</X></USER><NOTE>旧</NOTE></BWFXML>");
        let path = temp.file("c.wav", &wav(vec![fmt_chunk(), ixml_chunk(&xml)]));
        let report = write(&path, &MetadataPatch { note: Some("新".into()), ..Default::default() }).unwrap();
        assert!(!report.bext_written);
        let text = String::from_utf8_lossy(&fs::read(&path).unwrap()).to_string();
        assert!(text.contains("<TIMECODE_RATE>25/1</TIMECODE_RATE>"));
        assert!(text.contains("<X>保留我</X>"));
        assert!(text.contains("<NOTE>新</NOTE>"));
        assert!(!text.contains("<NOTE>旧</NOTE>"));
    }

    #[test]
    fn write_creates_bext_after_fmt_when_missing() {
        let temp = Temp::new();
        let path = temp.file("d.wav", &wav(vec![fmt_chunk(), chunk(b"data", &[0, 1, 2, 3])]));
        let report = write(&path, &MetadataPatch { description: Some("新增".into()), ..Default::default() }).unwrap();
        assert_eq!(report.metadata.chunk_ids, vec!["fmt ", "bext", "data"]);
        assert_eq!(report.metadata.description, "新增");
        assert_eq!(report.metadata.time_reference, 0);
    }

    #[test]
    fn bext_field_over_width_is_rejected_without_touching_file() {
        let temp = Temp::new();
        let path = temp.file("e.wav", &wav(vec![fmt_chunk(), bext_chunk("原样")]));
        let before = fs::read(&path).unwrap();
        let patch = MetadataPatch { description: Some("超".repeat(120)), ..Default::default() };
        let error = write(&path, &patch).unwrap_err();
        assert!(error.contains("最长 256 字节"));
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn non_riff_and_empty_patch_are_rejected() {
        let temp = Temp::new();
        let path = temp.file("f.bin", b"not a wav file at all");
        assert!(read(&path).unwrap_err().contains("RIFF"));
        let wav_path = temp.file("g.wav", &wav(vec![fmt_chunk()]));
        assert!(write(&wav_path, &MetadataPatch::default()).unwrap_err().contains("没有需要写入的字段"));
    }

    #[test]
    fn rf64_is_readable_but_not_writable() {
        let temp = Temp::new();
        let mut bytes = wav(vec![fmt_chunk()]);
        bytes[0..4].copy_from_slice(b"RF64");
        let path = temp.file("h.wav", &bytes);
        assert!(read(&path).is_ok());
        let patch = MetadataPatch { note: Some("x".into()), ..Default::default() };
        assert!(write(&path, &patch).unwrap_err().contains("RF64"));
    }

    #[test]
    fn xml_special_characters_roundtrip() {
        let temp = Temp::new();
        let path = temp.file("i.wav", &wav(vec![fmt_chunk()]));
        let value = "门 <a> & \"b\" 'c'";
        write(&path, &MetadataPatch { keywords: Some(value.into()), ..Default::default() }).unwrap();
        let meta = read(&path).unwrap();
        assert_eq!(meta.keywords, value);
        let text = String::from_utf8_lossy(&fs::read(&path).unwrap()).to_string();
        assert!(text.contains("&lt;a&gt;") && text.contains("&amp;"));
    }

    #[test]
    fn truncated_chunk_is_reported() {
        let temp = Temp::new();
        let mut bytes = wav(vec![fmt_chunk(), ixml_chunk("<BWFXML></BWFXML>")]);
        bytes.truncate(bytes.len() - 5);
        let path = temp.file("j.wav", &bytes);
        assert!(read(&path).unwrap_err().contains("截断"));
    }

    fn data_payload(path: &Path) -> Vec<u8> {
        let mut file = File::open(path).unwrap();
        let len = file.metadata().unwrap().len();
        let (chunks, _) = scan_chunks(&mut file, len).unwrap();
        let chunk = chunks.iter().find(|c| c.is("data")).expect("缺少 data chunk");
        read_chunk(&mut file, chunk, usize::MAX / 2).unwrap()
    }

    /// 真实文件路径：用本机 FFmpeg 写出带 bext 的 BWF，再回写 iXML，核对音频数据逐字节不变。
    /// 默认忽略（依赖外部 FFmpeg）；需要时用 `cargo test real_ffmpeg_bwf -- --ignored` 单跑。
    #[test]
    #[ignore = "依赖本机 FFmpeg，需显式 --ignored 运行"]
    fn real_ffmpeg_bwf_roundtrip() {
        let ffmpeg = std::env::var("SW_FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg.exe".into());
        let temp = Temp::new();
        let path = temp.0.join("real-bwf.wav");
        let status = std::process::Command::new(&ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=1:sample_rate=48000", "-c:a", "pcm_s16le", "-write_bext", "1", "-metadata", "description=real bext", "-metadata", "originator=SW-TEST"])
            .arg(&path)
            .status()
            .expect("无法启动 FFmpeg");
        assert!(status.success(), "FFmpeg 生成 BWF 失败");

        let before = read(&path).unwrap();
        assert!(before.has_bext, "FFmpeg 应写出 bext chunk");
        let payload_before = data_payload(&path);

        let patch = MetadataPatch { note: Some("真实文件回写".into()), category: Some("TEST".into()), ucs: Some("TEST_Tone_001".into()), ..Default::default() };
        let report = write(&path, &patch).unwrap();
        assert!(report.ixml_written);
        assert_eq!(report.metadata.note, "真实文件回写");
        assert_eq!(report.metadata.category, "TEST");
        assert_eq!(data_payload(&path), payload_before, "回写不得改动音频数据");

        let again = read(&path).unwrap();
        assert!(again.has_bext && again.has_ixml, "bext 与 iXML 应并存");
    }
}
