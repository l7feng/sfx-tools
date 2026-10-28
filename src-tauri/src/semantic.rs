//! 本地多语言文本语义检索桥。固定Python解释器执行编译时嵌入脚本，不读取音频内容或打开声卡。
use crate::catalog::CatalogState;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};
use std::{fs, io::{Read, Write}, path::{Path, PathBuf}, process::{Child, Command, Stdio}, sync::{Arc, Mutex, atomic::{AtomicBool,Ordering}}, time::{Duration,Instant}};
use uuid::Uuid;

const MODEL_ID:&str="intfloat/multilingual-e5-small";
const BRIDGE_SOURCE:&str=include_str!("../../ai_bridge.py");
const BUILD_TIMEOUT:Duration=Duration::from_secs(1800);
const SEARCH_TIMEOUT:Duration=Duration::from_secs(90);
const OUTPUT_LIMIT:usize=1024*1024;

#[derive(Clone,Debug,Serialize)]
pub struct SemanticStatus {
    pub status:String, pub processed:u64, pub total:u64, pub error:Option<String>,
    pub ready:bool, pub configured:bool, pub indexed:u64, pub searching:bool,
    pub job_id:Option<String>, pub revision:Option<String>, pub model:String,
}
impl Default for SemanticStatus {
    fn default()->Self{Self{status:"idle".into(),processed:0,total:0,error:None,ready:false,configured:false,indexed:0,searching:false,job_id:None,revision:None,model:MODEL_ID.into()}}
}
#[derive(Clone)]
pub struct SemanticState { inner:Arc<Inner> }
struct Inner { paths:Paths, worker:Mutex<Worker> }
struct Worker { snapshot:SemanticStatus, active:Option<Active> }
struct Active { id:String, cancel:Arc<AtomicBool>, progress:PathBuf, cancel_path:PathBuf, build:bool }
#[derive(Clone)]
struct Paths { python:PathBuf, model_dir:PathBuf, data_dir:PathBuf }
impl Paths {
    fn new(data_dir:PathBuf)->Self{let repo=Path::new(env!("CARGO_MANIFEST_DIR")).join("..");Self{
        python:PathBuf::from("D:/Ai-Files/Tools/BuildEnv/sound-workbench-ai/Scripts/python.exe"),
        model_dir:repo.join("build/models/multilingual-e5-small"),data_dir,
    }}
    fn index(&self)->PathBuf{self.data_dir.join("index.sqlite")}
    fn configured_revision(&self)->Result<String,String>{
        for (path,label) in [(&self.python,"本地Python环境"),(&self.model_dir.join("onnx/model.onnx"),"本地ONNX模型"),(&self.model_dir.join("tokenizer.json"),"本地分词器")] {
            if !path.is_file(){return Err(format!("语义检索未配置：缺少{label}（{}）。安装版不会自动携带这些外部依赖。",path.display()));}
        }
        let manifest=read_json(&self.model_dir.join("model-manifest.json"),64*1024).map_err(|e|format!("模型清单不可用：{e}"))?;
        if manifest.get("model_id").and_then(Value::as_str)!=Some(MODEL_ID){return Err("模型清单的model_id与multilingual-e5-small不一致。".into());}
        let files=manifest.get("files").and_then(Value::as_array).ok_or("模型清单缺少文件记录。")?;
        for relative in ["onnx/model.onnx","tokenizer.json"]{
            let entry=files.iter().find(|f|f.get("path").and_then(Value::as_str)==Some(relative)).ok_or_else(||format!("模型清单缺少{relative}"))?;
            let expected=entry.get("size").and_then(Value::as_u64).ok_or("模型清单缺少文件大小。")?;
            if fs::metadata(self.model_dir.join(relative)).map_err(err)?.len()!=expected||expected==0{return Err(format!("本地模型文件不完整：{relative}"));}
        }
        let revision=manifest.get("revision").and_then(Value::as_str).ok_or("模型清单缺少固定revision。")?;
        if revision.len()!=40||!revision.bytes().all(|c|c.is_ascii_hexdigit()){return Err("模型清单revision必须为固定的40位提交标识。".into());}
        Ok(revision.to_owned())
    }
}
fn err(e:impl std::fmt::Display)->String{e.to_string()}
fn read_json(path:&Path,limit:usize)->Result<Value,String>{
    let file=fs::File::open(path).map_err(err)?;let mut bytes=Vec::new();file.take(limit as u64+1).read_to_end(&mut bytes).map_err(err)?;
    if bytes.len()>limit{return Err("JSON文件超出允许大小。".into());}serde_json::from_slice(bytes.strip_prefix(&[0xef,0xbb,0xbf]).unwrap_or(&bytes)).map_err(err)
}
fn index_status(paths:&Paths,revision:&str)->Result<(bool,u64),String>{
    let path=paths.index();if !path.is_file(){return Ok((false,0));}
    let conn=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_ONLY|OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(err)?;
    conn.busy_timeout(Duration::from_millis(250)).map_err(err)?;
    let value=|key:&str|->Result<Option<String>,String>{conn.query_row("SELECT value FROM meta WHERE key=?1",[key],|r|r.get(0)).optional().map_err(err)};
    if value("revision")?.as_deref()!=Some(revision)||value("schema")?.as_deref()!=Some("1")||value("dim")?.as_deref()!=Some("384"){return Ok((false,0));}
    let count=conn.query_row("SELECT COUNT(*) FROM vectors",[],|r|r.get(0)).map_err(err)?;
    Ok((true,count))
}
impl SemanticState {
    pub fn new(data_dir:PathBuf)->Self{Self{inner:Arc::new(Inner{paths:Paths::new(data_dir),worker:Mutex::new(Worker{snapshot:SemanticStatus::default(),active:None})})}}
    fn snapshot(&self)->Result<SemanticStatus,String>{
        let (mut snapshot,progress)= {
            let guard=self.inner.worker.lock().map_err(|_|"语义任务状态锁不可用")?;
            (guard.snapshot.clone(),guard.active.as_ref().filter(|a|a.build).map(|a|a.progress.clone()))
        };
        if let Some(progress)=progress{if let Ok(value)=read_json(&progress,64*1024){
            snapshot.processed=value.get("processed").and_then(Value::as_u64).unwrap_or(snapshot.processed);
            snapshot.total=value.get("total").and_then(Value::as_u64).unwrap_or(snapshot.total).max(snapshot.processed);
        }}
        match self.inner.paths.configured_revision(){
            Ok(revision)=>{snapshot.configured=true;snapshot.revision=Some(revision.clone());match index_status(&self.inner.paths,&revision){
                Ok((ready,indexed))=>{snapshot.ready=ready;snapshot.indexed=indexed;}
                Err(error)=>{snapshot.ready=false;if snapshot.status!="building"{snapshot.error=Some(format!("语义索引无法读取：{error}"));}}
            }}
            Err(error)=>{snapshot.configured=false;snapshot.ready=false;snapshot.error=Some(error);}
        }
        Ok(snapshot)
    }
    fn begin(&self,build:bool)->Result<(String,Arc<AtomicBool>,PathBuf,PathBuf,String),String>{
        let revision=self.inner.paths.configured_revision()?;
        let mut worker=self.inner.worker.lock().map_err(|_|"语义任务状态锁不可用")?;
        if worker.active.is_some(){return Err("已有语义索引或检索任务运行，请稍后重试。".into());}
        if !build&&!index_status(&self.inner.paths,&revision)?.0{return Err("语义索引尚未就绪或模型版本已变化，请先建立索引。".into());}
        let id=Uuid::new_v4().to_string();let run=self.inner.paths.data_dir.join("jobs").join(&id);fs::create_dir_all(&run).map_err(err)?;
        let progress=run.join("progress.json");let cancel_path=run.join("cancel.flag");let cancel=Arc::new(AtomicBool::new(false));
        worker.snapshot.error=None;worker.snapshot.job_id=Some(id.clone());worker.snapshot.searching=!build;
        if build{worker.snapshot.status="building".into();worker.snapshot.processed=0;worker.snapshot.total=0;}
        worker.active=Some(Active{id:id.clone(),cancel:cancel.clone(),progress:progress.clone(),cancel_path:cancel_path.clone(),build});
        Ok((id,cancel,progress,cancel_path,revision))
    }
    fn finish(&self,id:&str,result:&Result<Value,String>,build:bool){
        if let Ok(mut worker)=self.inner.worker.lock(){
            if !worker.active.as_ref().is_some_and(|a|a.id==id){return;}
            let active=worker.active.take().unwrap();worker.snapshot.searching=false;
            if build{if let Ok(value)=read_json(&active.progress,64*1024){worker.snapshot.processed=value.get("processed").and_then(Value::as_u64).unwrap_or(worker.snapshot.processed);worker.snapshot.total=value.get("total").and_then(Value::as_u64).unwrap_or(worker.snapshot.total);}}
            match result{
                Ok(_)=>{worker.snapshot.status="completed".into();worker.snapshot.error=None;}
                Err(error)=>{worker.snapshot.status=if active.cancel.load(Ordering::Acquire){"cancelled"}else{"error"}.into();worker.snapshot.error=Some(error.clone());}
            }
        }
    }
    fn build(&self,db:PathBuf)->Result<String,String>{
        let (id,cancel,progress,cancel_path,revision)=self.begin(true)?;
        let input=json!({"op":"build","db":db,"index":self.inner.paths.index(),"model_dir":self.inner.paths.model_dir,"revision":revision,"progress_path":progress,"cancel_path":cancel_path});
        let state=self.clone();let job=id.clone();
        let spawn=std::thread::Builder::new().name("semantic-build".into()).spawn(move||{
            let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{
                let output=run_bridge(&state.inner.paths,&input,BUILD_TIMEOUT,&cancel)?;
                if !index_status(&state.inner.paths,&revision)?.0{return Err("索引程序返回成功，但索引元信息未通过版本校验。".into());}
                Ok(output)
            })).unwrap_or_else(|_|Err("语义索引线程异常退出，可重新建立索引。".into()));
            state.finish(&job,&result,true);
        });
        if let Err(error)=spawn{let result=Err(format!("无法启动语义索引线程：{error}"));self.finish(&id,&result,true);return Err(result.unwrap_err());}
        Ok(id)
    }
    fn search(&self,db:PathBuf,query:String,limit:usize)->Result<Value,String>{
        let query=query.trim();if query.is_empty(){return Err("请输入语义查询。".into());}if query.chars().count()>512{return Err("语义查询最多512个字符。".into());}
        let (id,cancel,progress,cancel_path,revision)=self.begin(false)?;
        let input=json!({"op":"search","db":db,"index":self.inner.paths.index(),"model_dir":self.inner.paths.model_dir,"revision":revision,"query":query,"top_k":limit.clamp(1,50),"progress_path":progress,"cancel_path":cancel_path});
        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{
            let result=run_bridge(&self.inner.paths,&input,SEARCH_TIMEOUT,&cancel)?;
            validate_search_result(&result,limit.clamp(1,50))?;Ok(result)
        })).unwrap_or_else(|_|Err("语义检索线程异常退出，可重试。".into()));
        self.finish(&id,&result,false);result
    }
    fn cancel(&self)->Result<(),String>{
        let worker=self.inner.worker.lock().map_err(|_|"语义任务状态锁不可用")?;
        let active=worker.active.as_ref().ok_or("当前没有可取消的语义任务。")?;
        active.cancel.store(true,Ordering::Release);
        // 文件供Python在批次边界主动回滚；父进程也会终止自己启动的子进程。
        fs::write(&active.cancel_path,b"cancel\n").map_err(|e|format!("取消已发出，但取消标记写入失败：{e}"))?;Ok(())
    }
}

