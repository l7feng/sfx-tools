//! 模型下载队列：固定清单、分块断点续传、SHA256 完整性核验与原子清单落盘。
//! 网络仅经本机 curl.exe；不自动启动、不删除既有模型文件、不写素材库、不联网查询 revision。
use serde::{Serialize,Deserialize};
use sha2::{Digest,Sha256};
use std::{collections::{HashMap,HashSet},fs::{self,File,OpenOptions},io::{Read,Write},path::{Path,PathBuf,Component},process::{Command,Stdio},sync::{Arc,Mutex,atomic::{AtomicBool,Ordering}}};
use uuid::Uuid;
fn err(e:impl std::fmt::Display)->String{e.to_string()}
const CHUNK:usize=4*1024*1024;
const PROXY_DEFAULT:&str="http://127.0.0.1:7897";
const E5_ID:&str="intfloat/multilingual-e5-small";
const E5_REV:&str="614241f622f53c4eeff9890bdc4f31cfecc418b3";
const CLAP_ID:&str="Xenova/clap-htsat-unfused";
const CLAP_REV:&str="c28f2883575e590e04d3146ff0713c2448d691ba";
const CLAP_LICENSE:&str="Apache-2.0 (upstream laion/clap-htsat-unfused); ONNX conversion redistribution pending review";
const SMALL_NOTE:&str="小文件来源由固定 revision 官方 URL 保证，哈希为下载后本地实测值";

#[derive(Clone,Debug,Serialize)]pub struct DownloadFileSpec{pub path:String,pub size:u64,pub sha256:Option<String>,pub installed:bool}
#[derive(Clone,Debug,Serialize)]pub struct DownloadSpec{pub model_id:String,pub revision:String,pub license:String,pub capability:String,pub destination:String,pub files:Vec<DownloadFileSpec>,pub installed:bool,pub note:String}
#[derive(Clone,Debug,Serialize)]pub struct DownloadStatus{pub job_id:Option<String>,pub status:String,pub model_id:Option<String>,pub current_file:Option<String>,pub processed_bytes:u64,pub total_bytes:u64,pub completed_files:u64,pub total_files:u64,pub error:Option<String>}
fn idle()->DownloadStatus{DownloadStatus{job_id:None,status:"idle".into(),model_id:None,current_file:None,processed_bytes:0,total_bytes:0,completed_files:0,total_files:0,error:None}}

#[derive(Clone)]pub struct ModelDownloadState{root:PathBuf,inner:Arc<Mutex<Inner>>}
struct Inner{snapshot:DownloadStatus,cancel:Option<Arc<AtomicBool>>,destinations:HashMap<String,String>}

#[derive(Clone,Debug)]struct PresetFile{path:String,size:u64,sha256:Option<String>}
#[derive(Clone,Debug)]struct PresetModel{model_id:String,revision:String,license:String,capability:String,default_dir:PathBuf,files:Vec<PresetFile>,note:String}
#[derive(Clone,Debug)]struct ManifestFile{path:String,size:u64,sha256:String}
#[derive(Clone,Deserialize)]struct ManifestLite{model_id:String,revision:String,license:String,files:Vec<ManifestLiteFile>}
#[derive(Clone,Deserialize)]struct ManifestLiteFile{path:String,size:u64,sha256:String}
#[derive(Clone)]struct DownloadEnv{base_url:String,proxy:Option<String>,chunk_size:usize}
struct FileOutcome{size:u64,sha256:String}
struct CurlOutcome{status:u16,effective_url:String,content_range:Option<String>,body:Vec<u8>}

fn resolve_proxy()->Option<String>{match std::env::var("SOUND_WORKBENCH_PROXY"){Ok(v) if v.is_empty()=>None,Ok(v)=>Some(v),Err(_)=>Some(PROXY_DEFAULT.into())}}
fn curl_program()->PathBuf{std::env::var_os("SOUND_WORKBENCH_CURL").map(PathBuf::from).unwrap_or_else(||PathBuf::from("curl.exe"))}
fn default_dir(name:&str)->PathBuf{let p=Path::new(env!("CARGO_MANIFEST_DIR")).join("../build/models").join(name);fs::canonicalize(&p).unwrap_or(p)}
fn lower_hex(s:&str)->bool{s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))}
fn validate_revision(r:&str)->Result<(),String>{if r.len()!=40||!lower_hex(r){return Err("revision 必须为40位小写十六进制提交标识。".into());}Ok(())}
fn validate_rel_path(p:&str)->Result<(),String>{let path=Path::new(p);if p.is_empty()||p.len()>256||path.is_absolute()||p.contains(':')||p.contains('\\')||path.components().any(|c|!matches!(c,Component::Normal(_))){return Err("清单相对路径不安全。".into());}Ok(())}
fn validate_destination(dest:&str)->Result<PathBuf,String>{let p=Path::new(dest);if !p.is_absolute(){return Err("下载目标目录必须是绝对路径。".into());}reject_links(p)?;Ok(p.to_path_buf())}
fn host_of(url:&str)->Option<String>{let rest=url.split_once("://")?.1;let authority=rest.split('/').next().unwrap_or(rest);let hostport=authority.rsplit('@').next().unwrap_or(authority);let host=if hostport.starts_with('['){let end=hostport.find(']')?;hostport[..=end].to_string()}else{hostport.split(':').next().unwrap_or(hostport).to_string()};Some(host.to_ascii_lowercase())}
fn assert_source_url_allowed(url:&str)->Result<(),String>{if !url.starts_with("https://"){return Err("模型下载仅允许HTTPS来源。".into());}let host=host_of(url).ok_or("模型来源URL无效")?;if host=="huggingface.co"||host.ends_with(".huggingface.co"){Ok(())}else{Err(format!("模型来源主机不在白名单：{host}"))}}
fn assert_no_host_escape(base:&str,effective:&str)->Result<(),String>{let b=host_of(base).ok_or("下载URL无效")?;let e=host_of(effective).unwrap_or_default();if e==b{return Ok(());}if b=="huggingface.co"&&e.ends_with(".huggingface.co"){return Ok(());}Err(format!("下载被拒绝：跳转目标主机不在白名单（{e}）。"))}
fn hf_base(model_id:&str,revision:&str)->Result<String,String>{if model_id!=E5_ID&&model_id!=CLAP_ID{return Err("不支持的模型ID。".into());}if !model_id.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'/'|b'-'|b'_'|b'.')){return Err("模型ID含非法字符。".into());}validate_revision(revision)?;let url=format!("https://huggingface.co/{model_id}/resolve/{revision}/");assert_source_url_allowed(&url)?;Ok(url)}

