//! 只读引用音频的本地目录。扫描不移动、删除或改写源文件。
use rodio::Source;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use sha2::{Digest,Sha256};
use std::{collections::BTreeMap, fs, io::Read, path::{Path, PathBuf}, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, time::{Duration, SystemTime, UNIX_EPOCH}};
use uuid::Uuid;

const SCHEMA_VERSION: i64 = 1;
#[path = "library_tools.rs"]
pub mod library_tools;
#[path = "source_ops.rs"]
pub mod source_ops;

#[cfg(test)]
#[path = "catalog_acceptance.rs"]
mod acceptance;

#[derive(Clone)]
pub struct CatalogState { inner: Arc<Inner> }
struct Inner { db_path: PathBuf, active: Mutex<Option<Active>>, _instance_lock: fs::File }
struct Active { id: String, cancel: Arc<AtomicBool> }

#[derive(Debug, Serialize)]
pub struct CatalogItem {
    pub id: String, pub path: String, pub name: String, pub format: String,
    pub size: u64, pub modified: String, pub rate: Option<u32>, pub channels: Option<u16>,
    pub duration: Option<f64>, pub status: String, pub error: Option<String>,
    pub favorite: bool, pub favorite_group: Option<String>, pub tags: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct CatalogPage { pub items: Vec<CatalogItem>, pub total: u64 }
#[derive(Debug, Serialize)]
pub struct FolderNode { pub path:String, pub parent:Option<String>, pub name:String, pub count:u64 }
#[derive(Debug, Serialize, Default)]
pub struct AvailabilityReport { pub checked:u64, pub available:u64, pub missing:u64, pub unavailable:u64, pub changed:u64 }
#[derive(Debug, Serialize, Clone)]
pub struct ImportJob {
    pub id: String, pub root: String, pub status: String, pub discovered: u64,
    pub processed: u64, pub added: u64, pub updated: u64, pub skipped: u64,
    pub failed: u64, pub error: Option<String>, pub library:String,
}
impl Default for ImportJob{fn default()->Self{Self{id:String::new(),root:String::new(),status:String::new(),discovered:0,processed:0,added:0,updated:0,skipped:0,failed:0,error:None,library:"sound".into()}}}

fn err(e: impl std::fmt::Display) -> String { e.to_string() }
fn now() -> String { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos().to_string() }
fn validate_library(library:&str)->Result<&str,String>{match library{"sound"|"music"=>Ok(library),_=>Err("素材库只能是sound（音效库）或music（音乐库）。".into())}}

fn connection(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(err)?;
    conn.busy_timeout(Duration::from_secs(5)).map_err(err)?;
    conn.pragma_update(None, "foreign_keys", "ON").map_err(err)?;
    conn.pragma_update(None, "synchronous", "NORMAL").map_err(err)?;
    Ok(conn)
}

impl CatalogState {
    pub fn database_path(&self)->PathBuf { self.inner.db_path.clone() }
    /// 0.1.21：元数据回写后同步刷新可检索文本；只重读容器头，不解码音频。
    pub fn refresh_metadata_index(&self,id:&str)->Result<(),String>{
        let path=self.resolve_asset(id)?;
        let format=audio_format(&path).unwrap_or_default();
        let text=metadata_search_text(&path,&format);
        let conn=connection(&self.inner.db_path)?;
        let changed=conn.execute("UPDATE assets SET meta_search=?2 WHERE id=?1",params![id,text]).map_err(err)?;
        if changed==0{return Err("素材不存在，元数据索引未更新。".into());}
        Ok(())
    }
    pub fn record_played(&self,id:&str)->Result<(),String>{library_tools::record_history(&self.inner.db_path,id)}
    pub fn new(db_path: PathBuf) -> Result<Self, String> {
        if let Some(parent) = db_path.parent() { fs::create_dir_all(parent).map_err(err)?; }
        // 跨进程持有锁，避免另一个实例把仍在运行的任务当成崩溃恢复。
        let lock_path=db_path.with_extension("instance.lock");
        let instance_lock=fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&lock_path).map_err(err)?;
        instance_lock.try_lock().map_err(|e|format!("目录数据库正在被另一实例使用，或无法获得独占锁：{e}。请关闭其他工作台后重试。"))?;
        let mut conn = connection(&db_path)?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).map_err(err)?;
        if version > SCHEMA_VERSION { return Err(format!("目录数据库版本 {version} 高于本程序支持版本 {SCHEMA_VERSION}，未修改数据库。")); }
        conn.pragma_update(None, "journal_mode", "WAL").map_err(err)?;
        if version == 0 {
            let tx = conn.transaction().map_err(err)?;
            tx.execute_batch("CREATE TABLE IF NOT EXISTS assets (
                id TEXT PRIMARY KEY, path_key TEXT NOT NULL UNIQUE, path TEXT NOT NULL,
                name TEXT NOT NULL, format TEXT NOT NULL, size INTEGER NOT NULL, modified TEXT NOT NULL,
                rate INTEGER, channels INTEGER, duration REAL, status TEXT NOT NULL, error TEXT);
                CREATE INDEX IF NOT EXISTS assets_name ON assets(name, id);
                CREATE TABLE IF NOT EXISTS jobs (
                id TEXT PRIMARY KEY, root TEXT NOT NULL, status TEXT NOT NULL,
                discovered INTEGER NOT NULL DEFAULT 0, processed INTEGER NOT NULL DEFAULT 0,
                added INTEGER NOT NULL DEFAULT 0, updated INTEGER NOT NULL DEFAULT 0,
                skipped INTEGER NOT NULL DEFAULT 0, failed INTEGER NOT NULL DEFAULT 0,
                error TEXT, created TEXT NOT NULL, changed TEXT NOT NULL);
                PRAGMA user_version=1;").map_err(err)?;
            tx.commit().map_err(err)?;
        }
        // schema1的兼容扩展，仅新增偏好表，不改写素材或既有任务。
        conn.execute_batch("CREATE TABLE IF NOT EXISTS import_history (path TEXT PRIMARY KEY, used_at TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS user_metadata (
                asset_id TEXT PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,
                favorite INTEGER NOT NULL DEFAULT 0 CHECK(favorite IN (0,1)),
                tags TEXT NOT NULL DEFAULT '[]', tags_search TEXT NOT NULL DEFAULT '');
            CREATE TABLE IF NOT EXISTS asset_fingerprints (
                asset_id TEXT PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,
                sha256 TEXT NOT NULL, size INTEGER NOT NULL, modified TEXT NOT NULL, prepared_at TEXT NOT NULL);").map_err(err)?;
        // 0.1.24：收藏分组。空串与 NULL 都表示未分组；取消收藏时一并清空。
        {
            let has_group=conn.prepare("PRAGMA table_info(user_metadata)").map_err(err)?
                .query_map([],|r|r.get::<_,String>(1)).map_err(err)?
                .collect::<Result<Vec<_>,_>>().map_err(err)?.iter().any(|name|name=="favorite_group");
            if !has_group{conn.execute_batch("ALTER TABLE user_metadata ADD COLUMN favorite_group TEXT;").map_err(err)?;}
        }
        // 0.1.21：内嵌元数据的可检索文本。NULL=尚未索引，空串=已索引但无内嵌字段；
        // 旧库升级后 NULL 记录会在下次重扫时补齐，不把「没扫过」误当成「没有元数据」。
        {
            let has_meta_search=conn.prepare("PRAGMA table_info(assets)").map_err(err)?
                .query_map([],|r|r.get::<_,String>(1)).map_err(err)?
                .collect::<Result<Vec<_>,_>>().map_err(err)?.iter().any(|name|name=="meta_search");
            if !has_meta_search{conn.execute_batch("ALTER TABLE assets ADD COLUMN meta_search TEXT;").map_err(err)?;}
        }
        {
            let tx=conn.transaction().map_err(err)?;
            tx.execute_batch("CREATE TABLE IF NOT EXISTS asset_libraries (
                asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
                library TEXT NOT NULL CHECK(library IN ('sound','music')),
                PRIMARY KEY(asset_id,library));
                CREATE INDEX IF NOT EXISTS asset_libraries_scope ON asset_libraries(library,asset_id);").map_err(err)?;
            let has_job_library=tx.prepare("PRAGMA table_info(jobs)").map_err(err)?.query_map([],|r|r.get::<_,String>(1)).map_err(err)?.collect::<Result<Vec<_>,_>>().map_err(err)?.iter().any(|name|name=="library");
            if !has_job_library{tx.execute_batch("ALTER TABLE jobs ADD COLUMN library TEXT NOT NULL DEFAULT 'sound' CHECK(library IN ('sound','music'));").map_err(err)?;}
            let migrated:Option<String>=tx.query_row("SELECT value FROM meta WHERE key='asset_libraries_revision'",[],|r|r.get(0)).optional().map_err(err)?;
            if migrated.as_deref()!=Some("1"){
                // 仅第一次升级把既有素材归入音效库；不按名称猜测类别，不污染以后单独导入的音乐库。
                tx.execute("INSERT OR IGNORE INTO asset_libraries(asset_id,library) SELECT id,'sound' FROM assets",[]).map_err(err)?;
                tx.execute("INSERT INTO meta(key,value) VALUES('asset_libraries_revision','1') ON CONFLICT(key) DO UPDATE SET value=excluded.value",[]).map_err(err)?;
            }
            // 旧安装版仍可能写入schema1：仅修复完全没有归属的记录，已有music-only归属不补sound。
            tx.execute("INSERT OR IGNORE INTO asset_libraries(asset_id,library)
                SELECT a.id,'sound' FROM assets a WHERE NOT EXISTS(SELECT 1 FROM asset_libraries l WHERE l.asset_id=a.id)",[]).map_err(err)?;
            tx.commit().map_err(err)?;
        }
        library_tools::initialize(&conn)?;
        // 一次性修复：旧版把时长按错误公式写入，清空后由下次扫描按文件头重算；不动素材、任务与用户标注。
        let revision: Option<String> = conn.query_row("SELECT value FROM meta WHERE key='duration_revision'", [], |r| r.get(0)).optional().map_err(err)?;
        if revision.as_deref() != Some("2") {
            let tx = conn.transaction().map_err(err)?;
            tx.execute("UPDATE assets SET duration=NULL WHERE duration IS NOT NULL", []).map_err(err)?;
            tx.execute("INSERT INTO meta(key,value) VALUES('duration_revision','2') ON CONFLICT(key) DO UPDATE SET value=excluded.value", []).map_err(err)?;
            tx.commit().map_err(err)?;
        }
        conn.execute("UPDATE jobs SET status='interrupted',error='上次进程在任务完成前退出，可恢复扫描。',changed=?1 WHERE status IN ('running','cancelling')", [now()]).map_err(err)?;
        Ok(Self { inner: Arc::new(Inner { db_path, active: Mutex::new(None), _instance_lock: instance_lock }) })
    }