fn validate_search_result(value:&Value,limit:usize)->Result<(),String>{
    let items=value.get("items").and_then(Value::as_array).ok_or("语义检索缺少结果列表。")?;
    if items.len()>limit{return Err("语义检索返回数量超过请求上限。".into());}
    for item in items{
        let id=item.get("id").and_then(Value::as_str).ok_or("语义检索返回无效素材ID。")?;
        let score=item.get("score").and_then(Value::as_f64).ok_or("语义检索返回无效相似度。")?;
        if id.is_empty()||id.len()>128||!score.is_finite()||!(-1.0..=1.0).contains(&score){return Err("语义检索结果超出约定范围。".into());}
    }Ok(())
}

struct ChildGuard(Child);
impl Drop for ChildGuard{fn drop(&mut self){if !matches!(self.0.try_wait(),Ok(Some(_))){let _=self.0.kill();let _=self.0.wait();}}}
fn read_bounded(mut reader:impl Read,limit:usize)->Result<Vec<u8>,String>{
    let mut output=Vec::new();let mut buffer=[0u8;8192];let mut exceeded=false;
    loop{let n=reader.read(&mut buffer).map_err(err)?;if n==0{break;}let take=n.min(limit.saturating_sub(output.len()));output.extend_from_slice(&buffer[..take]);if take<n{exceeded=true;}}
    if exceeded{Err("子进程输出超过允许大小。".into())}else{Ok(output)}
}
fn run_process(mut command:Command,input:&Value,timeout:Duration,cancel:&AtomicBool)->Result<Value,String>{
    if cancel.load(Ordering::Acquire){return Err("语义任务已取消。".into());}
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]{use std::os::windows::process::CommandExt;command.creation_flags(0x08000000);}
    let mut child=ChildGuard(command.spawn().map_err(|e|format!("无法启动本地语义进程：{e}"))?);
    let stdout=child.0.stdout.take().ok_or("无法读取语义进程stdout")?;let stderr=child.0.stderr.take().ok_or("无法读取语义进程stderr")?;
    let out=std::thread::spawn(move||read_bounded(stdout,OUTPUT_LIMIT));let errors=std::thread::spawn(move||read_bounded(stderr,64*1024));
    let write_result=(||{let mut stdin=child.0.stdin.take().ok_or("无法打开语义进程stdin".to_string())?;let bytes=serde_json::to_vec(input).map_err(err)?;stdin.write_all(&bytes).map_err(err)?;stdin.write_all(b"\n").map_err(err)})();
    let deadline=Instant::now()+timeout;
    let status=if let Err(e)=write_result{Err(e)}else{loop{
        if cancel.load(Ordering::Acquire){break Err("语义任务已取消；未提交的索引事务由SQLite回滚。".into());}
        if Instant::now()>=deadline{break Err(format!("语义进程超过{}秒，已停止本次任务。",timeout.as_secs()));}
        match child.0.try_wait(){Ok(Some(status))=>break Ok(status),Ok(None)=>std::thread::sleep(Duration::from_millis(50)),Err(e)=>break Err(format!("无法获取语义进程状态：{e}"))}
    }};
    if status.is_err(){let _=child.0.kill();let _=child.0.wait();}
    let output=out.join().map_err(|_|"语义输出读取线程异常")?;let stderr=errors.join().map_err(|_|"语义错误读取线程异常")?;
    let status=status?;let output=output?;let stderr=stderr?;
    if !status.success(){return Err(format!("本地语义进程退出（{}）：{}",status,String::from_utf8_lossy(&stderr).chars().take(2000).collect::<String>()));}
    let result:Value=serde_json::from_slice(&output).map_err(|e|format!("语义进程未返回单个有效JSON：{e}"))?;
    if result.get("ok").and_then(Value::as_bool)!=Some(true){return Err(result.get("error").and_then(Value::as_str).unwrap_or("语义进程返回失败。").to_owned());}
    Ok(result)
}
fn run_bridge(paths:&Paths,input:&Value,timeout:Duration,cancel:&AtomicBool)->Result<Value,String>{
    let mut command=Command::new(&paths.python);command.args(["-B","-X","utf8","-c",BRIDGE_SOURCE]).current_dir(&paths.data_dir).env("PYTHONUTF8","1").env("PYTHONIOENCODING","utf-8").env("HF_HUB_OFFLINE","1").env("TRANSFORMERS_OFFLINE","1").env("TOKENIZERS_PARALLELISM","false");
    run_process(command,input,timeout,cancel)
}

