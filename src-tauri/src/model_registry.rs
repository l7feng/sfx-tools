//! 本地模型登记与完整性核验。仅登记协议，不为未实现的音频语义适配器宣称可用。
use serde::{Serialize,Deserialize};
use sha2::{Digest,Sha256};
use std::{fs::{self,File,OpenOptions},io::{Read,Write},path::{Path,PathBuf,Component},sync::{Arc,Mutex,atomic::{AtomicBool,Ordering}}};
use uuid::Uuid;
fn err(e:impl std::fmt::Display)->String{e.to_string()}
#[derive(Clone,Debug,Serialize,Deserialize)]pub struct RegisteredModel{pub model_id:String,pub revision:String,pub license:String,pub directory:String,pub capability:String,pub verified_bytes:u64,pub manifest_sha256:String,pub inference_ready:bool,pub note:String}
#[derive(Clone,Debug,Serialize)]pub struct RegistryStatus{pub verifying:bool,pub processed_bytes:u64,pub total_bytes:u64,pub error:Option<String>,pub models:Vec<RegisteredModel>,pub e5_default_directory:String,pub e5_present:bool,pub audio_text_ready:bool,pub download_supported:bool}
#[derive(Clone)]pub struct ModelRegistryState{root:PathBuf,guard:Arc<AtomicBool>,progress:Arc<Mutex<(u64,u64,Option<String>)>>}
#[derive(Deserialize)]struct Manifest{model_id:String,revision:String,license:String,files:Vec<ModelFile>}
#[derive(Deserialize)]struct ModelFile{path:String,size:u64,sha256:String}
impl ModelRegistryState{
 pub fn new(root:PathBuf)->Self{Self{root,guard:Arc::new(AtomicBool::new(false)),progress:Arc::new(Mutex::new((0,0,None)))}}
 fn records(&self)->Result<Vec<RegisteredModel>,String>{let file=self.root.join("models.json");if !file.exists(){return Ok(vec![]);}reject_links(&file)?;let bytes=bounded_read(&file,128*1024)?;serde_json::from_slice(bytes.strip_prefix(&[0xef,0xbb,0xbf]).unwrap_or(&bytes)).map_err(|e|format!("模型登记文件损坏，未覆盖：{e}"))}
 // audio_text_ready 由固定版本CLAP的文件契约检查注入；download_supported 表示下载队列已注册，不表示当前有任务。
 fn status(&self,audio_text_ready:bool)->Result<RegistryStatus,String>{let e5=Path::new(env!("CARGO_MANIFEST_DIR")).join("../build/models/multilingual-e5-small");let (processed,total,error)=self.progress.lock().map_err(|_|"模型进度锁不可用")?.clone();Ok(RegistryStatus{verifying:self.guard.load(Ordering::Acquire),processed_bytes:processed,total_bytes:total,error,models:self.records()?,e5_present:e5.join("model-manifest.json").is_file(),e5_default_directory:e5.display().to_string(),audio_text_ready,download_supported:true})}
 fn verify(&self,directory:&str,capability:&str)->Result<RegisteredModel,String>{
  if !["metadata_text","audio_text"].contains(&capability){return Err("模型能力应为metadata_text或audio_text。".into());}
  self.guard.compare_exchange(false,true,Ordering::AcqRel,Ordering::Acquire).map_err(|_|"已有模型核验正在进行。")?;
  struct Guard(Arc<AtomicBool>);impl Drop for Guard{fn drop(&mut self){self.0.store(false,Ordering::Release);}}let _guard=Guard(self.guard.clone());
  let result=self.verify_inner(directory,capability);if let Err(e)=&result{if let Ok(mut p)=self.progress.lock(){p.2=Some(e.clone());}}result
 }
 fn verify_inner(&self,directory:&str,capability:&str)->Result<RegisteredModel,String>{
  let input=Path::new(directory);if !input.is_absolute(){return Err("模型目录必须是绝对路径。".into());}reject_links(input)?;let root=fs::canonicalize(input).map_err(err)?;if !root.is_dir(){return Err("模型位置必须是目录。".into());}
  let manifest_path=root.join("model-manifest.json");reject_links(&manifest_path)?;let bytes=bounded_read(&manifest_path,128*1024)?;let manifest:Manifest=serde_json::from_slice(bytes.strip_prefix(&[0xef,0xbb,0xbf]).unwrap_or(&bytes)).map_err(|e|format!("模型清单无效：{e}"))?;
  if manifest.model_id.is_empty()||manifest.model_id.len()>200||manifest.revision.len()!=40||!manifest.revision.bytes().all(|b|b.is_ascii_hexdigit())||manifest.license.trim().is_empty()||manifest.license.len()>200{return Err("清单必须包含model_id、固定40位revision与license声明。".into());}
  if manifest.files.is_empty()||manifest.files.len()>64{return Err("模型清单文件数应为1至64。".into());}let total=manifest.files.iter().try_fold(0u64,|sum,f|sum.checked_add(f.size).ok_or("模型容量溢出"))?;if total>4*1024*1024*1024{return Err("本版单模型核验上限4GiB。".into());}
  *self.progress.lock().map_err(|_|"模型进度锁不可用")?=(0,total,None);let mut names=std::collections::HashSet::new();let mut processed=0;
  for entry in &manifest.files{
   let relative=Path::new(&entry.path);if relative.is_absolute()||entry.path.contains(':')||entry.path.contains('\\')||relative.components().any(|c|!matches!(c,Component::Normal(_)))||!names.insert(entry.path.clone()){return Err("清单相对路径不安全或重复。".into());}
   if entry.size==0||entry.sha256.len()!=64||!entry.sha256.bytes().all(|b|b.is_ascii_hexdigit()){return Err("模型文件必须有非零size和完整SHA256。".into());}
   let path=root.join(relative);reject_links(&path)?;let canonical=fs::canonicalize(&path).map_err(err)?;if !canonical.starts_with(&root)||!canonical.is_file(){return Err("模型文件越出根目录或不是普通文件。".into());}
   let before=fs::metadata(&canonical).map_err(err)?;if before.len()!=entry.size{return Err(format!("模型文件长度不匹配：{}",entry.path));}let mut file=File::open(&canonical).map_err(err)?;let mut hash=Sha256::new();let mut buffer=[0u8;64*1024];let mut taken=0u64;while taken<=entry.size{let length=(entry.size+1-taken).min(buffer.len() as u64) as usize;let n=file.read(&mut buffer[..length]).map_err(err)?;if n==0{break;}taken+=n as u64;hash.update(&buffer[..n]);processed+=n as u64;if let Ok(mut p)=self.progress.lock(){p.0=processed;}}if taken!=entry.size{return Err(format!("模型文件在读取时变化：{}",entry.path));}
   let after=fs::metadata(&canonical).map_err(err)?;if after.len()!=before.len()||after.modified().map_err(err)?!=before.modified().map_err(err)?||format!("{:x}",hash.finalize())!=entry.sha256.to_ascii_lowercase(){return Err(format!("模型文件变化或SHA256不匹配：{}",entry.path));}
  }
  let note=if capability=="audio_text"{"文件完整性已核验；本版已接入固定版本CLAP的文件契约与ONNX张量自检，音频内容编码与语义检索尚未接入。"}else{"文件完整性已核验；登记不自动替换当前E5配置，模型版本变更需要重建对应语义索引。"}.into();
  let model=RegisteredModel{model_id:manifest.model_id,revision:manifest.revision,license:manifest.license,directory:root.display().to_string(),capability:capability.into(),verified_bytes:processed,manifest_sha256:format!("{:x}",Sha256::digest(&bytes)),inference_ready:false,note};
  if !self.root.is_absolute(){return Err("模型登记存储必须是绝对路径。".into());}reject_links(&self.root)?;fs::create_dir_all(&self.root).map_err(err)?;let mut records=self.records()?;records.retain(|r|r.model_id!=model.model_id||r.capability!=model.capability);if records.len()>=32{return Err("本地模型登记最多32项。".into());}records.push(model.clone());let destination=self.root.join("models.json");reject_links(&destination)?;let temp=self.root.join(format!("{}.partial",Uuid::new_v4()));let mut file=OpenOptions::new().write(true).create_new(true).open(&temp).map_err(err)?;let result=(||{file.write_all(&serde_json::to_vec_pretty(&records).map_err(err)?).map_err(err)?;file.sync_all().map_err(err)?;drop(file);fs::rename(&temp,&destination).map_err(err)})();if result.is_err(){let _=fs::remove_file(temp);}result?;Ok(model)
 }
}
fn reject_links(path:&Path)->Result<(),String>{for part in path.ancestors(){if let Ok(meta)=fs::symlink_metadata(part){#[cfg(windows)]{use std::os::windows::fs::MetadataExt;if meta.file_attributes()&0x400!=0{return Err("模型路径不允许链接或目录联接。".into());}}#[cfg(not(windows))]if meta.file_type().is_symlink(){return Err("模型路径不允许链接。".into());}}}Ok(())}
fn bounded_read(path:&Path,limit:usize)->Result<Vec<u8>,String>{let file=File::open(path).map_err(err)?;let mut data=vec![];file.take(limit as u64+1).read_to_end(&mut data).map_err(err)?;if data.len()>limit{return Err("模型清单/登记超出大小限制。".into());}Ok(data)}
#[tauri::command]pub async fn model_registry_list(registry:tauri::State<'_,ModelRegistryState>,audio_text:tauri::State<'_,crate::audio_text::AudioTextState>)->Result<RegistryStatus,String>{let registry=registry.inner().clone();let audio=audio_text.inner().clone();tauri::async_runtime::spawn_blocking(move||registry.status(audio.contract_ready())).await.map_err(err)?}
#[tauri::command]pub async fn model_registry_verify(directory:String,capability:String,registry:tauri::State<'_,ModelRegistryState>)->Result<RegisteredModel,String>{let registry=registry.inner().clone();tauri::async_runtime::spawn_blocking(move||registry.verify(&directory,&capability)).await.map_err(err)?}

#[cfg(test)]mod tests{
 use super::*;
 struct Temp(PathBuf);impl Temp{fn new()->Self{let p=std::env::temp_dir().join(format!("sw-model-test-{}",Uuid::new_v4()));fs::create_dir(&p).unwrap();Self(fs::canonicalize(p).unwrap())}}
 impl Drop for Temp{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
 fn manifest(root:&Path,path:&str,hash:&str){let json=serde_json::json!({"model_id":"test/model","revision":"a".repeat(40),"license":"test-only","files":[{"path":path,"size":4,"sha256":hash}]});fs::write(root.join("model-manifest.json"),serde_json::to_vec(&json).unwrap()).unwrap();}
 #[test]fn model_registry_verifies_bytes_persists_and_does_not_enable_unimplemented_inference(){
  let temp=Temp::new();let source=temp.0.join("model");fs::create_dir(&source).unwrap();fs::write(source.join("model.onnx"),b"test").unwrap();manifest(&source,"model.onnx",&format!("{:x}",Sha256::digest(b"test")));let state=ModelRegistryState::new(temp.0.join("registry"));let model=state.verify(&source.display().to_string(),"audio_text").unwrap();assert!(!model.inference_ready);assert_eq!(model.verified_bytes,4);assert!(model.note.contains("尚未接入"));let reloaded=ModelRegistryState::new(temp.0.join("registry"));let status=reloaded.status(false).unwrap();assert_eq!(status.models.len(),1);assert!(!status.audio_text_ready);assert!(status.download_supported);let ready=reloaded.status(true).unwrap();assert!(ready.audio_text_ready);assert_eq!(fs::read(source.join("model.onnx")).unwrap(),b"test");
 }
 #[test]fn model_registry_rejects_escape_hash_mismatch_duplicate_and_unpinned_revision(){
  let temp=Temp::new();let source=temp.0.join("model");fs::create_dir(&source).unwrap();fs::write(source.join("model.onnx"),b"test").unwrap();let state=ModelRegistryState::new(temp.0.join("registry"));manifest(&source,"../model/model.onnx",&format!("{:x}",Sha256::digest(b"test")));assert!(state.verify(&source.display().to_string(),"audio_text").is_err());manifest(&source,"model.onnx",&"0".repeat(64));assert!(state.verify(&source.display().to_string(),"audio_text").is_err());assert!(!state.guard.load(Ordering::Acquire));let mut value:serde_json::Value=serde_json::from_slice(&fs::read(source.join("model-manifest.json")).unwrap()).unwrap();value["revision"]="main".into();fs::write(source.join("model-manifest.json"),serde_json::to_vec(&value).unwrap()).unwrap();assert!(state.verify(&source.display().to_string(),"metadata_text").is_err());assert!(state.records().unwrap().is_empty());assert!(state.verify("relative","audio_text").is_err());
 }
}