fn reject_links(path:&Path)->Result<(),String>{for part in path.ancestors(){if let Ok(meta)=fs::symlink_metadata(part){#[cfg(windows)]{use std::os::windows::fs::MetadataExt;if meta.file_attributes()&0x400!=0{return Err("下载路径不允许链接或目录联接。".into());}}#[cfg(not(windows))]if meta.file_type().is_symlink(){return Err("下载路径不允许链接。".into());}}}Ok(())}
fn bounded_read(path:&Path,limit:usize)->Result<Vec<u8>,String>{let file=File::open(path).map_err(err)?;let mut data=vec![];file.take(limit as u64+1).read_to_end(&mut data).map_err(err)?;if data.len()>limit{return Err("模型清单超出大小限制。".into());}Ok(data)}
fn read_manifest_lite(path:&Path)->Option<ManifestLite>{let bytes=bounded_read(path,128*1024).ok()?;serde_json::from_slice::<ManifestLite>(bytes.strip_prefix(&[0xef,0xbb,0xbf]).unwrap_or(&bytes)).ok()}
fn part_path(target:&Path)->PathBuf{let mut os=target.as_os_str().to_os_string();os.push(".part");PathBuf::from(os)}
fn hash_file(path:&Path,cancel:&AtomicBool)->Result<String,String>{let mut file=File::open(path).map_err(err)?;let mut hash=Sha256::new();let mut buf=[0u8;64*1024];loop{if cancel.load(Ordering::Acquire){return Err("校验已取消。".into());}let n=file.read(&mut buf).map_err(err)?;if n==0{break;}hash.update(&buf[..n]);}Ok(format!("{:x}",hash.finalize()))}
fn measure(path:&Path,cancel:&AtomicBool)->Result<(u64,String),String>{Ok((fs::metadata(path).map_err(err)?.len(),hash_file(path,cancel)?))}

fn preset(model_id:&str)->Result<PresetModel,String>{match model_id{E5_ID=>e5_preset(),CLAP_ID=>clap_preset(),_=>Err("不支持的模型ID。".into())}}
fn e5_preset()->Result<PresetModel,String>{
 let dir=default_dir("multilingual-e5-small");
 let(files,note)=match read_manifest_lite(&dir.join("model-manifest.json")){
  Some(m)=>(m.files.into_iter().map(|e|PresetFile{path:e.path,size:e.size,sha256:Some(e.sha256.to_ascii_lowercase())}).collect::<Vec<_>>(),"本地清单已提供固定revision哈希；登记不自动替换当前E5语义索引配置，模型版本变更需重建索引。".to_string()),
  None=>(vec![],"本地model-manifest.json不存在，未安装；请先提供清单或从固定revision下载。".to_string()),
 };
 Ok(PresetModel{model_id:E5_ID.into(),revision:E5_REV.into(),license:"MIT".into(),capability:"metadata_text".into(),default_dir:dir,files,note})
}
fn clap_preset()->Result<PresetModel,String>{
 let dir=default_dir("clap-htsat-unfused");
 let files=vec![
  PresetFile{path:"onnx/audio_model_quantized.onnx".into(),size:34301667,sha256:Some("3fcff2c8824e7bcb83a983f2a49edab3b60cbcf4872ac70efee517355173bd1f".into())},
  // 官方固定 revision 的 LFS SHA256（64 位），已用本地下载文件与官方 tree API 双重核对。
  PresetFile{path:"onnx/text_model_quantized.onnx".into(),size:126603263,sha256:Some("1a3df8b197e249816e08415fd040434c44762b2eea7eb7bf8a48a0f0bf3c14e5".into())},
  PresetFile{path:"tokenizer.json".into(),size:0,sha256:None},
  PresetFile{path:"config.json".into(),size:0,sha256:None},
  PresetFile{path:"preprocessor_config.json".into(),size:0,sha256:None},
  PresetFile{path:"tokenizer_config.json".into(),size:0,sha256:None},
  PresetFile{path:"special_tokens_map.json".into(),size:0,sha256:None},
  PresetFile{path:"vocab.json".into(),size:0,sha256:None},
 ];
 let note=format!("已支持文件契约核验与ONNX张量自检，音频内容编码及语义检索仍未开放；{SMALL_NOTE}；ONNX转换再分发许可待审查。");
 Ok(PresetModel{model_id:CLAP_ID.into(),revision:CLAP_REV.into(),license:CLAP_LICENSE.into(),capability:"audio_text".into(),default_dir:dir,files,note})
}
fn expected_for(f:&PresetFile,existing:&Option<ManifestLite>)->(Option<u64>,Option<String>){if f.size>0{return(Some(f.size),f.sha256.clone());}if let Some(m)=existing{if let Some(e)=m.files.iter().find(|e|e.path==f.path){return(Some(e.size),Some(e.sha256.clone()));}}(None,None)}
fn file_installed(dest:&Path,target:&Path,exp_size:Option<u64>,exp_sha:Option<&str>,cancel:&AtomicBool)->Result<bool,String>{
 if !target.is_file(){return Ok(false);}reject_links(target)?;
 let canon=fs::canonicalize(target).map_err(err)?;let dest_canon=fs::canonicalize(dest).map_err(err)?;
 if !canon.starts_with(&dest_canon)||!canon.is_file(){return Ok(false);}
 let meta=fs::metadata(&canon).map_err(err)?;
 if let Some(s)=exp_size{if meta.len()!=s{return Ok(false);}}
 if let Some(h)=exp_sha{if hash_file(&canon,cancel)?.to_ascii_lowercase()!=h.to_ascii_lowercase(){return Ok(false);}}
 Ok(true)
}

fn last_header(headers:&str,name:&str)->Option<String>{headers.lines().rev().find_map(|line|{let(k,v)=line.split_once(':')?;if k.trim().eq_ignore_ascii_case(name){Some(v.trim().to_string())}else{None}})}
fn parse_content_range(s:&str)->Result<(u64,u64,Option<u64>),String>{let v=s.trim();let rest=v.strip_prefix("bytes ").or_else(||v.strip_prefix("BYTES ")).ok_or("Content-Range格式无效")?;let(range,total)=rest.split_once('/').ok_or("Content-Range缺少总长度")?;let(a,b)=range.split_once('-').ok_or("Content-Range缺少区间")?;let start=a.parse::<u64>().map_err(err)?;let end=b.parse::<u64>().map_err(err)?;let total=if total.trim()=="*"{None}else{Some(total.trim().parse::<u64>().map_err(err)?)};Ok((start,end,total))}
fn curl_chunk(env:&DownloadEnv,full_url:&str,start:u64,end:u64,scratch:&Path)->Result<CurlOutcome,String>{
 let id=Uuid::new_v4().to_string();let header_path=scratch.join(format!("{id}.hdr"));let body_path=scratch.join(format!("{id}.body"));
 let mut cmd=Command::new(curl_program());
 cmd.args(["-s","-S","-L","--max-redirs","5","--connect-timeout","20","--max-time","180"]);
 cmd.arg("-o").arg(&body_path).arg("-D").arg(&header_path).arg("-w").arg("%{http_code}\\n%{url_effective}");
 cmd.arg("--range").arg(format!("{start}-{end}"));
 match &env.proxy{Some(p)=>{cmd.arg("--proxy").arg(p);},None=>{cmd.arg("--noproxy").arg("*");}}
 cmd.arg(full_url).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
 #[cfg(windows)]{use std::os::windows::process::CommandExt;cmd.creation_flags(0x08000000);}
 let output=cmd.output().map_err(|e|format!("curl.exe不可用：{e}"))?;
 let ok=output.status.success();let headers=fs::read_to_string(&header_path).unwrap_or_default();let body=fs::read(&body_path).unwrap_or_default();
 let _=fs::remove_file(&header_path);let _=fs::remove_file(&body_path);
 if !ok{return Err(format!("curl分块传输失败（可能连接中断）：{}",String::from_utf8_lossy(&output.stderr).trim()));}
 let out=String::from_utf8_lossy(&output.stdout);let mut lines=out.lines();
 let status=lines.next().unwrap_or("0").trim().parse::<u16>().unwrap_or(0);let effective_url=lines.next().unwrap_or("").trim().to_string();
 Ok(CurlOutcome{status,effective_url,content_range:last_header(&headers,"content-range"),body})
}
// 单文件下载：base_url/proxy/chunk_size/cancel 均可注入；走完整 Range/206/Content-Range/哈希校验，不做测试旁路。
fn download_file(env:&DownloadEnv,rel_path:&str,target:&Path,expected_size:u64,expected_sha:Option<&str>,cancel:&AtomicBool,scratch:&Path,on_chunk:&mut dyn FnMut(u64))->Result<FileOutcome,String>{
 validate_rel_path(rel_path)?;let full_url=format!("{}{}",env.base_url,rel_path);
 if let Some(parent)=target.parent(){fs::create_dir_all(parent).map_err(err)?;reject_links(parent)?;}
 let part=part_path(target);reject_links(&part)?;
 let mut part_len=if part.is_file(){fs::metadata(&part).map_err(err)?.len()}else{0};
 let mut total=if expected_size>0{Some(expected_size)}else{None};
 if let Some(t)=total{if part_len>t{return Err("续传分片长度超过预期文件大小。".into());}}
 loop{
  if cancel.load(Ordering::Acquire){return Err("下载已取消。".into());}
  if let Some(t)=total{if part_len>=t{break;}}
  let req_start=part_len;let req_end=match total{Some(t)=>(req_start+env.chunk_size as u64).min(t)-1,None=>req_start+env.chunk_size as u64-1};
  let outcome=curl_chunk(env,&full_url,req_start,req_end,scratch)?;
  assert_no_host_escape(&full_url,&outcome.effective_url)?;
  if outcome.status!=206{return Err(format!("分块请求未返回206（收到{}），拒绝追加全量响应。",outcome.status));}
  let cr=outcome.content_range.as_deref().ok_or("缺少Content-Range，拒绝追加。")?;
  let(cr_start,cr_end,cr_total)=parse_content_range(cr)?;
  if cr_start!=req_start{return Err(format!("Content-Range起点不一致（期望{req_start}，收到{cr_start}）。"));}
  if cr_end<cr_start{return Err("Content-Range区间无效。".into());}
  if let Some(t)=cr_total{
   match total{Some(known) if known!=t=>return Err("Content-Range总长度与清单不一致。".into()),None=>total=Some(t),_=>{}}
   if part_len>t{return Err("续传分片长度超过实际文件大小。".into());}
  }
  let chunk_len=cr_end-cr_start+1;
  if outcome.body.len() as u64!=chunk_len{return Err("分块字节数与Content-Range不一致，拒绝落盘。".into());}
  let mut file=OpenOptions::new().append(true).create(true).open(&part).map_err(err)?;
  file.write_all(&outcome.body).map_err(err)?;file.sync_all().map_err(err)?;
  part_len=cr_end+1;on_chunk(chunk_len);
  if cancel.load(Ordering::Acquire){return Err("下载已取消。".into());}
 }
 let final_len=fs::metadata(&part).map_err(err)?.len();
 if let Some(t)=total{if final_len!=t{return Err("下载完成后长度校验失败，保留.part。".into());}}
 if expected_size>0&&final_len!=expected_size{return Err("下载完成后长度与清单不符，保留.part。".into());}
 let sha=hash_file(&part,cancel)?;
 if let Some(h)=expected_sha{if sha.to_ascii_lowercase()!=h.to_ascii_lowercase(){return Err("SHA256校验失败，保留.part且未落正式文件。".into());}}
 // 走到这里说明新字节已通过长度与SHA256校验；若目标存在（损坏/旧版本文件），
 // 仅允许替换下载目录内的普通文件，替换失败不影响刚校验过的.part。
 if target.exists(){reject_links(target)?;if !fs::metadata(target).map_err(err)?.is_file(){return Err("目标路径不是普通文件，拒绝覆盖。".into());}fs::remove_file(target).map_err(err)?;}
 reject_links(target)?;fs::rename(&part,target).map_err(err)?;
 Ok(FileOutcome{size:final_len,sha256:sha})
}

fn build_manifest_json(model:&PresetModel,files:&[ManifestFile])->Result<String,String>{
 validate_revision(&model.revision)?;
 if model.model_id.is_empty()||model.model_id.len()>200{return Err("model_id无效。".into());}
 if model.license.trim().is_empty()||model.license.len()>200{return Err("license声明无效。".into());}
 if files.is_empty()||files.len()>64{return Err("模型清单文件数应为1至64。".into());}
 let total=files.iter().try_fold(0u64,|a,f|a.checked_add(f.size).ok_or("模型容量溢出"))?;
 if total>4*1024*1024*1024{return Err("本版单模型上限4GiB。".into());}
 let mut seen=HashSet::new();
 for f in files{
  validate_rel_path(&f.path)?;
  if f.size==0{return Err("模型文件必须有非零size。".into());}
  if f.sha256.len()!=64||!lower_hex(&f.sha256){return Err("模型文件必须有64位小写SHA256。".into());}
  if !seen.insert(f.path.clone()){return Err("模型清单存在重复路径。".into());}
 }
 let value=serde_json::json!({"model_id":model.model_id,"revision":model.revision,"license":model.license,"files":files.iter().map(|f|serde_json::json!({"path":f.path,"size":f.size,"sha256":f.sha256})).collect::<Vec<_>>()});
 serde_json::to_string_pretty(&value).map_err(err)
}
fn write_manifest(dest:&Path,model:&PresetModel,files:&[ManifestFile])->Result<(),String>{
 let json=build_manifest_json(model,files)?;let path=dest.join("model-manifest.json");reject_links(&path)?;
 if let Some(m)=read_manifest_lite(&path){
  let same=m.model_id==model.model_id&&m.revision==model.revision&&m.license==model.license&&m.files.len()==files.len()&&files.iter().all(|f|m.files.iter().any(|e|e.path==f.path&&e.size==f.size&&e.sha256==f.sha256));
  if same{return Ok(());}
 }
 let temp=dest.join(format!("model-manifest.json.{}.partial",Uuid::new_v4()));
 let mut file=OpenOptions::new().write(true).create_new(true).open(&temp).map_err(err)?;
 let result=(||{file.write_all(json.as_bytes()).map_err(err)?;file.write_all(b"\n").map_err(err)?;file.sync_all().map_err(err)?;drop(file);fs::rename(&temp,&path).map_err(err)})();
 if result.is_err(){let _=fs::remove_file(&temp);}result
}

impl DownloadEnv{fn production(model:&PresetModel)->Result<Self,String>{Ok(Self{base_url:hf_base(&model.model_id,&model.revision)?,proxy:resolve_proxy(),chunk_size:CHUNK})}}
impl ModelDownloadState{
 pub fn new(root:PathBuf)->Self{Self{root,inner:Arc::new(Mutex::new(Inner{snapshot:idle(),cancel:None,destinations:HashMap::new()}))}}
 fn update<F:FnOnce(&mut DownloadStatus)>(&self,f:F){if let Ok(mut inner)=self.inner.lock(){f(&mut inner.snapshot);}}
 fn status(&self)->Result<DownloadStatus,String>{Ok(self.inner.lock().map_err(|_|"下载状态锁不可用")?.snapshot.clone())}
 fn cancel_job(&self)->Result<(),String>{let inner=self.inner.lock().map_err(|_|"下载状态锁不可用")?;let flag=inner.cancel.as_ref().ok_or("当前没有模型下载任务。")?;flag.store(true,Ordering::Release);Ok(())}
 fn list(&self)->Result<Vec<DownloadSpec>,String>{
  let destinations={let inner=self.inner.lock().map_err(|_|"下载状态锁不可用")?;inner.destinations.clone()};
  let mut out=vec![];
  for id in [E5_ID,CLAP_ID]{
   let model=preset(id)?;
   let dest_str=destinations.get(id).cloned().unwrap_or_else(||model.default_dir.display().to_string());
   let dest=PathBuf::from(&dest_str);let existing=read_manifest_lite(&dest.join("model-manifest.json"));
   let mut files=vec![];let mut all=true;
   for f in &model.files{
    let(es,eh)=expected_for(f,&existing);let target=dest.join(&f.path);
    let installed=if validate_rel_path(&f.path).is_ok(){file_installed(&dest,&target,es,eh.as_deref(),&AtomicBool::new(false)).unwrap_or(false)}else{false};
    if !installed{all=false;}
    files.push(DownloadFileSpec{path:f.path.clone(),size:f.size,sha256:f.sha256.clone(),installed});
   }
   out.push(DownloadSpec{model_id:model.model_id,revision:model.revision,license:model.license,capability:model.capability,destination:dest_str,files,installed:all,note:model.note});
  }
  Ok(out)
 }
 pub fn start(&self,model_id:String,destination:String)->Result<String,String>{let model=preset(&model_id)?;let env=DownloadEnv::production(&model)?;self.start_with(model,destination,env)}
 fn start_with(&self,model:PresetModel,destination:String,env:DownloadEnv)->Result<String,String>{
  validate_destination(&destination)?;validate_revision(&model.revision)?;
  if model.files.is_empty(){return Err("模型清单为空，未安装。".into());}
  let job_id=Uuid::new_v4().to_string();let cancel=Arc::new(AtomicBool::new(false));
  {
   let mut inner=self.inner.lock().map_err(|_|"下载状态锁不可用")?;
   if inner.cancel.is_some(){return Err("已有模型下载任务，请等待或取消。".into());}
   inner.snapshot=DownloadStatus{job_id:Some(job_id.clone()),status:"running".into(),model_id:Some(model.model_id.clone()),..idle()};
   inner.cancel=Some(cancel.clone());
  }
  let state=self.clone();let root=self.root.clone();
  let spawned=std::thread::Builder::new().name("sound-workbench-model-download".into()).spawn(move||{
   let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||state.run_job(&root,&model,&destination,&env,&cancel))).unwrap_or_else(|_|Err("模型下载线程异常退出。".into()));
   if let Ok(mut inner)=state.inner.lock(){
    inner.snapshot.status=if cancel.load(Ordering::Acquire){"cancelled"}else if result.is_ok(){"completed"}else{"failed"}.into();
    if let Err(e)=result{if !cancel.load(Ordering::Acquire){inner.snapshot.error=Some(e);}}
    inner.cancel=None;
   }
  });
  if let Err(e)=spawned{let mut inner=self.inner.lock().map_err(|_|"下载状态锁不可用")?;inner.cancel=None;inner.snapshot.status="failed".into();inner.snapshot.error=Some(e.to_string());return Err(e.to_string());}
  Ok(job_id)
 }
 fn run_job(&self,root:&Path,model:&PresetModel,destination:&str,env:&DownloadEnv,cancel:&AtomicBool)->Result<(),String>{
  let dest=validate_destination(destination)?;fs::create_dir_all(&dest).map_err(err)?;reject_links(&dest)?;
  {let mut inner=self.inner.lock().map_err(|_|"下载状态锁不可用")?;inner.destinations.insert(model.model_id.clone(),destination.to_string());}
  let scratch=root.join("download-scratch");fs::create_dir_all(&scratch).map_err(err)?;
  if model.files.len()>64{return Err("模型文件数超过上限。".into());}
  let known_total=model.files.iter().try_fold(0u64,|a,f|a.checked_add(f.size).ok_or("模型容量溢出"))?;
  if known_total>4*1024*1024*1024{return Err("模型总量超过4GiB。".into());}
  let existing=read_manifest_lite(&dest.join("model-manifest.json"));
  self.update(|s|{s.total_files=model.files.len() as u64;s.total_bytes=known_total;s.processed_bytes=0;s.completed_files=0;s.status="running".into();s.current_file=None;s.error=None;});
  let mut manifest_files:Vec<ManifestFile>=vec![];
  for f in &model.files{
   if cancel.load(Ordering::Acquire){return Err("下载已取消。".into());}
   validate_rel_path(&f.path)?;let target=dest.join(&f.path);
   if target.exists(){let canon=fs::canonicalize(&target).map_err(err)?;let dc=fs::canonicalize(&dest).map_err(err)?;if !canon.starts_with(&dc){return Err("目标文件越出下载目录。".into());}}
   self.update(|s|s.current_file=Some(f.path.clone()));
   let(es,eh)=expected_for(f,&existing);
   if file_installed(&dest,&target,es,eh.as_deref(),cancel)?{
    let(size,sha)=match(es,eh){(Some(sz),Some(h))=>(sz,h),_=>measure(&target,cancel)?};
    self.update(|s|{s.completed_files+=1;s.processed_bytes+=size;});
    manifest_files.push(ManifestFile{path:f.path.clone(),size,sha256:sha});continue;
   }
   let outcome=download_file(env,&f.path,&target,f.size,f.sha256.as_deref(),cancel,&scratch,&mut|d|self.update(|s|s.processed_bytes+=d))?;
   if f.size==0{self.update(|s|s.total_bytes+=outcome.size);}
   self.update(|s|s.completed_files+=1);
   manifest_files.push(ManifestFile{path:f.path.clone(),size:outcome.size,sha256:outcome.sha256});
  }
  self.update(|s|s.status="verifying".into());
  if cancel.load(Ordering::Acquire){return Err("下载已取消。".into());}
  write_manifest(&dest,model,&manifest_files)?;Ok(())
 }
}

#[tauri::command]pub async fn model_download_list(state:tauri::State<'_,ModelDownloadState>)->Result<Vec<DownloadSpec>,String>{let s=state.inner().clone();tauri::async_runtime::spawn_blocking(move||s.list()).await.map_err(err)?}
#[tauri::command]pub async fn model_download_start(model_id:String,destination:String,state:tauri::State<'_,ModelDownloadState>)->Result<String,String>{let s=state.inner().clone();tauri::async_runtime::spawn_blocking(move||s.start(model_id,destination)).await.map_err(err)?}
#[tauri::command]pub async fn model_download_status(state:tauri::State<'_,ModelDownloadState>)->Result<DownloadStatus,String>{let s=state.inner().clone();tauri::async_runtime::spawn_blocking(move||s.status()).await.map_err(err)?}
#[tauri::command]pub fn model_download_cancel(state:tauri::State<'_,ModelDownloadState>)->Result<(),String>{state.cancel_job()}

#[cfg(test)]mod tests{
 use super::*;
 use std::net::{TcpListener,TcpStream,Shutdown};
 use std::time::Duration;

 struct Temp(PathBuf);
 impl Temp{fn new()->Self{let p=std::env::temp_dir().join(format!("sw-dl-test-{}",Uuid::new_v4()));fs::create_dir_all(&p).unwrap();Self(fs::canonicalize(p).unwrap())}}
 impl Drop for Temp{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}

 struct ServerCfg{data:Vec<u8>,requests:usize,break_at:Option<usize>,break_done:bool,tamper:bool,ignore_range:bool,slow_ms:u64}
 struct TestServer{cfg:Arc<Mutex<ServerCfg>>,stop:Arc<AtomicBool>,handle:Option<std::thread::JoinHandle<()>>,base:String}
 impl TestServer{
  fn new(data:Vec<u8>)->Self{Self::with(data,|_|{})}
  fn with<F:FnOnce(&mut ServerCfg)>(data:Vec<u8>,f:F)->Self{
   let mut cfg=ServerCfg{data,requests:0,break_at:None,break_done:false,tamper:false,ignore_range:false,slow_ms:0};f(&mut cfg);
   let listener=TcpListener::bind("127.0.0.1:0").unwrap();listener.set_nonblocking(true).unwrap();
   let base=format!("http://{}/",listener.local_addr().unwrap());
   let cfg=Arc::new(Mutex::new(cfg));let stop=Arc::new(AtomicBool::new(false));
   let c=cfg.clone();let s=stop.clone();
   let handle=std::thread::spawn(move||{
    while !s.load(Ordering::Acquire){
     match listener.accept(){
      Ok((stream,_))=>{let cc=c.clone();std::thread::spawn(move||serve(stream,cc));}
      Err(e) if e.kind()==std::io::ErrorKind::WouldBlock=>std::thread::sleep(Duration::from_millis(2)),
      Err(_)=>break,
     }
    }
   });
   Self{cfg,stop,handle:Some(handle),base}
  }
  fn env(&self,chunk:usize)->DownloadEnv{DownloadEnv{base_url:self.base.clone(),proxy:None,chunk_size:chunk}}
  fn requests(&self)->usize{self.cfg.lock().unwrap().requests}
 }
 impl Drop for TestServer{fn drop(&mut self){self.stop.store(true,Ordering::Release);if let Some(h)=self.handle.take(){let _=h.join();}}}

 fn serve(mut stream:TcpStream,cfg:Arc<Mutex<ServerCfg>>){
  stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
  let req=read_head(&mut stream);let range=parse_range(&req);
  let(data,will_break,tamper,ignore,slow)={
   let mut c=cfg.lock().unwrap();let idx=c.requests;c.requests+=1;
   let br=c.break_at==Some(idx)&&!c.break_done;if br{c.break_done=true;}
   (c.data.clone(),br,c.tamper,c.ignore_range,c.slow_ms)
  };
  if slow>0{std::thread::sleep(Duration::from_millis(slow));}
  let(status,body,cr)=if ignore{
   ("200 OK".to_string(),data.clone(),None)
  }else{
   let(s,e)=range.unwrap_or((0,data.len().saturating_sub(1) as u64));
   let s=s.min(data.len() as u64);let e=e.min(data.len().saturating_sub(1) as u64);
   if s>e{("416 Range Not Satisfiable".to_string(),Vec::new(),None)}else{
    let mut chunk=data[s as usize..=e as usize].to_vec();
    if tamper&&!chunk.is_empty(){chunk[0]^=0xFF;}
    ("206 Partial Content".to_string(),chunk,Some(format!("bytes {}-{}/{}",s,e,data.len())))
   }
  };
  let mut head=format!("HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n",status,body.len());
  if let Some(cr)=cr{head.push_str(&format!("Content-Range: {}\r\n",cr));}
  head.push_str("\r\n");let _=stream.write_all(head.as_bytes());
  if will_break{let half=body.len()/2;let _=stream.write_all(&body[..half]);let _=stream.flush();let _=stream.shutdown(Shutdown::Both);return;}
  let _=stream.write_all(&body);let _=stream.flush();
 }
 fn read_head(stream:&mut TcpStream)->String{let mut buf=vec![];let mut tmp=[0u8;1024];loop{match stream.read(&mut tmp){Ok(0)|Err(_)=>break,Ok(n)=>{buf.extend_from_slice(&tmp[..n]);if buf.windows(4).any(|w|w==b"\r\n\r\n"){break;}}}}String::from_utf8_lossy(&buf).into_owned()}
 fn parse_range(req:&str)->Option<(u64,u64)>{for line in req.lines(){let l=line.to_ascii_lowercase();if let Some(v)=l.strip_prefix("range:"){let spec=v.trim().strip_prefix("bytes=")?;let(a,b)=spec.split_once('-')?;return Some((a.parse().ok()?,b.parse().ok()?));}}None}

 #[test]fn single_file_full_download_is_verified(){
  let temp=Temp::new();let data:Vec<u8>=(0..40u8).collect();
  let server=TestServer::new(data.clone());let env=server.env(16);
  let target=temp.0.join("sub/model.bin");let scratch=temp.0.join("scratch");fs::create_dir_all(&scratch).unwrap();
  let sha=format!("{:x}",Sha256::digest(&data));let mut progress=0u64;
  let outcome=download_file(&env,"sub/model.bin",&target,data.len() as u64,Some(&sha),&AtomicBool::new(false),&scratch,&mut|d|progress+=d).unwrap();
  assert_eq!(outcome.size,40);assert_eq!(outcome.sha256,sha);
  assert_eq!(fs::read(&target).unwrap(),data);assert!(!part_path(&target).exists());
  assert_eq!(progress,40);assert!(server.requests()>=3);
 }
 #[test]fn interrupt_then_resume_keeps_content(){
  let temp=Temp::new();let data:Vec<u8>=(0..40u8).collect();
  let server=TestServer::with(data.clone(),|c|c.break_at=Some(1));let env=server.env(16);
  let target=temp.0.join("model.bin");let part=part_path(&target);let scratch=temp.0.join("scratch");fs::create_dir_all(&scratch).unwrap();
  let sha=format!("{:x}",Sha256::digest(&data));let cancel=AtomicBool::new(false);
  let first=download_file(&env,"model.bin",&target,40,Some(&sha),&cancel,&scratch,&mut|_|{});
  assert!(first.is_err());assert!(!target.exists());
  assert_eq!(fs::metadata(&part).unwrap().len(),16);
  let outcome=download_file(&env,"model.bin",&target,40,Some(&sha),&cancel,&scratch,&mut|_|{}).unwrap();
  assert_eq!(outcome.sha256,sha);assert_eq!(fs::read(&target).unwrap(),data);assert!(!part.exists());
 }
 #[test]fn corrupt_existing_file_is_replaced_only_after_verified_download(){
  let temp=Temp::new();let data:Vec<u8>=(0..40u8).collect();
  let server=TestServer::new(data.clone());let env=server.env(16);
  let target=temp.0.join("model.bin");fs::write(&target,b"BROKEN").unwrap();
  let scratch=temp.0.join("scratch");fs::create_dir_all(&scratch).unwrap();
  let sha=format!("{:x}",Sha256::digest(&data));
  let outcome=download_file(&env,"model.bin",&target,40,Some(&sha),&AtomicBool::new(false),&scratch,&mut|_|{}).unwrap();
  assert_eq!(outcome.sha256,sha);assert_eq!(fs::read(&target).unwrap(),data);assert!(!part_path(&target).exists());
 }
 #[test]fn sha256_mismatch_rejects_and_keeps_part(){
  let temp=Temp::new();let data:Vec<u8>=(0..40u8).collect();
  let server=TestServer::with(data.clone(),|c|c.tamper=true);let env=server.env(16);
  let target=temp.0.join("model.bin");let scratch=temp.0.join("scratch");fs::create_dir_all(&scratch).unwrap();
  let sha=format!("{:x}",Sha256::digest(&data));
  let result=download_file(&env,"model.bin",&target,40,Some(&sha),&AtomicBool::new(false),&scratch,&mut|_|{});
  assert!(result.is_err());assert!(!target.exists());assert!(part_path(&target).exists());
 }
 #[test]fn http_200_is_not_appended_to_existing_part(){
  let temp=Temp::new();let data:Vec<u8>=(0..40u8).collect();
  let server=TestServer::with(data.clone(),|c|c.ignore_range=true);let env=server.env(16);
  let target=temp.0.join("model.bin");let part=part_path(&target);
  fs::write(&part,b"AAAAAAAAAAAAAAAA").unwrap();
  let scratch=temp.0.join("scratch");fs::create_dir_all(&scratch).unwrap();
  let result=download_file(&env,"model.bin",&target,40,None,&AtomicBool::new(false),&scratch,&mut|_|{});
  assert!(result.is_err());assert_eq!(fs::read(&part).unwrap(),b"AAAAAAAAAAAAAAAA".to_vec());assert!(!target.exists());
 }
 #[test]fn cancel_takes_effect_at_chunk_boundary(){
  let temp=Temp::new();let data:Vec<u8>=(0..64u8).collect();
  let server=TestServer::new(data.clone());let env=server.env(16);
  let target=temp.0.join("model.bin");let scratch=temp.0.join("scratch");fs::create_dir_all(&scratch).unwrap();
  let cancel=AtomicBool::new(false);
  let result=download_file(&env,"model.bin",&target,64,None,&cancel,&scratch,&mut|_|cancel.store(true,Ordering::Release));
  assert!(result.is_err());assert!(!target.exists());
  assert_eq!(fs::metadata(&part_path(&target)).unwrap().len(),16);
 }
 #[test]fn illegal_revision_path_relative_dir_and_escape_host_are_rejected(){
  assert!(validate_revision("main").is_err());
  assert!(validate_revision(&"A".repeat(40)).is_err());
  assert!(validate_revision(&"a".repeat(39)).is_err());
  assert!(validate_revision(&"a".repeat(40)).is_ok());
  for p in ["../x","/abs","a\\b","C:x","a/../b","","a/..","."]{assert!(validate_rel_path(p).is_err(),"path {p}");}
  assert!(validate_rel_path("onnx/model.onnx").is_ok());
  assert!(validate_destination("relative/dir").is_err());
  let temp=Temp::new();assert!(validate_destination(&temp.0.display().to_string()).is_ok());
  assert!(assert_source_url_allowed("https://evil.com/x").is_err());
  assert!(assert_source_url_allowed("https://huggingface.co.evil.com/x").is_err());
  assert!(assert_source_url_allowed("http://huggingface.co/x").is_err());
  assert!(assert_source_url_allowed("https://huggingface.co/Xenova/x/resolve/abc/f").is_ok());
  assert!(assert_source_url_allowed("https://cdn-lfs.huggingface.co/x").is_ok());
  assert!(hf_base(CLAP_ID,"not-a-revision").is_err());
  assert!(hf_base("unknown/model",&"a".repeat(40)).is_err());
  assert_eq!(hf_base(CLAP_ID,CLAP_REV).unwrap(),format!("https://huggingface.co/{CLAP_ID}/resolve/{CLAP_REV}/"));
  assert!(assert_no_host_escape("https://huggingface.co/x","https://evil.com/x").is_err());
  assert!(assert_no_host_escape("https://huggingface.co/x","https://cdn-lfs.huggingface.co/x").is_ok());
 }
 #[test]fn concurrent_start_is_mutex(){
  let temp=Temp::new();let data:Vec<u8>=(0..48u8).collect();
  let server=TestServer::with(data.clone(),|c|c.slow_ms=120);let env=server.env(16);
  let model=PresetModel{model_id:"test/local".into(),revision:"a".repeat(40),license:"test-only".into(),capability:"metadata_text".into(),default_dir:temp.0.clone(),files:vec![PresetFile{path:"a.bin".into(),size:0,sha256:None},PresetFile{path:"b.bin".into(),size:0,sha256:None}],note:"test".into()};
  let dest=temp.0.join("dest");fs::create_dir_all(&dest).unwrap();
  let state=ModelDownloadState::new(temp.0.join("state"));
  let first=state.start_with(model.clone(),dest.display().to_string(),env.clone());
  assert!(first.is_ok(),"{first:?}");
  let second=state.start_with(model.clone(),dest.display().to_string(),env.clone());
  assert!(second.is_err());
  state.cancel_job().unwrap();
  for _ in 0..300{let st=state.status().unwrap();if st.status!="running"&&st.status!="verifying"{break;}std::thread::sleep(Duration::from_millis(20));}
 }
 #[derive(Deserialize)]struct RegistryManifest{model_id:String,revision:String,license:String,files:Vec<RegistryFile>}
 #[derive(Deserialize)]struct RegistryFile{path:String,size:u64,sha256:String}
 #[test]fn generated_manifest_parses_as_registry_manifest(){
  let model=clap_preset().unwrap();
  let files=model.files.iter().map(|f|ManifestFile{path:f.path.clone(),size:if f.size>0{f.size}else{4096},sha256:f.sha256.clone().unwrap_or_else(||"a".repeat(64))}).collect::<Vec<_>>();
  let json=build_manifest_json(&model,&files).unwrap();
  let m:RegistryManifest=serde_json::from_str(&json).unwrap();
  assert_eq!(m.model_id,CLAP_ID);assert_eq!(m.revision.len(),40);assert!(m.revision.bytes().all(|b|b.is_ascii_hexdigit()));
  assert!(!m.license.trim().is_empty());assert!(m.license.len()<=200);
  assert!((1..=64).contains(&m.files.len()));
  assert!(m.files.iter().map(|f|f.size).sum::<u64>()<=4*1024*1024*1024);
  for f in &m.files{assert!(f.size>0);assert_eq!(f.sha256.len(),64);assert!(f.sha256.bytes().all(|b|b.is_ascii_hexdigit()));assert!(!f.path.contains(".."));assert!(!f.path.contains('\\'));assert!(!f.path.contains(':'));}
  let temp=Temp::new();let dest=temp.0.join("model");fs::create_dir_all(&dest).unwrap();
  write_manifest(&dest,&model,&files).unwrap();
  let path=dest.join("model-manifest.json");assert!(path.is_file());
  assert!(read_manifest_lite(&path).is_some());
  let on_disk:RegistryManifest=serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
  assert_eq!(on_disk.files.len(),m.files.len());
  let before=fs::metadata(&path).unwrap().modified().unwrap();
  write_manifest(&dest,&model,&files).unwrap();
  assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(),before);
  assert!(!fs::read_dir(&dest).unwrap().any(|e|e.unwrap().file_name().to_string_lossy().ends_with(".partial")));
 }
}