#[tauri::command]
pub async fn semantic_status(state:tauri::State<'_,SemanticState>)->Result<SemanticStatus,String>{let state=state.inner().clone();tauri::async_runtime::spawn_blocking(move||state.snapshot()).await.map_err(err)?}
#[tauri::command]
pub async fn semantic_build(catalog:tauri::State<'_,CatalogState>,state:tauri::State<'_,SemanticState>)->Result<String,String>{let db=catalog.database_path();let state=state.inner().clone();tauri::async_runtime::spawn_blocking(move||state.build(db)).await.map_err(err)?}
#[tauri::command]
pub async fn semantic_search(query:String,limit:Option<usize>,catalog:tauri::State<'_,CatalogState>,state:tauri::State<'_,SemanticState>)->Result<Value,String>{let db=catalog.database_path();let state=state.inner().clone();tauri::async_runtime::spawn_blocking(move||state.search(db,query,limit.unwrap_or(20))).await.map_err(err)?}
#[tauri::command]
pub async fn semantic_cancel(state:tauri::State<'_,SemanticState>)->Result<(),String>{let state=state.inner().clone();tauri::async_runtime::spawn_blocking(move||state.cancel()).await.map_err(err)?}

#[cfg(test)]
mod tests{
    use super::*;
    fn root()->PathBuf{let path=Path::new(env!("CARGO_MANIFEST_DIR")).join("../build/s2-validation/semantic-tests").join(Uuid::new_v4().to_string());fs::create_dir_all(&path).unwrap();path}
    #[test]
    fn missing_configuration_is_explicit_and_does_not_spawn_python(){
        let root=root();let paths=Paths{python:root.join("missing-python.exe"),model_dir:root.join("model"),data_dir:root.join("data")};
        let state=SemanticState{inner:Arc::new(Inner{paths,worker:Mutex::new(Worker{snapshot:SemanticStatus::default(),active:None})})};
        let status=state.snapshot().unwrap();assert!(!status.configured);assert!(!status.ready);assert!(status.error.unwrap().contains("未配置"));assert!(state.build(root.join("catalog.sqlite")).is_err());assert!(!root.join("data").exists());
    }
    #[test]
    fn index_revision_and_dimension_are_checked_read_only(){
        let root=root();let paths=Paths::new(root.clone());let db=Connection::open(paths.index()).unwrap();db.execute_batch("CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT);CREATE TABLE vectors(asset_id TEXT);INSERT INTO meta VALUES('schema','1'),('dim','384'),('revision','abc');INSERT INTO vectors VALUES('one');").unwrap();
        assert_eq!(index_status(&paths,"abc").unwrap(),(true,1));assert_eq!(index_status(&paths,"different").unwrap(),(false,0));db.execute("UPDATE meta SET value='768' WHERE key='dim'",[]).unwrap();assert_eq!(index_status(&paths,"abc").unwrap(),(false,0));
    }
    #[test]
    fn bounded_json_rejects_invalid_and_oversized_files(){let root=root();let file=root.join("progress.json");fs::write(&file,b"{broken").unwrap();assert!(read_json(&file,100).is_err());fs::write(&file,b"{\"ok\":true}").unwrap();assert!(read_json(&file,3).is_err());assert!(read_json(&file,100).unwrap()["ok"].as_bool().unwrap());}
    #[test]
    fn bounded_reader_drains_but_rejects_oversized_output(){assert!(read_bounded(std::io::Cursor::new(vec![1u8;100]),10).is_err());assert_eq!(read_bounded(std::io::Cursor::new(b"{}"),10).unwrap(),b"{}");}
    #[test]
    fn search_results_are_bounded_and_typed(){assert!(validate_search_result(&json!({"items":[{"id":"asset","score":0.7}]}),1).is_ok());assert!(validate_search_result(&json!({"items":[{"id":"asset","score":2}]}),1).is_err());assert!(validate_search_result(&json!({"items":[{"id":"asset","score":0.7}]}),0).is_err());assert!(validate_search_result(&json!({"ok":true}),10).is_err());}
    #[test]
    fn worker_guard_rejects_overlap_and_cancellation_releases_slot(){
        let root=root();let paths=Paths{python:root.join("configured-only-python.exe"),model_dir:root.join("model"),data_dir:root.join("data")};
        fs::create_dir_all(paths.model_dir.join("onnx")).unwrap();fs::write(&paths.python,b"test-only").unwrap();fs::write(paths.model_dir.join("onnx/model.onnx"),b"x").unwrap();fs::write(paths.model_dir.join("tokenizer.json"),b"x").unwrap();
        fs::write(paths.model_dir.join("model-manifest.json"),serde_json::to_vec(&json!({"model_id":MODEL_ID,"revision":"a".repeat(40),"files":[{"path":"onnx/model.onnx","size":1},{"path":"tokenizer.json","size":1}]})).unwrap()).unwrap();
        let state=SemanticState{inner:Arc::new(Inner{paths,worker:Mutex::new(Worker{snapshot:SemanticStatus::default(),active:None})})};
        let (id,cancel,_,flag,_)=state.begin(true).unwrap();assert!(state.begin(true).is_err());assert!(state.begin(false).is_err());state.cancel().unwrap();assert!(cancel.load(Ordering::Acquire));assert!(flag.exists());state.finish(&id,&Err("test cancellation".into()),true);assert_eq!(state.snapshot().unwrap().status,"cancelled");assert!(state.begin(true).is_ok());
    }
    #[cfg(windows)]
    fn powershell(script:&str)->Command{let mut cmd=Command::new("powershell.exe");cmd.args(["-NoLogo","-NoProfile","-NonInteractive","-Command",script]);cmd}
    #[test]
    #[cfg(windows)]
    fn child_protocol_validates_single_json_and_kills_on_timeout(){
        let cancel=AtomicBool::new(false);
        let command=powershell("$null=[Console]::In.ReadToEnd(); [Console]::Out.Write('{\"ok\":true,\"items\":[]}')");assert!(run_process(command,&json!({}),Duration::from_secs(10),&cancel).unwrap()["ok"].as_bool().unwrap());
        let command=powershell("$null=[Console]::In.ReadToEnd(); [Console]::Out.Write('not JSON')");assert!(run_process(command,&json!({}),Duration::from_secs(10),&cancel).unwrap_err().contains("JSON"));
        let command=powershell("$null=[Console]::In.ReadToEnd(); Start-Sleep -Seconds 30");let started=Instant::now();assert!(run_process(command,&json!({}),Duration::from_millis(200),&cancel).unwrap_err().contains("超过"));assert!(started.elapsed()<Duration::from_secs(5));
    }
    #[test]
    #[cfg(windows)]
    fn cancellation_stops_only_owned_child(){let cancel=Arc::new(AtomicBool::new(false));let signal=cancel.clone();let setter=std::thread::spawn(move||{std::thread::sleep(Duration::from_millis(200));signal.store(true,Ordering::Release);});let started=Instant::now();let command=powershell("$null=[Console]::In.ReadToEnd(); Start-Sleep -Seconds 30");let error=run_process(command,&json!({}),Duration::from_secs(10),&cancel).unwrap_err();setter.join().unwrap();assert!(error.contains("取消"));assert!(started.elapsed()<Duration::from_secs(5));}
    #[test]
    #[ignore = "需显式指定独立测试索引与只读源DB，调用真实本地模型"]
    fn real_embedded_bridge_chinese_search_smoke(){
        let data=PathBuf::from(std::env::var("SW_SEMANTIC_SMOKE_DATA_DIR").expect("缺少独立测试索引目录SW_SEMANTIC_SMOKE_DATA_DIR"));
        let db=PathBuf::from(std::env::var("SW_SEMANTIC_SMOKE_DB").expect("缺少只读源DB路径SW_SEMANTIC_SMOKE_DB"));
        assert!(data.is_absolute()&&data.is_dir(),"烟测数据目录必须已存在且为绝对路径");
        assert!(db.is_absolute()&&db.is_file(),"烟测源DB必须已存在且为绝对路径");
        let index=data.join("index.sqlite");assert!(index.is_file(),"请先复制已建索引为独立测试目录中的index.sqlite");
        assert_ne!(fs::canonicalize(&db).unwrap(),fs::canonicalize(&index).unwrap(),"索引不能与源DB相同");
        // 只实例化语义状态；不初始化CatalogState、不迁移或写入源目录数据库。
        let state=SemanticState::new(data);let before=state.snapshot().unwrap();assert!(before.ready&&before.configured);assert!(before.indexed>0);
        let result=state.search(db,"夜晚安静的雨声和森林环境".into(),5).unwrap();
        let items=result.get("items").and_then(Value::as_array).expect("缺少items");assert!(!items.is_empty());assert!(items.len()<=5);
        for item in items{let id=item["id"].as_str().unwrap();let score=item["score"].as_f64().unwrap();assert!(!id.is_empty()&&id.len()<=128);assert!(score.is_finite()&&(-1.0..=1.0).contains(&score));}
        let after=state.snapshot().unwrap();assert!(after.ready);assert!(!after.searching);assert_eq!(after.status,"completed");
        eprintln!("真实嵌入桥烟测：索引{}条，中文查询返回{}条；{}",before.indexed,items.len(),serde_json::to_string(&result).unwrap());
    }
}