    #[cfg(test)]
    fn start(&self, root: String, resume: Option<String>) -> Result<String,String> {
        self.start_in_library(root,resume,"sound")
    }
    fn start_in_library(&self, root: String, resume: Option<String>, library:&str) -> Result<String,String> {
        let library=validate_library(library)?.to_owned();
        let root = preview_root(&root)?;
        let mut active = self.inner.active.lock().map_err(|_| "任务锁不可用".to_string())?;
        if active.is_some() { return Err("已有导入任务运行，请等待完成或取消后重试。".into()); }
        let conn = connection(&self.inner.db_path)?;
        let id = resume.unwrap_or_else(|| Uuid::new_v4().to_string());
        let job = ImportJob { id:id.clone(), root:root.clone(), status:"running".into(),library:library.clone(), ..Default::default() };
        conn.execute("INSERT INTO jobs(id,root,status,created,changed,library) VALUES(?1,?2,'running',?3,?3,?4)
            ON CONFLICT(id) DO UPDATE SET status='running',discovered=0,processed=0,added=0,updated=0,skipped=0,failed=0,error=NULL,changed=excluded.changed",
            params![id,root,now(),library]).map_err(err)?;
        let cancel = Arc::new(AtomicBool::new(false));
        *active = Some(Active { id:id.clone(),cancel:cancel.clone() });
        let inner = self.inner.clone();
        let thread_id = id.clone();
        let spawn = std::thread::Builder::new().name("catalog-import".into()).spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scan(&inner.db_path,job,&cancel)));
            let failure = match result { Ok(Ok(())) => None, Ok(Err(e)) => Some(e), Err(_) => Some("导入线程异常退出，已有目录记录保留，可恢复。".into()) };
            if let Some(message) = failure {
                if let Ok(conn) = connection(&inner.db_path) { let _ = conn.execute("UPDATE jobs SET status='failed',error=?1,changed=?2 WHERE id=?3",params![message,now(),thread_id]); }
            }
            if let Ok(mut guard) = inner.active.lock() { *guard = None; }
        });
        if let Err(e) = spawn {
            *active = None;
            conn.execute("UPDATE jobs SET status='failed',error=?1,changed=?2 WHERE id=?3",params![e.to_string(),now(),id]).map_err(err)?;
            return Err(format!("无法启动导入线程：{e}"));
        }
        Ok(id)
    }

    fn cancel(&self,id:&str) -> Result<(),String> {
        let active = self.inner.active.lock().map_err(|_| "任务锁不可用".to_string())?;
        let running = active.as_ref().filter(|a| a.id == id).ok_or("该任务当前未运行。")?;
        // 先持久化请求，再发送取消信号，防止终态被 cancelling 覆盖。
        let conn = connection(&self.inner.db_path)?;
        conn.execute("UPDATE jobs SET status='cancelling',changed=?1 WHERE id=?2 AND status='running'",params![now(),id]).map_err(err)?;
        running.cancel.store(true,Ordering::SeqCst);
        Ok(())
    }

    fn resume(&self,id:String)->Result<String,String>{
        let conn=connection(&self.inner.db_path)?;
        let (root,status,library):(String,String,String)=conn.query_row("SELECT root,status,library FROM jobs WHERE id=?1",[&id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|_|"找不到该任务".to_string())?;
        if !["cancelled","interrupted","failed","completed_with_errors"].contains(&status.as_str()){return Err("仅可恢复取消、中断、失败或有错误的任务。".into());}
        self.start_in_library(root,Some(id),&library)
    }

    // 维护操作与导入互斥；大文件指纹在阻塞线程中流式读取，不占用UI线程。
    fn with_idle<T>(&self,operation:impl FnOnce(&Path)->Result<T,String>)->Result<T,String>{
        let active=self.inner.active.lock().map_err(|_|"任务锁不可用".to_string())?;
        if active.is_some(){return Err("导入任务正在运行，请等待完成或取消后再检查/重新定位。".into());}
        operation(&self.inner.db_path)
    }

    /// 播放只接受目录中的稳定ID，不允许前端直接提供任意磁盘路径。
    pub fn resolve_asset(&self,id:&str)->Result<PathBuf,String>{
        let conn=connection(&self.inner.db_path)?;
        let found:Option<(String,String)>=conn.query_row("SELECT path,status FROM assets WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(err)?;
        let (stored,status)=found.ok_or("素材不存在，请刷新目录。")?;
        if status!="ready"{return Err("素材当前不可播放，请检查是否缺失、离线或已变化，再重新扫描/定位。".into());}
        let path=PathBuf::from(stored);
        if !path.is_absolute()||audio_format(&path).is_none(){return Err("目录记录不是受支持的音频绝对路径。".into());}
        let metadata=fs::symlink_metadata(&path).map_err(|e|format!("素材不可访问：{e}"))?;
        if !metadata.is_file()||is_reparse(&metadata){return Err("素材已变成非普通文件或链接，请重新扫描。".into());}
        let canonical=fs::canonicalize(&path).map_err(err)?;
        if canonical!=path{return Err("素材路径发生重定向，请重新扫描。".into());}
        Ok(canonical)
    }
}

fn is_reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)] { use std::os::windows::fs::MetadataExt; metadata.file_attributes() & 0x400 != 0 }
    #[cfg(not(windows))] { metadata.file_type().is_symlink() }
}
fn path_string(path:&Path) -> Result<String,String> {
    path.to_str().map(str::to_owned).ok_or_else(|| "路径包含不可表示为Unicode的字符，已跳过。".into())
}
fn preview_root(root:&str) -> Result<String,String> {
    let input=Path::new(root.trim().trim_matches('"'));
    if !input.is_absolute() { return Err("请输入绝对目录路径。".into()); }
    let metadata=fs::symlink_metadata(input).map_err(|e|format!("无法读取目录：{e}"))?;
    if is_reparse(&metadata) || !metadata.is_dir() { return Err("请选择真实目录；不跟随符号链接或目录联接。".into()); }
    let path=fs::canonicalize(input).map_err(err)?;
    // 只打开目录句柄验证可读，不进行递归枚举。
    let _ = fs::read_dir(&path).map_err(|e|format!("目录不可读：{e}"))?;
    path_string(&path)
}
fn audio_format(path:&Path) -> Option<String> {
    let ext=path.extension()?.to_str()?.to_ascii_uppercase();
    match ext.as_str() { "WAV"|"FLAC"|"MP3"|"M4A" => Some(ext), "AIFF"|"AIF" => Some("AIFF".into()), _=>None }
}
/// 0.1.21：把 WAV/BWF 内嵌元数据压成一行可检索文本。
/// 非 RIFF/WAVE 与读取失败一律返回空串，不阻断导入；只取已知字段，不解析未知厂商块。
const META_SEARCH_LIMIT: usize = 4096;
fn metadata_search_text(path:&Path,format:&str)->String{
    if format!="WAV"{return String::new();}
    let meta=match crate::bwf_metadata::read(path){Ok(value)=>value,Err(_)=>return String::new()};
    let fields=[meta.description,meta.originator,meta.originator_reference,meta.origination_date,meta.origination_time,
        meta.project,meta.scene,meta.take,meta.tape,meta.note,meta.category,meta.keywords,meta.license,meta.creator,meta.source,meta.ucs];
    let mut text=String::new();
    for field in fields{
        let clean:String=field.chars().filter(|ch|!ch.is_control()).collect();
        let clean=clean.trim();
        if clean.is_empty(){continue;}
        if !text.is_empty(){text.push(' ');}
        text.push_str(clean);
        if text.chars().count()>=META_SEARCH_LIMIT{break;}
    }
    text.chars().take(META_SEARCH_LIMIT).collect()
}
fn metadata_audio(path:&Path) -> Result<(u32,u16,Option<f64>),String> {
    // 与播放共用同一解码入口：rodio 0.20.1 的 symphonia 适配器在 M4A 初始化时会触发
    // unreachable panic，且其 total_duration 换算有缺陷（见 duration.rs）。
    let source=crate::player::PlaybackSource::open(path).map_err(|e|format!("无法识别音频：{e}"))?;
    let rate=source.sample_rate(); let channels=source.channels();
    if rate == 0 || channels == 0 { return Err("音频头返回无效采样率或声道数。".into()); }
    Ok((rate,channels,crate::duration::seconds(path)))
}
fn save_progress(conn:&Connection,job:&ImportJob) -> Result<(),String> {
    conn.execute("UPDATE jobs SET status=CASE WHEN status='cancelling' AND ?1='running' THEN status ELSE ?1 END,
        discovered=?2,processed=?3,added=?4,updated=?5,skipped=?6,failed=?7,error=?8,changed=?9 WHERE id=?10",
        params![job.status,job.discovered,job.processed,job.added,job.updated,job.skipped,job.failed,job.error,now(),job.id]).map_err(err)?;
    Ok(())
}
fn record_file(conn:&Connection,path:&Path,format:&str,job:&mut ImportJob) -> Result<(),String> {
    let canonical=fs::canonicalize(path).map_err(err)?;
    let path_text=path_string(&canonical)?;
    // canonicalize 统一普通Windows路径的实际拼写；不主动小写，避免大小写敏感目录被合并。
    let key=path_text.clone();
    let metadata=fs::metadata(&canonical).map_err(err)?;
    let size=metadata.len();
    let modified=metadata.modified().map_err(err)?.duration_since(UNIX_EPOCH).map_err(err)?.as_nanos().to_string();
    let old:Option<(String,u64,String,String,Option<f64>,Option<String>)>=conn.query_row("SELECT id,size,modified,status,duration,meta_search FROM assets WHERE path_key=?1",[&key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional().map_err(err)?;
    // 文件未变且已有可用时长才跳过；时长为空说明需要按文件头补算（含旧版错误值被清空后的重扫）。
    if old.as_ref().is_some_and(|(_,bytes,mtime,status,duration,_)|*bytes==size && mtime==&modified && status=="ready" && duration.is_some()) {
        let (id,_,_,_,_,indexed)=old.as_ref().unwrap();
        // 旧库升级后 meta_search 为 NULL：只补内嵌元数据索引，不重新解码音频。
        if indexed.is_none(){
            let text=metadata_search_text(&canonical,format);
            conn.execute("UPDATE assets SET meta_search=?2 WHERE id=?1",params![id.as_str(),text]).map_err(err)?;
        }
        conn.execute("INSERT OR IGNORE INTO asset_libraries(asset_id,library) VALUES(?1,?2)",params![id.as_str(),validate_library(&job.library)?]).map_err(err)?;
        job.skipped+=1;return Ok(());
    }
    let (rate,channels,duration,status,error)=match metadata_audio(&canonical) {
        Ok((r,c,d))=>(Some(r),Some(c),d,"ready",None),
        Err(e)=>(None,None,None,"error",Some(e))
    };
    // 不将分析期间发生变化的元数据写成就绪；下次恢复可重试。
    let after=fs::metadata(&canonical).map_err(err)?;
    if after.len()!=size || after.modified().map_err(err)?!=metadata.modified().map_err(err)? { return Err("分析期间文件发生变化，请重新扫描。".into()); }
    let meta_text=metadata_search_text(&canonical,format);
    let id=old.as_ref().map(|a|a.0.clone()).unwrap_or_else(||Uuid::new_v4().to_string());
    let name=canonical.file_name().and_then(|n|n.to_str()).ok_or("文件名无法表示为Unicode")?;
    let library=validate_library(&job.library)?;
    // 素材更新、归属登记与过期指纹失效一起提交，进程中断不留下无归属的新素材。
    let tx=conn.unchecked_transaction().map_err(err)?;
    tx.execute("INSERT INTO assets(id,path_key,path,name,format,size,modified,rate,channels,duration,status,error,meta_search)
        VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
        ON CONFLICT(path_key) DO UPDATE SET path=excluded.path,name=excluded.name,format=excluded.format,
        size=excluded.size,modified=excluded.modified,rate=excluded.rate,channels=excluded.channels,
        duration=excluded.duration,status=excluded.status,error=excluded.error,meta_search=excluded.meta_search",
        params![id,key,path_text,name,format,size,modified,rate,channels,duration,status,error,meta_text]).map_err(err)?;
    tx.execute("INSERT OR IGNORE INTO asset_libraries(asset_id,library) VALUES(?1,?2)",params![id,library]).map_err(err)?;
    if old.as_ref().is_some_and(|(_,bytes,mtime,_,_,_)|*bytes!=size||mtime!=&modified){
        tx.execute("DELETE FROM asset_fingerprints WHERE asset_id=?1",[&id]).map_err(err)?;
    }
    tx.commit().map_err(err)?;
    if old.is_some(){job.updated+=1;}else{job.added+=1;}
    if status=="error"{job.failed+=1;}
    Ok(())
}
fn scan(db:&Path,mut job:ImportJob,cancel:&AtomicBool)->Result<(),String> {
    let conn=connection(db)?;
    let mut directories=vec![PathBuf::from(&job.root)];
    while let Some(directory)=directories.pop() {
        if cancel.load(Ordering::SeqCst){break;}
        let entries=match fs::read_dir(&directory){Ok(e)=>e,Err(e)=>{job.failed+=1;job.error=Some(format!("{}：{e}",directory.display()));save_progress(&conn,&job)?;continue;}};
        for entry in entries {
            if cancel.load(Ordering::SeqCst){break;}
            let entry=match entry{Ok(e)=>e,Err(e)=>{job.failed+=1;job.error=Some(format!("目录条目读取失败：{e}"));continue;}};
            let path=entry.path();
            let metadata=match fs::symlink_metadata(&path){Ok(m)=>m,Err(e)=>{job.failed+=1;job.error=Some(format!("{}：{e}",path.display()));continue;}};
            if is_reparse(&metadata){continue;}
            if metadata.is_dir(){directories.push(path);continue;}
            if !metadata.is_file(){continue;}
            if let Some(format)=audio_format(&path) {
                job.discovered+=1;
                if let Err(e)=record_file(&conn,&path,&format,&mut job){job.failed+=1;job.error=Some(format!("{}：{e}",path.display()));}
                job.processed+=1;
                if job.processed % 32 == 0 {save_progress(&conn,&job)?;}
            }
        }
    }
    job.status=if cancel.load(Ordering::SeqCst){"cancelled"}else if job.failed>0{"completed_with_errors"}else{"completed"}.into();
    save_progress(&conn,&job)
}

fn remember_path(db:&Path,root:&str)->Result<(),String>{
    let mut conn=connection(db)?;
    let tx=conn.transaction().map_err(err)?;
    tx.execute("INSERT INTO import_history(path,used_at) VALUES(?1,?2) ON CONFLICT(path) DO UPDATE SET used_at=excluded.used_at",params![root,now()]).map_err(err)?;
    tx.execute("DELETE FROM import_history WHERE path NOT IN (SELECT path FROM import_history ORDER BY used_at DESC,path LIMIT 12)",[]).map_err(err)?;
    tx.commit().map_err(err)
}
fn history(db:&Path)->Result<Vec<String>,String>{
    let conn=connection(db)?;
    let mut stmt=conn.prepare("SELECT path FROM import_history ORDER BY used_at DESC,path LIMIT 12").map_err(err)?;
    let result=stmt.query_map([],|r|r.get(0)).map_err(err)?.collect::<Result<Vec<String>,_>>().map_err(err)?;
    Ok(result)
}
fn preview_and_remember(db:&Path,root:&str)->Result<String,String>{
    let canonical=preview_root(root)?;
    remember_path(db,&canonical)?;
    Ok(canonical)
}
fn folder_prefix(folder:Option<&str>)->Result<Option<String>,String>{
    let Some(folder)=folder.filter(|s|!s.is_empty()) else{return Ok(None)};
    let path=Path::new(folder);
    if !path.is_absolute(){return Err("目录范围必须为完整绝对路径。".into())}
    // 来自目录树的规范路径可以离线使用，不再次访问磁盘；分隔边界防止a匹配ab。
    let mut prefix=path_string(path)?;
    if !prefix.ends_with(std::path::MAIN_SEPARATOR){prefix.push(std::path::MAIN_SEPARATOR);}
    Ok(Some(prefix))
}
#[cfg(test)]
fn folder_nodes(db:&Path)->Result<Vec<FolderNode>,String>{folder_nodes_in_library(db,None)}
fn folder_nodes_in_library(db:&Path,library:Option<&str>)->Result<Vec<FolderNode>,String>{
    let library=library.map(validate_library).transpose()?;
    let conn=connection(db)?;
    let mut stmt=conn.prepare("SELECT a.path FROM assets a WHERE ?1 IS NULL OR EXISTS(SELECT 1 FROM asset_libraries l WHERE l.asset_id=a.id AND l.library=?1)").map_err(err)?;
    let rows=stmt.query_map([library],|r|r.get::<_,String>(0)).map_err(err)?;
    let mut nodes:BTreeMap<String,FolderNode>=BTreeMap::new();
    let mut add_folder=|directory:&Path,count:u64|->Result<(),String>{
        for folder in directory.ancestors().take_while(|p|p.is_absolute()){
            let path=path_string(folder)?;
            let parent=folder.parent().filter(|p|p.is_absolute()).map(path_string).transpose()?;
            let name=folder.file_name().and_then(|n|n.to_str()).map(str::to_owned).unwrap_or_else(||{
                if let Some(unc)=path.strip_prefix(r"\\?\UNC\"){format!(r"\\{unc}")}
                else{path.strip_prefix(r"\\?\").unwrap_or(&path).to_owned()}
            });
            let node=nodes.entry(path.clone()).or_insert(FolderNode{path,parent,name,count:0});
            node.count+=count;
        }
        Ok(())
    };
    for row in rows{let file=row.map_err(err)?;if let Some(parent)=Path::new(&file).parent(){add_folder(parent,1)?;}}
    let mut explicit=conn.prepare("SELECT path FROM catalog_directories WHERE ?1 IS NULL OR library=?1 ORDER BY path").map_err(err)?;
    for path in explicit.query_map([library],|r|r.get::<_,String>(0)).map_err(err)?{add_folder(Path::new(&path.map_err(err)?),0)?;}
    Ok(nodes.into_values().collect())
}
#[derive(Default)]
struct SearchFilter<'a>{folder:Option<&'a str>,query:Option<&'a str>,format:Option<&'a str>,sort:Option<&'a str>,favorites_only:bool,favorite_group:Option<&'a str>,favorite_ungrouped:bool,library:Option<&'a str>,tag:Option<&'a str>,tags:Vec<String>,history_only:bool,playlist_id:Option<&'a str>,min_duration:Option<f64>,max_duration:Option<f64>,multilingual:bool,metadata:bool,ready_only:bool}
fn search_page(db:&Path,offset:usize,limit:usize,filter:SearchFilter<'_>)->Result<CatalogPage,String>{
    use rusqlite::types::Value;
    let query=filter.query.unwrap_or("");
    if query.chars().count()>1024{return Err("搜索内容不能超过1024个字符。".into());}
    let words:Vec<_>=query.split_whitespace().collect();
    if words.len()>32{return Err("搜索词不能超过32个。".into());}
    let format=filter.format.filter(|s|!s.is_empty()&&*s!="全部格式").map(str::to_ascii_uppercase);
    if format.as_deref().is_some_and(|s|!["WAV","AIFF","FLAC","MP3","M4A"].contains(&s)){return Err("不支持的格式筛选。".into());}
    // 排序表达式只来自白名单；用户输入均用绑定参数。
    let order=match filter.sort.unwrap_or("name"){
        "name"=>"a.name COLLATE NOCASE,a.id",
        "name_desc"=>"a.name COLLATE NOCASE DESC,a.id DESC",
        "duration"=>"a.duration IS NULL,a.duration,a.id",
        "duration_desc"=>"a.duration IS NULL,a.duration DESC,a.id DESC",
        "rate"=>"a.rate IS NULL,a.rate,a.id",
        "rate_desc"=>"a.rate IS NULL,a.rate DESC,a.id DESC",
        _=>return Err("不支持的排序方式。".into())
    };
    let order=if filter.history_only{"(SELECT h.played_at FROM play_history h WHERE h.asset_id=a.id) DESC,a.id"}else{order};
    let mut conditions=vec!["1=1".to_string()];let mut values:Vec<Value>=Vec::new();
    // 隐藏不可用素材：与关键词、格式、收藏等条件同批进入 SQL，COUNT 与分页共用同一 predicate。
    if filter.ready_only{conditions.push("a.status='ready'".into());}
    if let Some(library)=filter.library{values.push(Value::Text(validate_library(library)?.to_owned()));conditions.push(format!("EXISTS(SELECT 1 FROM asset_libraries l WHERE l.asset_id=a.id AND l.library=?{})",values.len()));}
    if let Some(prefix)=folder_prefix(filter.folder)?{
        values.push(Value::Text(prefix));let n=values.len();conditions.push(format!("substr(a.path,1,length(?{n}))=?{n}"));
    }
    if let Some(format)=format{values.push(Value::Text(format));conditions.push(format!("a.format=?{}",values.len()));}
    if filter.favorites_only{conditions.push("COALESCE(m.favorite,0)=1".into());}
    // 收藏分组：分组筛选只作用于已收藏素材；未分组按空串/NULL 判定。
    if let Some(group)=filter.favorite_group.filter(|s|!s.is_empty()){
        if group.chars().count()>40 || group.chars().any(char::is_control){return Err("收藏分组筛选内容无效。".into());}
        values.push(Value::Text(group.to_owned()));
        conditions.push(format!("(COALESCE(m.favorite,0)=1 AND m.favorite_group=?{})",values.len()));
    }
    if filter.favorite_ungrouped{conditions.push("(COALESCE(m.favorite,0)=1 AND COALESCE(m.favorite_group,'')='')".into());}
    // 时长区间：素材库里时长未知的记录一律不计入区间结果，避免"未知"混进 0 秒。
    if let Some(min)=filter.min_duration.filter(|value|value.is_finite()&&*value>0.0){
        values.push(Value::Real(min));conditions.push(format!("a.duration IS NOT NULL AND a.duration>=?{}",values.len()));
    }
    if let Some(max)=filter.max_duration.filter(|value|value.is_finite()&&*value>0.0){
        values.push(Value::Real(max));conditions.push(format!("a.duration IS NOT NULL AND a.duration<=?{}",values.len()));
    }
    if let Some(tag)=filter.tag.filter(|s|!s.is_empty()){
        if tag.chars().count()>40 || tag.chars().any(char::is_control){return Err("标签筛选内容无效。".into());}
        values.push(Value::Text(tag.to_owned()));conditions.push(format!("EXISTS(SELECT 1 FROM json_each(COALESCE(m.tags,'[]')) t WHERE t.value=?{})",values.len()));
    }
    // 维度条可同时勾选多个标签，彼此取交集；上限 8 个防止拼出过长 SQL。
    if filter.tags.len()>8{return Err("标签筛选最多 8 项。".into());}
    for tag in &filter.tags{
        if tag.is_empty()||tag.chars().count()>40||tag.chars().any(char::is_control){return Err("标签筛选内容无效。".into());}
        values.push(Value::Text(tag.clone()));conditions.push(format!("EXISTS(SELECT 1 FROM json_each(COALESCE(m.tags,'[]')) t WHERE t.value=?{})",values.len()));
    }
    if filter.history_only{conditions.push("EXISTS(SELECT 1 FROM play_history h WHERE h.asset_id=a.id)".into());}
    if let Some(playlist)=filter.playlist_id{
        if playlist.is_empty()||playlist.len()>128{return Err("播放列表ID无效。".into());}
        values.push(Value::Text(playlist.to_owned()));let n=values.len();
        conditions.push(format!("EXISTS(SELECT 1 FROM playlist_items pi JOIN playlists p ON p.id=pi.playlist_id WHERE pi.asset_id=a.id AND p.id=?{n}{} )",if let Some(library)=filter.library{format!(" AND p.library='{}'",validate_library(library)?)}else{String::new()}));
    }
    // 跨语言扩展只作用于普通文本关键词：同一词的候选之间 OR，词与词之间仍是 AND。
    // 上面的标签/分类/格式/时长/收藏/播放列表等条件各自独立生成，完全不受扩展影响；
    // 所有候选词依旧走绑定参数，不拼进 SQL 文本。
    // 「含元数据」开启才把路径、标签与内嵌元数据并入可检索文本；关闭时只匹配文件名。
    // 拼接的只有固定列名，用户输入仍全部走绑定参数，标签/分类/格式/时长/收藏/播放列表条件也不受影响。
    let searchable=if filter.metadata{
        "a.name || ' ' || a.path || ' ' || COALESCE(m.tags_search,'') || ' ' || COALESCE(a.meta_search,'')"
    }else{
        "a.name"
    };
    let expanded=if filter.multilingual{crate::multilingual::expand_query(&words)}else{words.iter().map(|word|vec![word.to_ascii_lowercase()]).collect::<Vec<Vec<String>>>()};
    for variants in expanded{
        let mut alternatives=Vec::with_capacity(variants.len());
        for variant in variants{
            // SQLite内建lower覆盖ASCII拉丁字母，中文保持原字；instr按字面匹配%/_及引号。
            values.push(Value::Text(variant));
            alternatives.push(format!("instr(lower({searchable}),?{})>0",values.len()));
        }
        conditions.push(format!("({})",alternatives.join(" OR ")));
    }
    let predicate=conditions.join(" AND ");
    let from=" FROM assets a LEFT JOIN user_metadata m ON m.asset_id=a.id ";
    let mut conn=connection(db)?;let tx=conn.transaction().map_err(err)?;
    let total=tx.query_row(&format!("SELECT COUNT(*){from}WHERE {predicate}"),rusqlite::params_from_iter(values.iter()),|r|r.get(0)).map_err(err)?;
    values.push(Value::Integer(limit.clamp(1,500) as i64));let limit_parameter=values.len();
    values.push(Value::Integer(offset.min(i64::MAX as usize) as i64));let offset_parameter=values.len();
    let sql=format!("SELECT a.id,a.path,a.name,a.format,a.size,a.modified,a.rate,a.channels,a.duration,a.status,a.error,COALESCE(m.favorite,0),COALESCE(m.tags,'[]'),COALESCE(m.favorite_group,''){from}WHERE {predicate} ORDER BY {order} LIMIT ?{limit_parameter} OFFSET ?{offset_parameter}");
    let mut stmt=tx.prepare(&sql).map_err(err)?;
    let items=stmt.query_map(rusqlite::params_from_iter(values.iter()),|r|{
        let raw:String=r.get(12)?;
        let tags=serde_json::from_str(&raw).map_err(|e|rusqlite::Error::FromSqlConversionFailure(12,rusqlite::types::Type::Text,Box::new(e)))?;
        let group:String=r.get(13)?;
        Ok(CatalogItem{id:r.get(0)?,path:r.get(1)?,name:r.get(2)?,format:r.get(3)?,size:r.get(4)?,modified:r.get(5)?,rate:r.get(6)?,channels:r.get(7)?,duration:r.get(8)?,status:r.get(9)?,error:r.get(10)?,favorite:r.get(11)?,favorite_group:if group.is_empty(){None}else{Some(group)},tags})
    }).map_err(err)?.collect::<Result<Vec<_>,_>>().map_err(err)?;
    drop(stmt);tx.commit().map_err(err)?;
    Ok(CatalogPage{items,total})
}
#[cfg(test)]
fn list_page_scoped(db:&Path,offset:usize,limit:usize,folder:Option<&str>)->Result<CatalogPage,String>{search_page(db,offset,limit,SearchFilter{folder,..Default::default()})}
#[cfg(test)]
fn list_page(db:&Path,offset:usize,limit:usize)->Result<CatalogPage,String>{list_page_scoped(db,offset,limit,None)}
fn set_favorite(db:&Path,id:&str,favorite:bool)->Result<(),String>{
    let conn=connection(db)?;
    // 取消收藏同时清空分组，避免下次重新收藏时悄悄落回旧分组。
    let changed=conn.execute("INSERT INTO user_metadata(asset_id,favorite,favorite_group) SELECT id,?2,NULL FROM assets WHERE id=?1
        ON CONFLICT(asset_id) DO UPDATE SET favorite=excluded.favorite,favorite_group=CASE WHEN excluded.favorite=0 THEN NULL ELSE favorite_group END",params![id,favorite]).map_err(err)?;
    if changed==0{return Err("素材不存在，收藏未保存。".into());}Ok(())
}
/// 收藏分组：设置分组即视为收藏；传 None 只清空分组、保留收藏状态。
fn set_favorite_group(db:&Path,ids:Vec<String>,group:Option<&str>)->Result<u64,String>{
    if ids.is_empty()||ids.len()>500||ids.iter().any(|id|id.is_empty()||id.len()>128){return Err("请选择1至500个有效素材ID。".into());}
    let group=match group.map(str::trim).filter(|value|!value.is_empty()){
        None=>None,
        Some(value)=>{
            if value.chars().count()>40||value.chars().any(char::is_control){return Err("收藏分组须为1至40个可见字符。".into());}
            Some(value)
        }
    };
    let mut conn=connection(db)?;let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(err)?;
    let mut changed=0u64;
    for id in ids.into_iter().collect::<std::collections::BTreeSet<_>>(){
        changed+=match group{
            Some(name)=>tx.execute("INSERT INTO user_metadata(asset_id,favorite,favorite_group) SELECT id,1,?2 FROM assets WHERE id=?1
                ON CONFLICT(asset_id) DO UPDATE SET favorite=1,favorite_group=excluded.favorite_group",params![id,name]).map_err(err)?,
            None=>tx.execute("INSERT INTO user_metadata(asset_id,favorite) SELECT id,0 FROM assets WHERE id=?1
                ON CONFLICT(asset_id) DO UPDATE SET favorite_group=NULL",[&id]).map_err(err)?,
        } as u64;
    }
    if changed==0{return Err("素材不存在，收藏分组未保存。".into());}
    tx.commit().map_err(err)?;Ok(changed)
}
fn set_tags(db:&Path,id:&str,tags:Vec<String>)->Result<(),String>{
    if tags.len()>20{return Err("每条素材最多20个标签。".into());}
    let mut clean=Vec::new();
    for tag in tags{
        let tag=tag.trim();if tag.is_empty(){continue;}
        if tag.chars().count()>40{return Err("单个标签不能超过40个字符。".into());}
        if tag.chars().any(char::is_control){return Err("标签不能包含换行或控制字符。".into());}
        if !clean.iter().any(|existing:&String|existing==tag){clean.push(tag.to_owned());}
    }
    let json=serde_json::to_string(&clean).map_err(err)?;let text=clean.join(" ");
    let conn=connection(db)?;
    let changed=conn.execute("INSERT INTO user_metadata(asset_id,tags,tags_search) SELECT id,?2,?3 FROM assets WHERE id=?1
        ON CONFLICT(asset_id) DO UPDATE SET tags=excluded.tags,tags_search=excluded.tags_search",params![id,json,text]).map_err(err)?;
    if changed==0{return Err("素材不存在，标签未保存。".into());}Ok(())
}
fn assign_library(db:&Path,ids:Vec<String>,library:&str)->Result<u64,String>{
    let library=validate_library(library)?;
    if ids.is_empty()||ids.len()>500||ids.iter().any(|id|id.is_empty()||id.len()>128){return Err("请选择1至500个有效素材ID。".into());}
    let unique:std::collections::BTreeSet<_>=ids.into_iter().collect();
    let mut conn=connection(db)?;let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(err)?;
    for id in &unique{let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM assets WHERE id=?1)",[id],|r|r.get(0)).map_err(err)?;if !exists{return Err(format!("素材不存在，整批归属未修改：{id}"));}}
    let mut added=0u64;
    for id in unique{added+=tx.execute("INSERT OR IGNORE INTO asset_libraries(asset_id,library) VALUES(?1,?2)",params![id,library]).map_err(err)? as u64;}
    tx.commit().map_err(err)?;Ok(added)
}
fn jobs(db:&Path)->Result<Vec<ImportJob>,String> {
    let conn=connection(db)?;
    let mut stmt=conn.prepare("SELECT id,root,status,discovered,processed,added,updated,skipped,failed,error,library FROM jobs ORDER BY changed DESC LIMIT 50").map_err(err)?;
    let result=stmt.query_map([],|r|Ok(ImportJob{id:r.get(0)?,root:r.get(1)?,status:r.get(2)?,discovered:r.get(3)?,processed:r.get(4)?,added:r.get(5)?,updated:r.get(6)?,skipped:r.get(7)?,failed:r.get(8)?,error:r.get(9)?,library:r.get(10)?})).map_err(err)?.collect::<Result<Vec<_>,_>>().map_err(err)?;
    Ok(result)
}

fn metadata_stamp(metadata:&fs::Metadata)->Result<String,String>{
    Ok(metadata.modified().map_err(err)?.duration_since(UNIX_EPOCH).map_err(err)?.as_nanos().to_string())
}
fn ordinary_audio_path(raw:&str)->Result<PathBuf,String>{
    let path=Path::new(raw.trim().trim_matches('"'));
    if !path.is_absolute()||audio_format(path).is_none(){return Err("请选择WAV/AIFF/FLAC/MP3/M4A音频的完整绝对路径。".into());}
    let metadata=fs::symlink_metadata(path).map_err(|e|format!("文件不可访问：{e}"))?;
    if !metadata.is_file()||is_reparse(&metadata){return Err("只接受普通音频文件，不跟随文件链接。".into());}
    fs::canonicalize(path).map_err(err)
}
fn file_fingerprint(path:&Path)->Result<(String,u64,String),String>{
    let mut options=fs::OpenOptions::new();options.read(true);
    #[cfg(windows)] {use std::os::windows::fs::OpenOptionsExt;options.share_mode(1);}
    let mut file=options.open(path).map_err(|e|format!("读取指纹失败，文件可能正在写入：{e}"))?;
    let before=file.metadata().map_err(err)?;let modified=metadata_stamp(&before)?;
    let mut digest=Sha256::new();let mut buffer=vec![0u8;1024*1024];let mut count=0u64;
    loop{let n=file.read(&mut buffer).map_err(err)?;if n==0{break;}digest.update(&buffer[..n]);count+=n as u64;}
    let after=file.metadata().map_err(err)?;
    if count!=before.len()||after.len()!=before.len()||metadata_stamp(&after)?!=modified{return Err("文件在计算指纹期间发生变化，请稍后重试。".into());}
    Ok((format!("{:x}",digest.finalize()),count,modified))
}
fn prepare_fingerprint(db:&Path,id:&str)->Result<String,String>{
    let conn=connection(db)?;
    let found:Option<(String,u64,String,String)>=conn.query_row("SELECT path,size,modified,status FROM assets WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(err)?;
    let (stored,size,modified,status)=found.ok_or("素材不存在。")?;
    if status!="ready"{return Err("请先恢复源文件并重新扫描；仅可为就绪素材登记内容指纹。".into());}
    let path=ordinary_audio_path(&stored)?;
    if path!=PathBuf::from(&stored){return Err("源路径发生重定向，请重新扫描。".into());}
    let (digest,actual_size,actual_modified)=file_fingerprint(&path)?;
    if actual_size!=size||actual_modified!=modified{return Err("源文件已变化，请重新扫描后再登记指纹。".into());}
    conn.execute("INSERT INTO asset_fingerprints(asset_id,sha256,size,modified,prepared_at) VALUES(?1,?2,?3,?4,?5)
        ON CONFLICT(asset_id) DO UPDATE SET sha256=excluded.sha256,size=excluded.size,modified=excluded.modified,prepared_at=excluded.prepared_at",params![id,digest,size,modified,now()]).map_err(err)?;
    Ok(digest)
}
fn check_availability(db:&Path,folder:Option<&str>)->Result<AvailabilityReport,String>{
    let conn=connection(db)?;let prefix=folder_prefix(folder)?;
    let mut stmt=conn.prepare("SELECT id,path,size,modified,status FROM assets WHERE ?1 IS NULL OR substr(path,1,length(?1))=?1").map_err(err)?;
    let rows=stmt.query_map([prefix],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,u64>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?))).map_err(err)?.collect::<Result<Vec<_>,_>>().map_err(err)?;
    drop(stmt);let mut report=AvailabilityReport::default();
    for (id,stored,size,modified,status) in rows{
        report.checked+=1;let path=Path::new(&stored);
        match fs::symlink_metadata(path){
            Err(e)=>{
                let parent_accessible=path.parent().is_some_and(|parent|fs::metadata(parent).is_ok());
                let (state,message)=if e.kind()==std::io::ErrorKind::NotFound&&parent_accessible{
                    report.missing+=1;("missing",format!("文件缺失，目录记录已保留：{e}"))
                }else{report.unavailable+=1;("unavailable",format!("文件或父目录不可访问，可能离线、移动或权限不足；记录已保留：{e}"))};
                conn.execute("UPDATE assets SET status=?1,error=?2 WHERE id=?3",params![state,message,id]).map_err(err)?;
            }
            Ok(metadata)=>{
                let stable=metadata.is_file()&&!is_reparse(&metadata)&&metadata.len()==size&&metadata_stamp(&metadata).as_deref()==Ok(modified.as_str())&&fs::canonicalize(path).ok().as_deref()==Some(path);
                if !stable{report.changed+=1;conn.execute("UPDATE assets SET status='changed',error='文件类型、位置或元数据已变化，请重新扫描。' WHERE id=?1",[&id]).map_err(err)?;continue;}
                report.available+=1;
                if ["missing","unavailable","changed"].contains(&status.as_str()){
                    match metadata_audio(path){
                        Ok((rate,channels,duration))=>{conn.execute("UPDATE assets SET status='ready',error=NULL,rate=?1,channels=?2,duration=?3 WHERE id=?4",params![rate,channels,duration,id]).map_err(err)?;}
                        Err(message)=>{conn.execute("UPDATE assets SET status='error',error=?1 WHERE id=?2",params![message,id]).map_err(err)?;}
                    }
                }
            }
        }
    }
    Ok(report)
}
fn relocate_asset(db:&Path,id:&str,new_path:&str)->Result<String,String>{
    let mut conn=connection(db)?;
    let stored:Option<(String,u64,String)>=conn.query_row("SELECT path,size,modified FROM assets WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(err)?;
    let (old_path,old_size,old_modified)=stored.ok_or("素材不存在。")?;
    let fingerprint:Option<(String,u64,String)>=conn.query_row("SELECT sha256,size,modified FROM asset_fingerprints WHERE asset_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(err)?;
    let (expected,fingerprint_size,fingerprint_modified)=fingerprint.ok_or("旧记录没有内容指纹，无法验证新文件；已保留原索引。请恢复原路径的源文件并登记指纹后再重新定位，不能仅按文件名关联。")?;
    if fingerprint_size!=old_size||fingerprint_modified!=old_modified{return Err("已登记指纹与当前目录版本不一致，请先恢复源文件并重新登记。".into());}
    let path=ordinary_audio_path(new_path)?;let target=path_string(&path)?;
    if target==old_path{return Err("新位置与当前路径相同，无需重新定位。".into());}
    let occupied:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM assets WHERE path_key=?1 AND id<>?2)",params![target,id],|r|r.get(0)).map_err(err)?;
    if occupied{return Err("目标路径已经登记为另一条素材，未合并或覆盖任何记录。".into());}
    let (actual,size,modified)=file_fingerprint(&path)?;
    if actual!=expected||size!=old_size{return Err("目标文件SHA256与登记指纹不一致，未修改原索引。".into());}
    let (rate,channels,duration)=metadata_audio(&path)?;
    let after=fs::metadata(&path).map_err(err)?;
    if after.len()!=size||metadata_stamp(&after)?!=modified{return Err("目标文件验证期间发生变化，请重试。".into());}
    let name=path.file_name().and_then(|n|n.to_str()).ok_or("目标文件名无法表示为Unicode")?;
    let format=audio_format(&path).ok_or("目标格式不支持")?;
    let tx=conn.transaction().map_err(err)?;
    tx.execute("UPDATE assets SET path_key=?1,path=?1,name=?2,format=?3,size=?4,modified=?5,rate=?6,channels=?7,duration=?8,status='ready',error=NULL WHERE id=?9",params![target,name,format,size,modified,rate,channels,duration,id]).map_err(err)?;
    tx.execute("UPDATE asset_fingerprints SET size=?1,modified=?2,prepared_at=?3 WHERE asset_id=?4",params![size,modified,now(),id]).map_err(err)?;
    tx.commit().map_err(err)?;
    Ok(target)
}

/// 波形/时长一致的重复素材合并：保留 keep 一条，把其余条的收藏、标签、库归属、
/// 播放列表与试听历史汇入保留条，再删除其余目录记录。只整理目录，不移动、不删除磁盘源文件。
fn merge_assets(db:&Path,keep:&str,remove:&[String])->Result<u64,String>{
    if keep.is_empty()||keep.len()>128{return Err("保留的素材ID无效。".into());}
    if remove.is_empty()||remove.len()>500{return Err("请一次合并1至500条重复素材。".into());}
    let unique:std::collections::BTreeSet<_>=remove.iter().collect();
    if unique.len()!=remove.len(){return Err("合并列表存在重复ID。".into());}
    if remove.iter().any(|id|id==keep){return Err("保留素材不能同时出现在合并列表中。".into());}
    let mut conn=connection(db)?;
    for id in std::iter::once(keep).chain(remove.iter().map(|value|value.as_str())){
        let exists:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM assets WHERE id=?1)",[id],|r|r.get(0)).map_err(err)?;
        if !exists{return Err(format!("素材不存在，整批未合并：{id}"));}
    }
    let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(err)?;
    for rid in remove{
        // 收藏取并集；保留条未分组时继承被合并条的分组
        let favorite:bool=tx.query_row("SELECT COALESCE((SELECT favorite FROM user_metadata WHERE asset_id=?1),0)",[rid],|r|r.get(0)).map_err(err)?;
        if favorite{tx.execute("INSERT INTO user_metadata(asset_id,favorite) VALUES(?1,1) ON CONFLICT(asset_id) DO UPDATE SET favorite=1",[keep]).map_err(err)?;}
        let removed_group:Option<String>=tx.query_row("SELECT favorite_group FROM user_metadata WHERE asset_id=?1",[rid],|r|r.get(0)).optional().map_err(err)?.flatten();
        if let Some(name)=removed_group.filter(|value|!value.trim().is_empty()){
            tx.execute("UPDATE user_metadata SET favorite_group=?2 WHERE asset_id=?1 AND COALESCE(TRIM(favorite_group),'')=''",params![keep,name]).map_err(err)?;
        }
        // 标签取并集，保留原有顺序
        let removed_tags:String=tx.query_row("SELECT COALESCE((SELECT tags FROM user_metadata WHERE asset_id=?1),'[]')",[rid],|r|r.get(0)).map_err(err)?;
        let kept_tags:String=tx.query_row("SELECT COALESCE((SELECT tags FROM user_metadata WHERE asset_id=?1),'[]')",[keep],|r|r.get(0)).map_err(err)?;
        let mut merged:Vec<String>=serde_json::from_str(&kept_tags).map_err(err)?;
        for tag in serde_json::from_str::<Vec<String>>(&removed_tags).map_err(err)?.into_iter(){if !merged.contains(&tag){merged.push(tag);}}
        if merged.len()>20{return Err("合并后标签超过每条20个的上限，请先精简标签再合并。".into());}
        let json=serde_json::to_string(&merged).map_err(err)?;let text=merged.join(" ");
        tx.execute("INSERT INTO user_metadata(asset_id,tags,tags_search) VALUES(?1,?2,?3) ON CONFLICT(asset_id) DO UPDATE SET tags=excluded.tags,tags_search=excluded.tags_search",params![keep,json,text]).map_err(err)?;
        // 库归属与播放列表整批迁移到保留条
        tx.execute("INSERT OR IGNORE INTO asset_libraries(asset_id,library) SELECT ?1,library FROM asset_libraries WHERE asset_id=?2",params![keep,rid]).map_err(err)?;
        tx.execute("INSERT OR IGNORE INTO playlist_items(playlist_id,asset_id,added) SELECT playlist_id,?1,added FROM playlist_items WHERE asset_id=?2",params![keep,rid]).map_err(err)?;
        // 试听历史只保留最近一次
        let played:Option<String>=tx.query_row("SELECT played_at FROM play_history WHERE asset_id=?1",[rid],|r|r.get(0)).optional().map_err(err)?;
        if let Some(played)=played{
            let kept:Option<String>=tx.query_row("SELECT played_at FROM play_history WHERE asset_id=?1",[keep],|r|r.get(0)).optional().map_err(err)?;
            match kept{
                None=>{tx.execute("INSERT INTO play_history(asset_id,played_at) VALUES(?1,?2)",params![keep,played]).map_err(err)?;}
                Some(old)=>{if played>old{tx.execute("UPDATE play_history SET played_at=?2 WHERE asset_id=?1",params![keep,played]).map_err(err)?;}}
            }
        }
        tx.execute("DELETE FROM assets WHERE id=?1",[rid]).map_err(err)?;
    }
    tx.commit().map_err(err)?;
    Ok(remove.len() as u64)
}

#[tauri::command]
pub async fn catalog_preview(root:String,state:tauri::State<'_,CatalogState>)->Result<String,String>{let db=state.inner.db_path.clone();tauri::async_runtime::spawn_blocking(move||preview_and_remember(&db,&root)).await.map_err(err)?}
#[tauri::command]
pub async fn catalog_get_items(ids:Vec<String>,state:tauri::State<'_,CatalogState>)->Result<Vec<CatalogItem>,String>{
    if ids.len()>100{return Err("一次最多读取100条素材。".into());}
    let db=state.database_path();
    tauri::async_runtime::spawn_blocking(move||{
        let conn=connection(&db)?;let mut items=Vec::new();let mut seen=std::collections::HashSet::new();
        for id in ids{
            if !seen.insert(id.clone()){continue;}
            let item=conn.query_row("SELECT a.id,a.path,a.name,a.format,a.size,a.modified,a.rate,a.channels,a.duration,a.status,a.error,COALESCE(u.favorite,0),COALESCE(u.tags,'[]'),COALESCE(u.favorite_group,'') FROM assets a LEFT JOIN user_metadata u ON a.id=u.asset_id WHERE a.id=?1",[id],|r|{
                let tags:String=r.get(12)?;let tags=serde_json::from_str(&tags).map_err(|e|rusqlite::Error::FromSqlConversionFailure(12,rusqlite::types::Type::Text,Box::new(e)))?;
                let group:String=r.get(13)?;
                Ok(CatalogItem{id:r.get(0)?,path:r.get(1)?,name:r.get(2)?,format:r.get(3)?,size:r.get(4)?,modified:r.get(5)?,rate:r.get(6)?,channels:r.get(7)?,duration:r.get(8)?,status:r.get(9)?,error:r.get(10)?,favorite:r.get(11)?,favorite_group:if group.is_empty(){None}else{Some(group)},tags})
            }).optional().map_err(err)?;if let Some(item)=item{items.push(item);}
        }Ok(items)
    }).await.map_err(err)?
}
#[tauri::command]
pub async fn catalog_list(state:tauri::State<'_,CatalogState>,offset:usize,limit:usize,folder:Option<String>,query:Option<String>,format:Option<String>,sort:Option<String>,favorites_only:Option<bool>,library:Option<String>,tag:Option<String>,tags:Option<Vec<String>>,history_only:Option<bool>,playlist_id:Option<String>,min_duration:Option<f64>,max_duration:Option<f64>,multilingual:Option<bool>,metadata:Option<bool>,favorite_group:Option<String>,favorite_ungrouped:Option<bool>,ready_only:Option<bool>)->Result<CatalogPage,String>{let db=state.inner.db_path.clone();tauri::async_runtime::spawn_blocking(move||search_page(&db,offset,limit,SearchFilter{folder:folder.as_deref(),query:query.as_deref(),format:format.as_deref(),sort:sort.as_deref(),favorites_only:favorites_only.unwrap_or(false),favorite_group:favorite_group.as_deref(),favorite_ungrouped:favorite_ungrouped.unwrap_or(false),library:library.as_deref(),tag:tag.as_deref(),tags:tags.unwrap_or_default(),history_only:history_only.unwrap_or(false),playlist_id:playlist_id.as_deref(),min_duration,max_duration,multilingual:multilingual.unwrap_or(false),metadata:metadata.unwrap_or(false),ready_only:ready_only.unwrap_or(false)})).await.map_err(err)?}
/// 侧栏「最近添加」用：按入库先后取最近 N 条素材。
/// assets 的主键是 TEXT id，SQLite 隐藏自增列 rowid 因此天然等于「入库顺序」，
/// 比 modified（文件修改时间，可能是十年前的老素材）更贴近「最近加进来的」。
fn recent_items(db:&Path,limit:usize)->Result<Vec<CatalogItem>,String>{
    let limit=limit.clamp(1,200);
    let conn=connection(db)?;
    let mut stmt=conn.prepare("SELECT a.id,a.path,a.name,a.format,a.size,a.modified,a.rate,a.channels,a.duration,a.status,a.error,COALESCE(u.favorite,0),COALESCE(u.tags,'[]'),COALESCE(u.favorite_group,'') FROM assets a LEFT JOIN user_metadata u ON a.id=u.asset_id ORDER BY a.rowid DESC LIMIT ?1").map_err(err)?;
    let rows=stmt.query_map([limit as i64],|r|{
        let tags:String=r.get(12)?;let tags=serde_json::from_str(&tags).map_err(|e|rusqlite::Error::FromSqlConversionFailure(12,rusqlite::types::Type::Text,Box::new(e)))?;
        let group:String=r.get(13)?;
        Ok(CatalogItem{id:r.get(0)?,path:r.get(1)?,name:r.get(2)?,format:r.get(3)?,size:r.get(4)?,modified:r.get(5)?,rate:r.get(6)?,channels:r.get(7)?,duration:r.get(8)?,status:r.get(9)?,error:r.get(10)?,favorite:r.get(11)?,favorite_group:if group.is_empty(){None}else{Some(group)},tags})
    }).map_err(err)?;
    let mut items=Vec::new();
    for row in rows{items.push(row.map_err(err)?);}
    Ok(items)
}
#[tauri::command]
pub async fn catalog_recent(state:tauri::State<'_,CatalogState>,limit:usize)->Result<Vec<CatalogItem>,String>{let db=state.inner.db_path.clone();tauri::async_runtime::spawn_blocking(move||recent_items(&db,limit)).await.map_err(err)?}
#[tauri::command]
pub async fn catalog_assign_library(ids:Vec<String>,library:String,state:tauri::State<'_,CatalogState>)->Result<u64,String>{let db=state.inner.db_path.clone();tauri::async_runtime::spawn_blocking(move||assign_library(&db,ids,&library)).await.map_err(err)?}
#[tauri::command]
pub async fn catalog_set_favorite(state:tauri::State<'_,CatalogState>,id:String,favorite:bool)->Result<(),String>{let db=state.inner.db_path.clone();tauri::async_runtime::spawn_blocking(move||set_favorite(&db,&id,favorite)).await.map_err(err)?}
#[tauri::command]
pub async fn catalog_set_favorite_group(ids:Vec<String>,group:Option<String>,state:tauri::State<'_,CatalogState>)->Result<u64,String>{let db=state.inner.db_path.clone();tauri::async_runtime::spawn_blocking(move||set_favorite_group(&db,ids,group.as_deref())).await.map_err(err)?}
#[tauri::command]
pub async fn catalog_set_tags(state:tauri::State<'_,CatalogState>,id:String,tags:Vec<String>)->Result<(),String>{let db=state.inner.db_path.clone();tauri::async_runtime::spawn_blocking(move||set_tags(&db,&id,tags)).await.map_err(err)?}
#[tauri::command]
pub async fn catalog_history(state:tauri::State<'_,CatalogState>)->Result<Vec<String>,String>{let db=state.inner.db_path.clone();tauri::async_runtime::spawn_blocking(move||history(&db)).await.map_err(err)?}
#[tauri::command]
pub async fn catalog_folders(state:tauri::State<'_,CatalogState>,library:Option<String>)->Result<Vec<FolderNode>,String>{let db=state.inner.db_path.clone();tauri::async_runtime::spawn_blocking(move||folder_nodes_in_library(&db,library.as_deref())).await.map_err(err)?}
#[tauri::command]
pub async fn catalog_jobs(state:tauri::State<'_,CatalogState>)->Result<Vec<ImportJob>,String>{let db=state.inner.db_path.clone();tauri::async_runtime::spawn_blocking(move||jobs(&db)).await.map_err(err)?}
#[tauri::command]
pub async fn catalog_start(root:String,state:tauri::State<'_,CatalogState>,library:Option<String>)->Result<String,String>{let state=state.inner().clone();tauri::async_runtime::spawn_blocking(move||state.start_in_library(root,None,library.as_deref().unwrap_or("sound"))).await.map_err(err)?}
#[tauri::command]
pub async fn catalog_cancel(id:String,state:tauri::State<'_,CatalogState>)->Result<(),String>{let state=state.inner().clone();tauri::async_runtime::spawn_blocking(move||state.cancel(&id)).await.map_err(err)?}
#[tauri::command]
pub async fn catalog_resume(id:String,state:tauri::State<'_,CatalogState>)->Result<String,String>{
    let state=state.inner().clone();tauri::async_runtime::spawn_blocking(move||state.resume(id)).await.map_err(err)?
}
#[tauri::command]
pub async fn catalog_check_availability(folder:Option<String>,state:tauri::State<'_,CatalogState>)->Result<AvailabilityReport,String>{let state=state.inner().clone();tauri::async_runtime::spawn_blocking(move||state.with_idle(|db|check_availability(db,folder.as_deref()))).await.map_err(err)?}
#[tauri::command]
pub async fn catalog_prepare_fingerprint(id:String,state:tauri::State<'_,CatalogState>)->Result<String,String>{let state=state.inner().clone();tauri::async_runtime::spawn_blocking(move||state.with_idle(|db|prepare_fingerprint(db,&id))).await.map_err(err)?}
#[tauri::command]
pub async fn catalog_relocate(id:String,new_path:String,state:tauri::State<'_,CatalogState>)->Result<String,String>{let state=state.inner().clone();tauri::async_runtime::spawn_blocking(move||state.with_idle(|db|relocate_asset(db,&id,&new_path))).await.map_err(err)?}
#[tauri::command]
pub async fn catalog_merge_assets(keep:String,remove:Vec<String>,state:tauri::State<'_,CatalogState>)->Result<u64,String>{let state=state.inner().clone();tauri::async_runtime::spawn_blocking(move||state.with_idle(|db|merge_assets(db,&keep,&remove))).await.map_err(err)?}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture{root:PathBuf,db:PathBuf}
    impl Fixture {
        fn new()->Self{let root=Path::new(env!("CARGO_MANIFEST_DIR")).join("../build/s1.2-validation/test-data").join(Uuid::new_v4().to_string());fs::create_dir_all(root.join("audio")).unwrap();Self{db:root.join("catalog.sqlite3"),root}}
        fn state(&self)->CatalogState{CatalogState::new(self.db.clone()).unwrap()}
        fn audio(&self)->PathBuf{self.root.join("audio")}
        fn wav(&self,name:&str){fs::write(self.audio().join(name),wav()).unwrap();}
    }
    // 测试临时目录保留供审计，不删除用户目录。
    fn wav()->Vec<u8>{let mut b=Vec::new();b.extend(b"RIFF");b.extend(38u32.to_le_bytes());b.extend(b"WAVEfmt ");b.extend(16u32.to_le_bytes());b.extend(1u16.to_le_bytes());b.extend(1u16.to_le_bytes());b.extend(44100u32.to_le_bytes());b.extend(88200u32.to_le_bytes());b.extend(2u16.to_le_bytes());b.extend(16u16.to_le_bytes());b.extend(b"data");b.extend(2u32.to_le_bytes());b.extend(0i16.to_le_bytes());b}
    fn direct(f:&Fixture,id:&str,cancel:&AtomicBool)->ImportJob{
        let conn=connection(&f.db).unwrap();let root=path_string(&f.audio()).unwrap();
        conn.execute("INSERT INTO jobs(id,root,status,created,changed)VALUES(?1,?2,'running',?3,?3)",params![id,root,now()]).unwrap();
        scan(&f.db,ImportJob{id:id.into(),root,status:"running".into(),..Default::default()},cancel).unwrap();
        jobs(&f.db).unwrap().into_iter().find(|j|j.id==id).unwrap()
    }
    fn wait_for_import(state:&CatalogState){let deadline=std::time::Instant::now()+Duration::from_secs(20);while state.inner.active.lock().unwrap().is_some(){assert!(std::time::Instant::now()<deadline,"导入任务超时");std::thread::sleep(Duration::from_millis(2));}}
    fn library_page(f:&Fixture,library:&str)->CatalogPage{search_page(&f.db,0,100,SearchFilter{library:Some(library),..Default::default()}).unwrap()}
    #[test]
    fn ready_only_filter_composes_with_keyword_and_total(){
        let f=Fixture::new();let _state=f.state();f.wav("ready.wav");f.wav("missing.wav");
        direct(&f,"ready-only-filter",&AtomicBool::new(false));
        let all=list_page(&f.db,0,100).unwrap();assert_eq!(all.total,2);
        let id=all.items.iter().find(|item|item.name=="missing.wav").unwrap().id.clone();
        {let conn=connection(&f.db).unwrap();conn.execute("UPDATE assets SET status='missing' WHERE id=?1",params![id]).unwrap();}
        // 关闭开关时两条都在；开启后仅保留 status='ready'，COUNT 与分页结果一致。
        assert_eq!(search_page(&f.db,0,100,SearchFilter{query:Some(".wav"),..Default::default()}).unwrap().total,2);
        let filtered=search_page(&f.db,0,100,SearchFilter{query:Some(".wav"),ready_only:true,..Default::default()}).unwrap();
        assert_eq!(filtered.total,1);assert_eq!(filtered.items.len(),1);assert_eq!(filtered.items[0].name,"ready.wav");
        // 关键词仍独立生效，隐藏不可用素材不会放宽其它条件。
        assert_eq!(search_page(&f.db,0,100,SearchFilter{query:Some("不存在的素材"),ready_only:true,..Default::default()}).unwrap().total,0);
        assert_eq!(search_page(&f.db,0,100,SearchFilter{ready_only:true,..Default::default()}).unwrap().total,1);
    }
    #[test]
    fn metadata_toggle_only_adds_embedded_fields_to_text_search(){
        let f=Fixture::new();let _state=f.state();f.wav("door-close.wav");
        direct(&f,"meta-scan",&AtomicBool::new(false));
        let id=list_page(&f.db,0,100).unwrap().items[0].id.clone();
        {let conn=connection(&f.db).unwrap();conn.execute("UPDATE assets SET meta_search='近场收录 foley_marker' WHERE id=?1",params![id]).unwrap();}
        // 关闭时只搜文件名：内嵌元数据词与标签词都不命中，名称仍命中。
        assert_eq!(search_page(&f.db,0,100,SearchFilter{query:Some("foley_marker"),..Default::default()}).unwrap().total,0);
        assert_eq!(search_page(&f.db,0,100,SearchFilter{query:Some("近场收录"),..Default::default()}).unwrap().total,0);
        assert_eq!(search_page(&f.db,0,100,SearchFilter{query:Some("door-close"),..Default::default()}).unwrap().total,1);
        set_tags(&f.db,&id,vec!["门".into()]).unwrap();
        assert_eq!(search_page(&f.db,0,100,SearchFilter{query:Some("门"),..Default::default()}).unwrap().total,0);
        assert_eq!(search_page(&f.db,0,100,SearchFilter{query:Some("门"),metadata:true,..Default::default()}).unwrap().total,1);
        // 开启后两个词都命中，名称搜索与注入防护不受影响。
        assert_eq!(search_page(&f.db,0,100,SearchFilter{query:Some("foley_marker"),metadata:true,..Default::default()}).unwrap().total,1);
        assert_eq!(search_page(&f.db,0,100,SearchFilter{query:Some("近场收录"),metadata:true,..Default::default()}).unwrap().total,1);
        assert_eq!(search_page(&f.db,0,100,SearchFilter{query:Some("door-close"),metadata:true,..Default::default()}).unwrap().total,1);
        assert_eq!(search_page(&f.db,0,100,SearchFilter{query:Some("' OR 1=1 --"),metadata:true,..Default::default()}).unwrap().total,0);
        // 标签筛选仍是与文本关键词独立的一层条件。
        set_tags(&f.db,&id,vec!["门".into()]).unwrap();
        assert_eq!(search_page(&f.db,0,100,SearchFilter{query:Some("foley_marker"),metadata:true,tag:Some("门"),..Default::default()}).unwrap().total,1);
        assert_eq!(search_page(&f.db,0,100,SearchFilter{query:Some("foley_marker"),metadata:true,tag:Some("不存在"),..Default::default()}).unwrap().total,0);
    }
    #[test]
    fn metadata_text_extracts_bwf_fields_and_skips_non_wav(){
        let f=Fixture::new();let path=f.audio().join("with-meta.wav");fs::write(&path,wav()).unwrap();
        crate::bwf_metadata::write(&path,&crate::bwf_metadata::MetadataPatch{
            description:Some("金属门闩特写".into()),note:Some("近场收录".into()),
            keywords:Some("门 金属".into()),category:Some("FOLEY".into()),..Default::default()
        }).unwrap();
        let text=metadata_search_text(&path,"WAV");
        assert!(text.contains("金属门闩特写"),"抽取结果：{text}");
        assert!(text.contains("近场收录"),"抽取结果：{text}");
        assert!(text.contains("FOLEY"),"抽取结果：{text}");
        assert_eq!(metadata_search_text(&path,"MP3"),"");
    }
    #[test]
    fn write_back_refreshes_metadata_index_for_search(){
        let f=Fixture::new();let state=f.state();f.wav("indexed.wav");
        direct(&f,"index-scan",&AtomicBool::new(false));
        let id=list_page(&f.db,0,100).unwrap().items[0].id.clone();
        let path=f.audio().join("indexed.wav");
        crate::bwf_metadata::write(&path,&crate::bwf_metadata::MetadataPatch{note:Some("回写后命中词".into()),..Default::default()}).unwrap();
        assert_eq!(search_page(&f.db,0,100,SearchFilter{query:Some("回写后命中词"),metadata:true,..Default::default()}).unwrap().total,0);
        state.refresh_metadata_index(&id).unwrap();
        assert_eq!(search_page(&f.db,0,100,SearchFilter{query:Some("回写后命中词"),metadata:true,..Default::default()}).unwrap().total,1);
    }
    #[test]
    fn libraries_migrate_existing_records_to_sound_without_losing_annotations(){
        let f=Fixture::new();let state=f.state();f.wav("existing.wav");direct(&f,"legacy-scan",&AtomicBool::new(false));let item=list_page(&f.db,0,100).unwrap().items.remove(0);
        set_favorite(&f.db,&item.id,true).unwrap();set_tags(&f.db,&item.id,vec!["旧标签".into()]).unwrap();
        {let conn=connection(&f.db).unwrap();conn.execute("DELETE FROM asset_libraries",[]).unwrap();conn.execute("DELETE FROM meta WHERE key='asset_libraries_revision'",[]).unwrap();conn.execute_batch("ALTER TABLE jobs DROP COLUMN library;").unwrap();}
        drop(state);let _reopened=f.state();let sound=library_page(&f,"sound");assert_eq!(sound.total,1);assert_eq!(sound.items[0].id,item.id);assert!(sound.items[0].favorite);assert_eq!(sound.items[0].tags,vec!["旧标签"]);assert_eq!(library_page(&f,"music").total,0);assert_eq!(jobs(&f.db).unwrap()[0].library,"sound");assert_eq!(list_page(&f.db,0,100).unwrap().total,1);
    }
    #[test]
    fn libraries_music_import_survives_restart_and_skipped_sound_scan_adds_membership(){
        let f=Fixture::new();let state=f.state();f.wav("same-source.wav");
        let first=state.start_in_library(path_string(&f.audio()).unwrap(),None,"music").unwrap();wait_for_import(&state);
        assert_eq!(jobs(&f.db).unwrap().into_iter().find(|j|j.id==first).unwrap().library,"music");assert_eq!(library_page(&f,"sound").total,0);let original=library_page(&f,"music").items.remove(0);set_favorite(&f.db,&original.id,true).unwrap();set_tags(&f.db,&original.id,vec!["共享标注".into()]).unwrap();
        drop(state);let state=f.state();assert_eq!(library_page(&f,"sound").total,0);assert_eq!(library_page(&f,"music").total,1);
        let second=state.start(path_string(&f.audio()).unwrap(),None).unwrap();wait_for_import(&state);let job=jobs(&f.db).unwrap().into_iter().find(|j|j.id==second).unwrap();assert_eq!(job.library,"sound");assert_eq!(job.skipped,1);assert_eq!(job.added,0);
        let sound=library_page(&f,"sound");let music=library_page(&f,"music");assert_eq!(sound.total,1);assert_eq!(music.total,1);assert_eq!(sound.items[0].id,original.id);assert_eq!(music.items[0].id,original.id);assert!(sound.items[0].favorite);assert_eq!(sound.items[0].tags,vec!["共享标注"]);assert_eq!(list_page(&f.db,0,100).unwrap().total,1);assert_eq!(fs::read(f.audio().join("same-source.wav")).unwrap(),wav());
    }
    #[test]
    fn libraries_legacy_app_insert_after_migration_is_visible_on_restart(){
        let f=Fixture::new();let state=f.state();f.wav("music-only.wav");state.start_in_library(path_string(&f.audio()).unwrap(),None,"music").unwrap();wait_for_import(&state);
        let music_id=library_page(&f,"music").items.remove(0).id;assert_eq!(library_page(&f,"sound").total,0);drop(state);
        f.wav("legacy-app.wav");let old_path=path_string(&fs::canonicalize(f.audio().join("legacy-app.wav")).unwrap()).unwrap();let old_id=Uuid::new_v4().to_string();
        // 模拟升级后再运行旧版：旧版仅登记assets，不知道asset_libraries表。
        {let conn=connection(&f.db).unwrap();assert_eq!(conn.query_row("SELECT value FROM meta WHERE key='asset_libraries_revision'",[],|r|r.get::<_,String>(0)).unwrap(),"1");conn.execute("INSERT INTO assets(id,path_key,path,name,format,size,modified,status) VALUES(?1,?2,?2,'legacy-app.wav','WAV',46,'0','ready')",params![old_id,old_path]).unwrap();}
        let state=f.state();let sound=library_page(&f,"sound");let music=library_page(&f,"music");assert_eq!(sound.total,1);assert_eq!(sound.items[0].id,old_id);assert_eq!(sound.items[0].path,old_path);assert_eq!(music.total,1);assert_eq!(music.items[0].id,music_id);assert_eq!(list_page(&f.db,0,100).unwrap().total,2);
        drop(state);let _reopened=f.state();assert_eq!(library_page(&f,"sound").total,1);assert_eq!(library_page(&f,"music").total,1);assert_eq!(fs::read(f.audio().join("legacy-app.wav")).unwrap(),wav());
    }
    #[test]
    fn libraries_assignment_is_additive_idempotent_and_atomic(){
        let f=Fixture::new();let _state=f.state();f.wav("rain.wav");direct(&f,"assign",&AtomicBool::new(false));let id=list_page(&f.db,0,100).unwrap().items[0].id.clone();
        assert!(assign_library(&f.db,vec![id.clone(),"missing-id".into()],"music").is_err());assert_eq!(library_page(&f,"music").total,0);
        assert_eq!(assign_library(&f.db,vec![id.clone(),id.clone()],"music").unwrap(),1);assert_eq!(assign_library(&f.db,vec![id.clone()],"music").unwrap(),0);assert_eq!(library_page(&f,"sound").total,1);assert_eq!(library_page(&f,"music").total,1);
        assert!(assign_library(&f.db,vec![id],"music'; DELETE FROM assets; --").is_err());assert!(assign_library(&f.db,vec![],"sound").is_err());assert!(search_page(&f.db,0,100,SearchFilter{library:Some("other"),..Default::default()}).is_err());assert_eq!(list_page(&f.db,0,100).unwrap().total,1);
    }
    #[test]
    fn libraries_scan_rolls_back_asset_when_membership_write_fails(){
        let f=Fixture::new();let _state=f.state();f.wav("transaction.wav");let conn=connection(&f.db).unwrap();
        conn.execute_batch("CREATE TRIGGER fail_membership BEFORE INSERT ON asset_libraries WHEN NEW.library='music' BEGIN SELECT RAISE(ABORT,'injected membership failure'); END;").unwrap();
        let mut job=ImportJob{library:"music".into(),..Default::default()};assert!(record_file(&conn,&f.audio().join("transaction.wav"),"WAV",&mut job).is_err());
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM assets",[],|r|r.get::<_,u64>(0)).unwrap(),0);assert_eq!(conn.query_row("SELECT COUNT(*) FROM asset_libraries",[],|r|r.get::<_,u64>(0)).unwrap(),0);assert_eq!(fs::read(f.audio().join("transaction.wav")).unwrap(),wav());
    }
    #[test]
    fn libraries_interrupted_job_resume_preserves_music_scope(){
        let f=Fixture::new();let state=f.state();f.wav("resume.wav");
        {let conn=connection(&f.db).unwrap();conn.execute("INSERT INTO jobs(id,root,status,created,changed,library) VALUES('music-resume',?1,'running',?2,?2,'music')",params![path_string(&f.audio()).unwrap(),now()]).unwrap();}
        drop(state);let state=f.state();let interrupted=jobs(&f.db).unwrap().into_iter().find(|j|j.id=="music-resume").unwrap();assert_eq!(interrupted.status,"interrupted");assert_eq!(interrupted.library,"music");
        state.resume("music-resume".into()).unwrap();wait_for_import(&state);let complete=jobs(&f.db).unwrap().into_iter().find(|j|j.id=="music-resume").unwrap();assert_eq!(complete.status,"completed");assert_eq!(complete.library,"music");assert_eq!(library_page(&f,"sound").total,0);assert_eq!(library_page(&f,"music").total,1);
    }
    #[test]
    fn libraries_folder_tree_and_parameterized_search_are_isolated(){
        let f=Fixture::new();let state=f.state();let sound=f.audio().join("sound");let music=f.audio().join("music");fs::create_dir_all(&sound).unwrap();fs::create_dir_all(&music).unwrap();fs::write(sound.join("雨_100% RAIN.wav"),wav()).unwrap();fs::write(music.join("雨_100% RAIN.wav"),wav()).unwrap();
        state.start_in_library(path_string(&sound).unwrap(),None,"sound").unwrap();wait_for_import(&state);state.start_in_library(path_string(&music).unwrap(),None,"music").unwrap();wait_for_import(&state);
        let sound_path=path_string(&fs::canonicalize(&sound).unwrap()).unwrap();let music_path=path_string(&fs::canonicalize(&music).unwrap()).unwrap();
        let nodes=folder_nodes_in_library(&f.db,Some("sound")).unwrap();assert!(nodes.iter().any(|n|n.path==sound_path&&n.count==1));assert!(!nodes.iter().any(|n|n.path==music_path));
        assert_eq!(search_page(&f.db,0,100,SearchFilter{library:Some("music"),query:Some("雨 _100% rain"),..Default::default()}).unwrap().total,1);assert_eq!(search_page(&f.db,0,100,SearchFilter{library:Some("music"),folder:Some(&sound_path),..Default::default()}).unwrap().total,0);assert_eq!(search_page(&f.db,0,100,SearchFilter{query:Some("雨 _100% rain"),..Default::default()}).unwrap().total,2);
    }
    #[test]
    fn global_search_combines_chinese_ascii_literal_words_and_folder(){
        let f=Fixture::new();let _state=f.state();
        for dir in ["one","two"]{fs::create_dir_all(f.audio().join(dir)).unwrap();}
        fs::write(f.audio().join("one/雨_100% RAIN.wav"),wav()).unwrap();
        fs::write(f.audio().join("one/雨夜 Thunder.wav"),wav()).unwrap();
        fs::write(f.audio().join("two/雨_100% RAIN.wav"),wav()).unwrap();
        direct(&f,"search",&AtomicBool::new(false));
        let all=search_page(&f.db,0,100,SearchFilter{query:Some("雨 RAIN"),..Default::default()}).unwrap();assert_eq!(all.total,2);
        let folder=path_string(&fs::canonicalize(f.audio().join("one")).unwrap()).unwrap();
        assert_eq!(search_page(&f.db,0,100,SearchFilter{folder:Some(&folder),query:Some("雨 _100% rain"),format:Some("wav"),..Default::default()}).unwrap().total,1);
        assert_eq!(search_page(&f.db,0,100,SearchFilter{query:Some("%"),..Default::default()}).unwrap().total,2);
        assert_eq!(search_page(&f.db,0,100,SearchFilter{query:Some("' OR 1=1 --"),..Default::default()}).unwrap().total,0);
        assert_eq!(search_page(&f.db,0,100,SearchFilter{query:Some("雨"),format:Some("MP3"),..Default::default()}).unwrap().total,0);
        assert!(search_page(&f.db,0,100,SearchFilter{sort:Some("name; DELETE FROM assets"),..Default::default()}).is_err());
        assert!(search_page(&f.db,0,100,SearchFilter{query:Some(&"汉".repeat(1025)),..Default::default()}).is_err());
        assert!(search_page(&f.db,0,100,SearchFilter{query:Some(&vec!["雨";33].join(" ")),..Default::default()}).is_err());
        assert_eq!(list_page(&f.db,0,100).unwrap().total,3);
    }
    #[test]
    fn multilingual_search_expands_only_text_words(){
        let f=Fixture::new();let _state=f.state();
        fs::write(f.audio().join("雨声_OnlyChinese.wav"),wav()).unwrap();
        fs::write(f.audio().join("thunder_OnlyEnglish.wav"),wav()).unwrap();
        direct(&f,"search",&AtomicBool::new(false));
        // 关闭扩展时按字面匹配：库里没有含 rain 的名称。
        assert_eq!(search_page(&f.db,0,100,SearchFilter{query:Some("rain"),..Default::default()}).unwrap().total,0);
        // 开启后英文 rain 扩展出「雨声」，命中纯中文文件名。
        assert_eq!(search_page(&f.db,0,100,SearchFilter{query:Some("rain"),multilingual:true,..Default::default()}).unwrap().total,1);
        // 中文「雷」扩展出 thunder，命中纯英文文件名。
        assert_eq!(search_page(&f.db,0,100,SearchFilter{query:Some("雷"),multilingual:true,..Default::default()}).unwrap().total,1);
        // 词与词之间仍是 AND：两个文件各只满足一个词。
        assert_eq!(search_page(&f.db,0,100,SearchFilter{query:Some("rain 雷"),multilingual:true,..Default::default()}).unwrap().total,0);
        // 格式/分类筛选不受扩展影响。
        assert_eq!(search_page(&f.db,0,100,SearchFilter{query:Some("rain"),multilingual:true,format:Some("MP3"),..Default::default()}).unwrap().total,0);
        // 注入串仍按字面绑定，不放大结果。
        assert_eq!(search_page(&f.db,0,100,SearchFilter{query:Some("' OR 1=1 --"),multilingual:true,..Default::default()}).unwrap().total,0);
    }
    #[test]
    fn search_paginates_complete_results_and_orders_unknown_duration_last(){
        let f=Fixture::new();let _state=f.state();
        for i in 0..121{f.wav(&format!("needle-{i:03}.wav"));}f.wav("other.wav");
        direct(&f,"pages",&AtomicBool::new(false));
        let first=search_page(&f.db,0,100,SearchFilter{query:Some("needle"),..Default::default()}).unwrap();
        let second=search_page(&f.db,100,100,SearchFilter{query:Some("needle"),..Default::default()}).unwrap();
        assert_eq!(first.total,121);assert_eq!(first.items.len(),100);assert_eq!(second.items.len(),21);
        assert!(first.items.iter().all(|a|second.items.iter().all(|b|a.id!=b.id)));
        let conn=connection(&f.db).unwrap();conn.execute("UPDATE assets SET duration=CASE WHEN name='other.wav' THEN 2.0 ELSE NULL END",[]).unwrap();
        let sorted=search_page(&f.db,0,100,SearchFilter{sort:Some("duration"),..Default::default()}).unwrap();assert_eq!(sorted.items[0].name,"other.wav");
        let unknown_ids:Vec<_>=sorted.items.iter().skip(1).map(|a|a.id.clone()).collect();let mut expected=unknown_ids.clone();expected.sort();assert_eq!(unknown_ids,expected);
    }
    #[test]
    fn sorts_descending_and_filters_by_duration_range(){
        let f=Fixture::new();let _state=f.state();
        for name in ["a.wav","b.wav","c.wav"]{f.wav(name);}
        direct(&f,"range",&AtomicBool::new(false));
        let conn=connection(&f.db).unwrap();
        conn.execute("UPDATE assets SET duration=CASE name WHEN 'a.wav' THEN 5.0 WHEN 'b.wav' THEN 30.0 ELSE 120.0 END",[]).unwrap();
        drop(conn);
        let desc=search_page(&f.db,0,100,SearchFilter{sort:Some("duration_desc"),..Default::default()}).unwrap();
        assert_eq!(desc.items.iter().map(|a|a.name.as_str()).collect::<Vec<_>>(),vec!["c.wav","b.wav","a.wav"]);
        let name_desc=search_page(&f.db,0,100,SearchFilter{sort:Some("name_desc"),..Default::default()}).unwrap();
        assert_eq!(name_desc.items.iter().map(|a|a.name.as_str()).collect::<Vec<_>>(),vec!["c.wav","b.wav","a.wav"]);
        // 时长区间：区间外的记录与时长未知的记录都不出现。
        let ranged=search_page(&f.db,0,100,SearchFilter{min_duration:Some(10.0),max_duration:Some(60.0),..Default::default()}).unwrap();
        assert_eq!(ranged.total,1);assert_eq!(ranged.items[0].name,"b.wav");
        let from_ten=search_page(&f.db,0,100,SearchFilter{min_duration:Some(10.0),..Default::default()}).unwrap();
        assert_eq!(from_ten.items.iter().map(|a|a.name.as_str()).collect::<Vec<_>>(),vec!["b.wav","c.wav"]);
        // 排序表达式仍只走白名单。
        assert!(search_page(&f.db,0,100,SearchFilter{sort:Some("duration; DROP TABLE assets"),..Default::default()}).is_err());
    }
    #[test]
    fn multiple_dimension_tag_filters_intersect(){
        let f=Fixture::new();let _state=f.state();
        for name in ["a.wav","b.wav","c.wav"]{f.wav(name);}
        direct(&f,"dimension",&AtomicBool::new(false));
        let items=list_page(&f.db,0,100).unwrap().items;
        assert_eq!(items.len(),3);
        let tags=[vec!["流派:电子","情绪:紧张"],vec!["流派:电子"],vec!["情绪:紧张"]];
        for (item,values) in items.iter().zip(tags){set_tags(&f.db,&item.id,values.into_iter().map(str::to_string).collect()).unwrap();}
        let both=search_page(&f.db,0,100,SearchFilter{tags:vec!["流派:电子".into(),"情绪:紧张".into()],..Default::default()}).unwrap();
        assert_eq!(both.total,1);
        let one=search_page(&f.db,0,100,SearchFilter{tags:vec!["流派:电子".into()],..Default::default()}).unwrap();
        assert_eq!(one.total,2);
        assert!(search_page(&f.db,0,100,SearchFilter{tags:(0..9).map(|i|format!("t{i}")).collect(),..Default::default()}).is_err());
    }
    #[test]
    fn favorite_tags_persist_across_rescan_and_restart(){
        let f=Fixture::new();let state=f.state();f.wav("素材.wav");direct(&f,"before",&AtomicBool::new(false));
        let id=list_page(&f.db,0,100).unwrap().items[0].id.clone();
        set_favorite(&f.db,&id,true).unwrap();
        set_tags(&f.db,&id,vec![" 深夜 ".into(),"Deep".into(),"深夜".into(),"".into(),"雨_100%".into()]).unwrap();
        let result=search_page(&f.db,0,100,SearchFilter{query:Some("深夜 deep 雨_100%"),favorites_only:true,metadata:true,..Default::default()}).unwrap();
        assert_eq!(result.total,1);assert_eq!(result.items[0].tags,vec!["深夜","Deep","雨_100%"]);assert!(result.items[0].favorite);
        let mut changed=wav();changed.extend([0,0]);fs::write(f.audio().join("素材.wav"),changed).unwrap();direct(&f,"after",&AtomicBool::new(false));
        drop(state);let _reopened=f.state();let saved=list_page(&f.db,0,100).unwrap();assert_eq!(saved.items[0].id,id);assert!(saved.items[0].favorite);assert_eq!(saved.items[0].tags.len(),3);
        set_favorite(&f.db,&id,false).unwrap();assert_eq!(search_page(&f.db,0,100,SearchFilter{favorites_only:true,..Default::default()}).unwrap().total,0);assert_eq!(list_page(&f.db,0,100).unwrap().items[0].tags.len(),3);
        assert!(set_tags(&f.db,&id,vec!["长".repeat(41)]).is_err());assert!(set_tags(&f.db,&id,vec!["x".into();21]).is_err());assert!(set_tags(&f.db,&id,vec!["x\ny".into()]).is_err());
        assert_eq!(list_page(&f.db,0,100).unwrap().items[0].tags.len(),3);
        assert!(set_favorite(&f.db,"unknown",true).is_err());assert!(set_tags(&f.db,"unknown",vec![]).is_err());
    }
    #[test]
    fn favorite_groups_set_group_also_favorites_and_unfavorite_clears_it(){
        let f=Fixture::new();let state=f.state();f.wav("甲.wav");f.wav("乙.wav");direct(&f,"before",&AtomicBool::new(false));
        let ids=list_page(&f.db,0,100).unwrap().items.into_iter().map(|item|item.id).collect::<Vec<_>>();
        assert_eq!(set_favorite_group(&f.db,vec![ids[0].clone()],Some(" 常用 ")).unwrap(),1);
        let grouped=search_page(&f.db,0,100,SearchFilter{favorite_group:Some("常用"),..Default::default()}).unwrap();
        assert_eq!(grouped.total,1);assert!(grouped.items[0].favorite);assert_eq!(grouped.items[0].favorite_group.as_deref(),Some("常用"));
        assert_eq!(search_page(&f.db,0,100,SearchFilter{favorite_ungrouped:true,..Default::default()}).unwrap().total,0);
        assert_eq!(set_favorite_group(&f.db,vec![ids[1].clone()],None).unwrap(),1);
        assert_eq!(search_page(&f.db,0,100,SearchFilter{favorite_ungrouped:true,..Default::default()}).unwrap().total,0);
        assert_eq!(search_page(&f.db,0,100,SearchFilter{favorites_only:true,..Default::default()}).unwrap().total,1);
        // 清空分组保留收藏，取消收藏一并清空分组
        set_favorite_group(&f.db,vec![ids[0].clone()],None).unwrap();
        assert_eq!(list_page(&f.db,0,100).unwrap().items.iter().find(|item|item.id==ids[0]).unwrap().favorite_group,None);
        assert_eq!(search_page(&f.db,0,100,SearchFilter{favorites_only:true,..Default::default()}).unwrap().total,1);
        assert_eq!(search_page(&f.db,0,100,SearchFilter{favorite_ungrouped:true,..Default::default()}).unwrap().total,1);
        set_favorite_group(&f.db,vec![ids[0].clone()],Some("雨声")).unwrap();
        set_favorite(&f.db,&ids[0],false).unwrap();
        assert_eq!(list_page(&f.db,0,100).unwrap().items.iter().find(|item|item.id==ids[0]).unwrap().favorite_group,None);
        assert!(set_favorite_group(&f.db,vec![ids[0].clone()],Some(&"长".repeat(41))).is_err());
        assert!(set_favorite_group(&f.db,vec![],Some("甲")).is_err());
        assert!(set_favorite_group(&f.db,vec!["unknown".into()],Some("甲")).is_err());
    }
    #[test]
    fn asset_resolution_accepts_only_registered_existing_audio_id(){
        let f=Fixture::new();let state=f.state();f.wav("播放.wav");direct(&f,"resolve",&AtomicBool::new(false));
        let item=list_page(&f.db,0,100).unwrap().items.remove(0);
        assert_eq!(state.resolve_asset(&item.id).unwrap(),fs::canonicalize(f.audio().join("播放.wav")).unwrap());
        assert!(state.resolve_asset(&item.path).is_err());assert!(state.resolve_asset("unknown").is_err());
        fs::rename(f.audio().join("播放.wav"),f.audio().join("移走.wav")).unwrap();assert!(state.resolve_asset(&item.id).is_err());
    }
    #[test]
    fn folder_tree_preserves_original_ancestors_and_scope_boundaries(){
        let f=Fixture::new();let _state=f.state();
        for dir in ["雨_100%","雨_100%/子目录","雨_100%-其他"]{
            fs::create_dir_all(f.audio().join(dir)).unwrap();
            fs::write(f.audio().join(dir).join("sample.wav"),wav()).unwrap();
        }
        let j=direct(&f,"tree",&AtomicBool::new(false));assert_eq!(j.added,3);
        let root=fs::canonicalize(f.audio().join("雨_100%")).unwrap();
        let nested=fs::canonicalize(f.audio().join("雨_100%/子目录")).unwrap();
        let nodes=folder_nodes(&f.db).unwrap();
        let root_text=path_string(&root).unwrap();let nested_text=path_string(&nested).unwrap();
        let root_node=nodes.iter().find(|n|n.path==root_text).unwrap();assert_eq!(root_node.name,"雨_100%");assert_eq!(root_node.count,2);
        let child=nodes.iter().find(|n|n.path==nested_text).unwrap();assert_eq!(child.parent.as_deref(),Some(root_text.as_str()));assert_eq!(child.count,1);
        let scoped=list_page_scoped(&f.db,0,100,Some(&root_text)).unwrap();assert_eq!(scoped.total,2);assert_eq!(scoped.items.len(),2);
        assert_eq!(list_page_scoped(&f.db,1,1,Some(&root_text)).unwrap().items.len(),1);
        assert!(list_page_scoped(&f.db,0,100,Some("relative")).is_err());
        assert_eq!(list_page(&f.db,0,100).unwrap().total,3);
    }
    #[test]
    fn checked_path_history_is_unique_bounded_and_persistent(){
        let f=Fixture::new();let state=f.state();let mut checked=Vec::new();
        for i in 0..14{let path=f.audio().join(format!("history-{i:02}"));fs::create_dir_all(&path).unwrap();checked.push(preview_and_remember(&f.db,path.to_str().unwrap()).unwrap());}
        assert_eq!(history(&f.db).unwrap().len(),12);assert_eq!(history(&f.db).unwrap()[0],checked[13]);
        preview_and_remember(&f.db,&checked[4]).unwrap();let recent=history(&f.db).unwrap();assert_eq!(recent[0],checked[4]);assert_eq!(recent.iter().filter(|p|**p==checked[4]).count(),1);
        assert!(preview_and_remember(&f.db,"relative-path").is_err());assert_eq!(history(&f.db).unwrap(),recent);
        drop(state);let _reopened=f.state();assert_eq!(history(&f.db).unwrap(),recent);
    }
    #[test]
    #[cfg(windows)]
    fn different_drives_and_unc_shares_remain_separate_in_tree(){
        let f=Fixture::new();let _state=f.state();let conn=connection(&f.db).unwrap();
        for path in [r"\\?\C:\Audio\same.wav",r"\\?\D:\Audio\same.wav",r"\\?\UNC\server\share\Audio\same.wav"]{
            conn.execute("INSERT INTO assets(id,path_key,path,name,format,size,modified,status) VALUES(?1,?2,?2,'same.wav','WAV',46,'0','ready')",params![Uuid::new_v4().to_string(),path]).unwrap();
        }
        let nodes=folder_nodes(&f.db).unwrap();let audio:Vec<_>=nodes.iter().filter(|n|n.name=="Audio").collect();assert_eq!(audio.len(),3);assert!(audio.iter().all(|n|n.count==1));
        for node in audio{assert_eq!(list_page_scoped(&f.db,0,100,Some(&node.path)).unwrap().total,1);}
        let roots:Vec<_>=nodes.iter().filter(|n|n.parent.is_none()).collect();assert_eq!(roots.len(),3);
        assert!(roots.iter().any(|n|n.name.starts_with(r"\\server\share")));
    }
    #[test]
    fn repeated_scan_and_changed_file_keep_uuid(){
        let f=Fixture::new();let _state=f.state();f.wav("中文 雨.wav");let source=fs::read(f.audio().join("中文 雨.wav")).unwrap();
        let first=direct(&f,"one",&AtomicBool::new(false));assert_eq!(first.added,1);
        let id=list_page(&f.db,0,100).unwrap().items[0].id.clone();
        let second=direct(&f,"two",&AtomicBool::new(false));assert_eq!(second.skipped,1);assert_eq!(list_page(&f.db,0,100).unwrap().total,1);
        let mut changed=source.clone();changed.extend([0,0]);fs::write(f.audio().join("中文 雨.wav"),&changed).unwrap();
        assert_eq!(direct(&f,"three",&AtomicBool::new(false)).updated,1);
        assert_eq!(list_page(&f.db,0,100).unwrap().items[0].id,id);assert_eq!(fs::read(f.audio().join("中文 雨.wav")).unwrap(),changed);
    }
    #[test]
    fn bad_audio_is_retained_with_error_and_retried(){let f=Fixture::new();let _state=f.state();fs::write(f.audio().join("bad.mp3"),b"not audio").unwrap();let j=direct(&f,"bad",&AtomicBool::new(false));assert_eq!(j.failed,1);assert_eq!(j.status,"completed_with_errors");let page=list_page(&f.db,0,10).unwrap();assert_eq!(page.items[0].status,"error");assert!(page.items[0].error.is_some());assert_eq!(direct(&f,"retry",&AtomicBool::new(false)).skipped,0);}
    #[test]
    fn cancellation_preserves_rows_and_recovery_marks_interrupted(){
        let f=Fixture::new();let state=f.state();f.wav("a.wav");direct(&f,"first",&AtomicBool::new(false));
        let cancelled=direct(&f,"stop",&AtomicBool::new(true));assert_eq!(cancelled.status,"cancelled");assert_eq!(list_page(&f.db,0,10).unwrap().total,1);
        let conn=connection(&f.db).unwrap();conn.execute("UPDATE jobs SET status='running' WHERE id='stop'",[]).unwrap();drop(conn);drop(state);
        let _reopened=f.state();assert_eq!(jobs(&f.db).unwrap().into_iter().find(|j|j.id=="stop").unwrap().status,"interrupted");assert_eq!(direct(&f,"rescan",&AtomicBool::new(false)).skipped,1);
    }
    #[test]
    fn future_schema_is_refused(){let f=Fixture::new();let conn=connection(&f.db).unwrap();conn.pragma_update(None,"user_version",99).unwrap();drop(conn);assert!(CatalogState::new(f.db.clone()).is_err());let conn=connection(&f.db).unwrap();assert_eq!(conn.pragma_query_value(None,"user_version",|r|r.get::<_,i64>(0)).unwrap(),99);}
    #[test]
    fn second_instance_is_refused_until_owner_drops(){let f=Fixture::new();let first=f.state();assert!(CatalogState::new(f.db.clone()).err().unwrap().contains("另一实例"));drop(first);assert!(CatalogState::new(f.db.clone()).is_ok());}
    #[test]
    fn four_formats_catalog_metadata_and_source_bytes(){
        let f=Fixture::new();let _state=f.state();let source=Path::new(env!("CARGO_MANIFEST_DIR")).join("../build/s1-samples");
        let cases=[("pcm-44100.wav",44100,"WAV"),("pcm-48000.aiff",48000,"AIFF"),("lossless-48000.flac",48000,"FLAC"),("lossy-44100.mp3",44100,"MP3"),("long-44100.wav",44100,"WAV")];
        let originals:Vec<_>=cases.iter().map(|(name,_,_)|{let bytes=fs::read(source.join(name)).unwrap();fs::write(f.audio().join(name),&bytes).unwrap();bytes}).collect();
        let j=direct(&f,"four-formats",&AtomicBool::new(false));assert_eq!(j.added,5);assert_eq!(j.failed,0);
        let page=list_page(&f.db,0,100).unwrap();
        for (i,(name,rate,format)) in cases.iter().enumerate(){
            let item=page.items.iter().find(|a|a.name==*name).unwrap();assert_eq!(item.status,"ready");assert_eq!(item.rate,Some(*rate));assert_eq!(item.channels,Some(1));assert_eq!(item.format,*format);
            let expected=if name.starts_with("long"){5.0}else{0.5};
            let duration=item.duration.unwrap_or_else(||panic!("{name} 未取得时长"));
            assert!((duration-expected).abs()<0.02,"{name} 时长 {duration} 与实测 {expected} 不符");
            assert_eq!(fs::read(source.join(name)).unwrap(),originals[i]);assert_eq!(fs::read(f.audio().join(name)).unwrap(),originals[i]);
        }
    }
    #[test]
    fn m4a_imports_with_duration_format_filter_and_unchanged_bytes(){
        let f=Fixture::new();let _state=f.state();
        let source=Path::new(env!("CARGO_MANIFEST_DIR")).join("../build/s1-samples/aac-44100.m4a");
        let bytes=fs::read(&source).unwrap();
        fs::write(f.audio().join("aac-44100.m4a"),&bytes).unwrap();
        let job=direct(&f,"m4a",&AtomicBool::new(false));
        assert_eq!(job.added,1);
        assert_eq!(job.failed,0,"M4A导入失败：{:?}",list_page(&f.db,0,100).unwrap().items.first().and_then(|item|item.error.clone()));
        let item=&list_page(&f.db,0,100).unwrap().items[0];
        assert_eq!(item.format,"M4A");assert_eq!(item.status,"ready");
        assert_eq!(item.rate,Some(44100));assert_eq!(item.channels,Some(1));
        let duration=item.duration.unwrap_or_else(||panic!("m4a 未取得时长"));
        assert!((0.45..=0.56).contains(&duration),"m4a 时长 {duration} 超出AAC编码容差");
        // 格式筛选白名单（大小写不敏感）接受 M4A，且不误伤既有格式。
        assert_eq!(search_page(&f.db,0,100,SearchFilter{format:Some("M4A"),..Default::default()}).unwrap().total,1);
        assert_eq!(search_page(&f.db,0,100,SearchFilter{format:Some("m4a"),..Default::default()}).unwrap().total,1);
        assert_eq!(search_page(&f.db,0,100,SearchFilter{format:Some("MP3"),..Default::default()}).unwrap().total,0);
        assert!(search_page(&f.db,0,100,SearchFilter{format:Some("OGG"),..Default::default()}).is_err());
        assert_eq!(fs::read(f.audio().join("aac-44100.m4a")).unwrap(),bytes);
    }
    #[test]
    fn merge_assets_folds_annotations_memberships_playlist_history_and_keeps_disk_files(){
        let f=Fixture::new();let _state=f.state();
        f.wav("a.wav");f.wav("b.wav");f.wav("c.wav");
        direct(&f,"merge",&AtomicBool::new(false));
        let items=list_page(&f.db,0,100).unwrap().items;
        let find=|name:&str|items.iter().find(|item|item.name==name).unwrap().id.clone();
        let (a,b,c)=(find("a.wav"),find("b.wav"),find("c.wav"));
        set_favorite(&f.db,&b,true).unwrap();
        set_tags(&f.db,&a,vec!["保留".into()]).unwrap();
        set_tags(&f.db,&b,vec!["保留".into(),"额外".into()]).unwrap();
        assign_library(&f.db,vec![b.clone()],"music").unwrap();
        library_tools::record_history(&f.db,&b).unwrap();
        {let conn=connection(&f.db).unwrap();
         conn.execute("INSERT INTO playlists(id,library,name,created) VALUES('pl','sound','列表','0')",[]).unwrap();
         conn.execute("INSERT INTO playlist_items(playlist_id,asset_id,added) VALUES('pl',?1,'0')",[&b]).unwrap();}
        let merged=merge_assets(&f.db,&a,&[b.clone()]).unwrap();
        assert_eq!(merged,1);
        let page=list_page(&f.db,0,100).unwrap();
        assert_eq!(page.total,2);
        let kept=page.items.iter().find(|item|item.id==a).unwrap();
        assert!(kept.favorite);
        assert_eq!(kept.tags,vec!["保留","额外"]);
        // 库归属、播放列表、试听历史都迁移到保留条
        assert_eq!(search_page(&f.db,0,100,SearchFilter{library:Some("music"),..Default::default()}).unwrap().items[0].id,a);
        let conn=connection(&f.db).unwrap();
        assert_eq!(conn.query_row::<u64,_,_>("SELECT COUNT(*) FROM playlist_items WHERE asset_id=?1",[&a],|r|r.get(0)).unwrap(),1);
        assert_eq!(conn.query_row::<u64,_,_>("SELECT COUNT(*) FROM play_history WHERE asset_id=?1",[&a],|r|r.get(0)).unwrap(),1);
        assert_eq!(conn.query_row::<u64,_,_>("SELECT COUNT(*) FROM asset_fingerprints WHERE asset_id=?1",[&b],|r|r.get(0)).unwrap(),0);
        // 磁盘源文件原样保留，未被合并动过
        assert_eq!(fs::read(f.audio().join("b.wav")).unwrap(),wav());
        assert_eq!(fs::read(f.audio().join("c.wav")).unwrap(),wav());
        // 参数校验：空列表、缺素材、保留条混入合并列表都整体拒绝
        assert!(merge_assets(&f.db,&a,&[]).is_err());
        assert!(merge_assets(&f.db,&a,&["missing".into()]).is_err());
        assert!(merge_assets(&f.db,&a,&[a.clone()]).is_err());
        assert!(merge_assets(&f.db,&a,&[c.clone(),c.clone()]).is_err());
        assert_eq!(list_page(&f.db,0,100).unwrap().total,2);
    }
    #[test]
    fn rescan_refills_missing_duration_without_changing_id(){
        let f=Fixture::new();let _state=f.state();
        let source=Path::new(env!("CARGO_MANIFEST_DIR")).join("../build/s1-samples/pcm-44100.wav");
        fs::write(f.audio().join("pcm-44100.wav"),fs::read(source).unwrap()).unwrap();
        direct(&f,"first",&AtomicBool::new(false));
        let id=list_page(&f.db,0,100).unwrap().items[0].id.clone();
        // 模拟旧版错误时长被一次性迁移清空：文件未变，但时长缺失时必须重算。
        {let conn=connection(&f.db).unwrap();conn.execute("UPDATE assets SET duration=NULL",[]).unwrap();}
        let j=direct(&f,"second",&AtomicBool::new(false));
        assert_eq!(j.updated,1);assert_eq!(j.skipped,0);assert_eq!(j.added,0);
        let item=&list_page(&f.db,0,100).unwrap().items[0];
        assert_eq!(item.id,id);
        let duration=item.duration.unwrap_or_else(||panic!("重扫后仍无时长"));
        assert!((duration-0.5).abs()<0.02,"重算时长 {duration} 与实测 0.5 不符");
    }
    #[test]
    fn historical_duration_migration_refills_and_preserves_annotations(){
        let f=Fixture::new();let state=f.state();let source=Path::new(env!("CARGO_MANIFEST_DIR")).join("../build/s1-samples/pcm-44100.wav");
        let bytes=fs::read(&source).unwrap();fs::write(f.audio().join("historical.wav"),&bytes).unwrap();direct(&f,"old-duration",&AtomicBool::new(false));
        let id=list_page(&f.db,0,100).unwrap().items[0].id.clone();set_favorite(&f.db,&id,true).unwrap();set_tags(&f.db,&id,vec!["保留标注".into()]).unwrap();
        {let conn=connection(&f.db).unwrap();conn.execute("UPDATE assets SET duration=2000",[]).unwrap();conn.execute("DELETE FROM meta WHERE key='duration_revision'",[]).unwrap();}
        drop(state);let _reopened=f.state();assert!(list_page(&f.db,0,100).unwrap().items[0].duration.is_none());
        let job=direct(&f,"duration-refill",&AtomicBool::new(false));assert_eq!(job.updated,1);assert_eq!(job.skipped,0);
        let item=list_page(&f.db,0,100).unwrap().items.remove(0);assert_eq!(item.id,id);assert!(item.favorite);assert_eq!(item.tags,vec!["保留标注"]);assert!((item.duration.unwrap()-0.5).abs()<0.02);assert_eq!(fs::read(f.audio().join("historical.wav")).unwrap(),bytes);
    }
    #[test]
    fn missing_asset_relocates_only_by_fingerprint_and_keeps_id_and_annotations(){
        let f=Fixture::new();let state=f.state();f.wav("old.wav");direct(&f,"before-relocate",&AtomicBool::new(false));
        let before=list_page(&f.db,0,100).unwrap().items.remove(0);set_favorite(&f.db,&before.id,true).unwrap();set_tags(&f.db,&before.id,vec!["雨".into()]).unwrap();
        let digest=state.with_idle(|db|prepare_fingerprint(db,&before.id)).unwrap();assert_eq!(digest.len(),64);
        let target_dir=f.root.join("new-location");fs::create_dir_all(&target_dir).unwrap();let target=target_dir.join("renamed.wav");fs::rename(f.audio().join("old.wav"),&target).unwrap();
        let report=state.with_idle(|db|check_availability(db,None)).unwrap();assert_eq!(report.checked,1);assert_eq!(report.missing,1);assert_eq!(list_page(&f.db,0,100).unwrap().items[0].status,"missing");
        let mut wrong=wav();*wrong.last_mut().unwrap()=1;let wrong_path=target_dir.join("wrong.wav");fs::write(&wrong_path,wrong).unwrap();
        assert!(state.with_idle(|db|relocate_asset(db,&before.id,wrong_path.to_str().unwrap())).unwrap_err().contains("SHA256"));
        assert_eq!(list_page(&f.db,0,100).unwrap().items[0].path,before.path);
        let new_path=state.with_idle(|db|relocate_asset(db,&before.id,target.to_str().unwrap())).unwrap();
        let after=list_page(&f.db,0,100).unwrap().items.remove(0);assert_eq!(after.id,before.id);assert_eq!(after.path,new_path);assert_eq!(after.name,"renamed.wav");assert_eq!(after.status,"ready");assert!(after.favorite);assert_eq!(after.tags,vec!["雨"]);assert_eq!(fs::read(&target).unwrap(),wav());
        let tree=folder_nodes(&f.db).unwrap();assert!(tree.iter().any(|n|n.path==path_string(&fs::canonicalize(&target_dir).unwrap()).unwrap()));assert!(!tree.iter().any(|n|n.path==path_string(&fs::canonicalize(f.audio()).unwrap()).unwrap()));
    }
    #[test]
    fn legacy_missing_asset_without_fingerprint_is_not_matched_by_name(){
        let f=Fixture::new();let _state=f.state();f.wav("same.wav");direct(&f,"legacy",&AtomicBool::new(false));let before=list_page(&f.db,0,100).unwrap().items.remove(0);
        let target_dir=f.root.join("moved");fs::create_dir_all(&target_dir).unwrap();let target=target_dir.join("same.wav");fs::rename(f.audio().join("same.wav"),&target).unwrap();
        check_availability(&f.db,None).unwrap();let error=relocate_asset(&f.db,&before.id,target.to_str().unwrap()).unwrap_err();assert!(error.contains("没有内容指纹"));
        let after=list_page(&f.db,0,100).unwrap();assert_eq!(after.total,1);assert_eq!(after.items[0].id,before.id);assert_eq!(after.items[0].path,before.path);assert_eq!(fs::read(target).unwrap(),wav());
    }
    #[test]
    fn relocation_conflict_and_changed_file_fingerprint_invalidation(){
        let f=Fixture::new();let _state=f.state();f.wav("a.wav");f.wav("b.wav");direct(&f,"two-files",&AtomicBool::new(false));
        let a=list_page(&f.db,0,100).unwrap().items.into_iter().find(|a|a.name=="a.wav").unwrap();prepare_fingerprint(&f.db,&a.id).unwrap();
        assert!(relocate_asset(&f.db,&a.id,f.audio().join("b.wav").to_str().unwrap()).unwrap_err().contains("已经登记"));
        let mut changed=wav();changed.extend([0,0]);fs::write(f.audio().join("a.wav"),changed).unwrap();assert!(prepare_fingerprint(&f.db,&a.id).unwrap_err().contains("已变化"));
        let report=check_availability(&f.db,None).unwrap();assert_eq!(report.changed,1);assert_eq!(report.available,1);direct(&f,"changed",&AtomicBool::new(false));
        let conn=connection(&f.db).unwrap();assert_eq!(conn.query_row("SELECT COUNT(*) FROM asset_fingerprints WHERE asset_id=?1",[&a.id],|r|r.get::<_,u64>(0)).unwrap(),0);
        assert_eq!(list_page(&f.db,0,100).unwrap().total,2);
    }
    #[test]
    fn temporarily_unavailable_folder_restores_without_losing_catalog(){
        let f=Fixture::new();let _state=f.state();f.wav("offline.wav");direct(&f,"offline",&AtomicBool::new(false));let before=list_page(&f.db,0,100).unwrap().items.remove(0);
        let parked=f.root.join("parked-audio");fs::rename(f.audio(),&parked).unwrap();let report=check_availability(&f.db,None).unwrap();assert_eq!(report.unavailable,1);assert_eq!(list_page(&f.db,0,100).unwrap().items[0].status,"unavailable");
        fs::rename(&parked,f.audio()).unwrap();let report=check_availability(&f.db,None).unwrap();assert_eq!(report.available,1);let after=list_page(&f.db,0,100).unwrap().items.remove(0);assert_eq!(after.id,before.id);assert_eq!(after.status,"ready");assert_eq!(fs::read(f.audio().join("offline.wav")).unwrap(),wav());
    }
    #[test]
    #[cfg(windows)]
    fn directory_junction_is_not_followed(){
        let f=Fixture::new();let _state=f.state();f.wav("local.wav");
        let target=f.root.join("outside");fs::create_dir_all(&target).unwrap();fs::write(target.join("outside.wav"),wav()).unwrap();
        let link=f.audio().join("linked");
        // 仅在本测试专用目录创建联接，不跨用户目录，不删除测试内容。
        let result=std::process::Command::new("cmd.exe").args(["/D","/C","mklink","/J"]).arg(link.to_string_lossy().replace('/',"\\")).arg(target.to_string_lossy().replace('/',"\\")).output().unwrap();
        assert!(result.status.success(),"junction creation failed: {}",String::from_utf8_lossy(&result.stderr));
        assert!(preview_root(link.to_str().unwrap()).is_err());
        let j=direct(&f,"junction",&AtomicBool::new(false));assert_eq!(j.added,1);assert_eq!(list_page(&f.db,0,100).unwrap().total,1);assert!(target.join("outside.wav").exists());
    }
    #[test]
    fn real_worker_cancel_resume_and_single_task_guard(){
        let f=Fixture::new();let state=f.state();for i in 0..2000{f.wav(&format!("cancel-{i:04}.wav"));}
        let id=state.start(path_string(&f.audio()).unwrap(),None).unwrap();
        assert!(state.start(path_string(&f.audio()).unwrap(),None).is_err());
        let deadline=std::time::Instant::now()+Duration::from_secs(30);
        loop{let j=jobs(&f.db).unwrap().into_iter().find(|j|j.id==id).unwrap();if j.processed>=32{break;}assert!(std::time::Instant::now()<deadline);std::thread::sleep(Duration::from_millis(2));}
        state.cancel(&id).unwrap();
        while state.inner.active.lock().unwrap().is_some(){assert!(std::time::Instant::now()<deadline);std::thread::sleep(Duration::from_millis(2));}
        let cancelled=jobs(&f.db).unwrap().into_iter().find(|j|j.id==id).unwrap();assert_eq!(cancelled.status,"cancelled");assert!(cancelled.processed>=32&&cancelled.processed<2000);
        let first_id=list_page(&f.db,0,1).unwrap().items[0].id.clone();
        assert_eq!(state.resume(id.clone()).unwrap(),id);
        while state.inner.active.lock().unwrap().is_some(){assert!(std::time::Instant::now()<deadline);std::thread::sleep(Duration::from_millis(2));}
        let complete=jobs(&f.db).unwrap().into_iter().find(|j|j.id==id).unwrap();assert_eq!(complete.status,"completed");assert!(complete.skipped>=cancelled.processed);assert_eq!(list_page(&f.db,0,500).unwrap().total,2000);
        let conn=connection(&f.db).unwrap();assert_eq!(conn.query_row("SELECT COUNT(*) FROM assets WHERE id=?1",[first_id],|r|r.get::<_,i64>(0)).unwrap(),1);
    }
    #[test]
    fn ten_thousand_tiny_synthetic_wavs_catalog_pressure(){
        let f=Fixture::new();let _state=f.state();let bytes=wav();for i in 0..10_000{fs::write(f.audio().join(format!("sample-{i:05}.wav")),&bytes).unwrap();}
        let started=std::time::Instant::now();let first=direct(&f,"10k",&AtomicBool::new(false));assert_eq!(first.added,10_000);assert_eq!(first.failed,0);assert_eq!(list_page(&f.db,0,100).unwrap().total,10_000);assert_eq!(list_page(&f.db,9900,100).unwrap().items.len(),100);
        let second=direct(&f,"10k-repeat",&AtomicBool::new(false));assert_eq!(second.skipped,10_000);
        eprintln!("10k synthetic 46-byte mono WAV catalog + repeat: {:?}; evidence {}. Not real sound-library throughput.",started.elapsed(),f.root.display());
    }
    #[test]
    #[ignore = "仅在用户明确授权真实音效目录后设置SW_BENCH_ROOT和SW_BENCH_DATA_DIR执行"]
    fn real_library_benchmark(){
        use std::io::Write;
        use std::time::Instant;
        // 将尚不存在的输出路径接在已核验的真实父目录下；先检查再创建，避免写入素材源。
        fn resolved_output(path:&Path)->PathBuf{
            assert!(path.is_absolute(),"基准输出必须是绝对路径");
            let mut parent=path;let mut suffix=Vec::new();
            while !parent.exists(){suffix.push(parent.file_name().expect("路径无有效父目录").to_owned());parent=parent.parent().expect("路径无父目录");}
            let mut resolved=fs::canonicalize(parent).expect("输出父目录不可访问");
            for part in suffix.into_iter().rev(){resolved.push(part);}resolved
        }
        fn snapshot(root:&Path)->Vec<(String,u64,String,String)>{
            let mut result=Vec::new();let mut dirs=vec![root.to_owned()];
            while let Some(dir)=dirs.pop(){
                for entry in fs::read_dir(&dir).unwrap_or_else(|e|panic!("真实基准目录不可完整读取 {}：{e}",dir.display())){
                    let path=entry.expect("真实目录条目不可读").path();let meta=fs::symlink_metadata(&path).expect("真实目录元数据不可读");
                    if is_reparse(&meta){continue;}if meta.is_dir(){dirs.push(path);continue;}if !meta.is_file(){continue;}
                    if let Some(format)=audio_format(&path){result.push((path_string(&fs::canonicalize(&path).unwrap()).unwrap(),meta.len(),metadata_stamp(&meta).unwrap(),format));}
                }
            }
            result.sort();result
        }
        fn run_scan(db:&Path,root:&str,id:&str)->(ImportJob,f64){
            let conn=connection(db).unwrap();conn.execute("INSERT INTO jobs(id,root,status,created,changed) VALUES(?1,?2,'running',?3,?3)",params![id,root,now()]).unwrap();
            let started=Instant::now();scan(db,ImportJob{id:id.into(),root:root.into(),status:"running".into(),..Default::default()},&AtomicBool::new(false)).unwrap();let seconds=started.elapsed().as_secs_f64();
            (jobs(db).unwrap().into_iter().find(|j|j.id==id).unwrap(),seconds)
        }
        fn p95(values:&[f64])->f64{let mut sorted=values.to_vec();sorted.sort_by(f64::total_cmp);sorted[((sorted.len() as f64*0.95).ceil() as usize).saturating_sub(1)]}
        let source=PathBuf::from(preview_root(&std::env::var("SW_BENCH_ROOT").expect("需显式设置已获授权的SW_BENCH_ROOT")).unwrap());
        let data=resolved_output(Path::new(&std::env::var("SW_BENCH_DATA_DIR").expect("需显式设置获授权的SW_BENCH_DATA_DIR")));
        assert!(!data.starts_with(&source),"验证数据目录不能位于真实素材源内部");
        fs::create_dir_all(&data).expect("无法创建基准数据根");
        let run_dir=data.join(format!("real-library-{}",Uuid::new_v4()));fs::create_dir(&run_dir).expect("无法创建独立基准目录");
        let output=std::env::var("SW_BENCH_OUTPUT").map(|p|resolved_output(Path::new(&p))).unwrap_or_else(|_|run_dir.join("report.json"));
        assert!(output.starts_with(&data)&&!output.starts_with(&source),"SW_BENCH_OUTPUT必须位于授权数据目录内且不能在素材源内");
        assert!(!output.exists(),"基准报告已存在，不覆盖旧文件");
        fs::create_dir_all(output.parent().unwrap()).expect("无法创建报告父目录");
        let db=run_dir.join("benchmark.sqlite");let _state=CatalogState::new(db.clone()).expect("独立基准库初始化失败");
        let before=snapshot(&source);assert!(!before.is_empty(),"授权目录内没有受支持音频，不生成虚假查询基准");
        let source_text=path_string(&source).unwrap();let (first,first_seconds)=run_scan(&db,&source_text,"real-first");let (repeat,repeat_seconds)=run_scan(&db,&source_text,"real-repeat");
        let mut formats:BTreeMap<String,u64>=BTreeMap::new();for (_,_,_,format) in &before{*formats.entry(format.clone()).or_default()+=1;}
        let conn=connection(&db).unwrap();let mut stmt=conn.prepare("SELECT name FROM assets ORDER BY path").unwrap();let names:Vec<String>=stmt.query_map([],|r|r.get(0)).unwrap().collect::<Result<_,_>>().unwrap();drop(stmt);
        assert!(!names.is_empty(),"没有目录记录，无法测量查询");
        let queries:Vec<String>=(0..100).map(|i|names[(i*names.len()/100).min(names.len()-1)].chars().take(128).collect()).collect();
        let unique_queries=queries.iter().collect::<std::collections::BTreeSet<_>>().len();
        let mut measurements=Vec::new();let mut round_p95=Vec::new();let mut all_ms=Vec::new();let mut expected=vec![0u64;100];let mut consistent=true;
        for round in 0..5{
            let mut round_ms=Vec::new();for (i,query) in queries.iter().enumerate(){
                let started=Instant::now();let result=search_page(&db,0,100,SearchFilter{query:Some(query),..Default::default()}).unwrap();let ms=started.elapsed().as_secs_f64()*1000.0;
                if round==0{expected[i]=result.total;}else if expected[i]!=result.total{consistent=false;}
                measurements.push(serde_json::json!({"round":round+1,"query_index":i,"query":query,"elapsed_ms":ms,"total_matches":result.total,"returned_rows":result.items.len()}));round_ms.push(ms);all_ms.push(ms);
            }round_p95.push(p95(&round_ms));
        }
        let after=snapshot(&source);let source_unchanged=before==after;
        let metadata_sha=format!("{:x}",Sha256::digest(serde_json::to_vec(&before).unwrap()));
        let report=serde_json::json!({
            "source_root":source_text,"database":path_string(&db).unwrap(),"report_path":path_string(&output).unwrap(),"timestamp_unix_ns":now(),
            "source_audio_files":before.len(),"source_audio_bytes":before.iter().map(|x|x.1).sum::<u64>(),"format_counts":formats,
            "source_metadata_unchanged":source_unchanged,"source_metadata_snapshot_sha256":metadata_sha,"source_snapshot_fields":["canonical_path","bytes","modified_unix_ns","extension_format"],
            "source_snapshot_note":"只比较文件路径/大小/修改时间，未逐文件读取内容SHA256；目录扫描只读，不写真实源文件。",
            "meets_10000_file_scale":before.len()>=10000,"first_scan_seconds":first_seconds,"first_scan":first,"repeat_scan_seconds":repeat_seconds,"repeat_scan":repeat,
            "query_count":100,"distinct_query_strings":unique_queries,"rounds":5,"query_p95_ms":p95(&all_ms),"round_p95_ms":round_p95,"query_results_consistent":consistent,
            "query_selection":"按已登记路径顺序等距选取100个真实文件名，截取前128字符；不足100条时复用实际查询。未人工创造素材。",
            "cache_conditions":"进程内复用、数据库与文件系统缓存温热；OS冷缓存未控制。扫描后执行查询，后台扫描关闭。",
            "measurements":measurements,"source_metadata_snapshot":before,
            "limitations":"不是冷缓存或后台扫描并行基准；本机盘型未在测试中识别；不足万条时不能认定万条容量验收。"
        });
        let mut file=fs::OpenOptions::new().write(true).create_new(true).open(&output).expect("无法独占创建新报告");file.write_all(&serde_json::to_vec_pretty(&report).unwrap()).unwrap();file.sync_all().unwrap();
        eprintln!("真实库基准报告：{}；首扫 {:.3}s，复扫 {:.3}s，查询P95 {:.3}ms；源元数据不变 {}",output.display(),first_seconds,repeat_seconds,p95(&all_ms),source_unchanged);
        assert!(source_unchanged,"基准期间源目录有并发变化，报告不可作为稳定验收成绩");assert!(consistent,"查询结果出现不一致，见报告原始明细");
    }
}

#[cfg(test)]
mod recent_tests{
    use super::*;
    /// 侧栏「最近添加」的排序依据是入库顺序（rowid），不是文件修改时间；
    /// 顺带验证 limit 会被钳到 1–200，不会被前端传 0 或超大值打穿。
    #[test]fn recent_items_is_insert_order_and_clamps_limit(){
        let dir=std::env::temp_dir().join(format!("sw-recent-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db=dir.join("catalog.sqlite");
        let state=CatalogState::new(db.clone()).unwrap();
        {
            let conn=connection(&db).unwrap();
            for i in 0..5{
                conn.execute("INSERT INTO assets(id,path_key,path,name,format,size,modified,status) VALUES(?1,?2,?3,?4,'WAV',100,?5,'ready')",
                    params![format!("id{i}"),format!("key{i}"),format!("C:/sounds/a{i}.wav"),format!("a{i}.wav"),"2020-01-01"]).unwrap();
            }
        }
        let three=recent_items(&db,3).unwrap();
        assert_eq!(three.len(),3);
        assert_eq!(three[0].name,"a4.wav");
        assert_eq!(three[1].name,"a3.wav");
        assert_eq!(three[2].name,"a2.wav");
        assert_eq!(recent_items(&db,0).unwrap().len(),1);
        assert!(recent_items(&db,9999).unwrap().len()<=200);
        drop(state);
        let _=std::fs::remove_dir_all(&dir);
    }
}
