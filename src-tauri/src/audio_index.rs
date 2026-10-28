//! 声学特征检索基线：只读音频，不使用文本语义，不打开声卡。
use crate::{catalog::CatalogState,player::PlaybackSource};
use rodio::Source;
use rusqlite::{params,Connection,OpenFlags,OptionalExtension};
use serde::{Serialize,Deserialize};
use std::{fs,path::{Path,PathBuf},sync::{Arc,Mutex,OnceLock,atomic::{AtomicBool,Ordering}},time::{Duration,UNIX_EPOCH}};
use uuid::Uuid;
const VERSION:u32=2;
const FFT:usize=1024;
const MAX_FRAMES:u64=2_000_000;
static TWIDDLES:OnceLock<Vec<(f64,f64)>>=OnceLock::new();
fn err(e:impl std::fmt::Display)->String{e.to_string()}
fn library(value:&str)->Result<&str,String>{match value{"sound"|"music"=>Ok(value),_=>Err("请选择音效库或音乐库。".into())}}
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq)]struct Stamp{path:String,size:u64,modified:String,version:u32}
#[derive(Clone,Debug)]struct Row{id:String,path:String,size:u64,modified:String}
#[derive(Clone,Debug,Serialize,Deserialize)]struct Features{values:Vec<f64>,analyzed_seconds:f64}
#[derive(Clone,Debug,Serialize)]pub struct IndexStatus{pub job_id:Option<String>,pub status:String,pub processed:u64,pub total:u64,pub failed:u64,pub indexed:u64,pub error:Option<String>,pub version:u32}
impl Default for IndexStatus{fn default()->Self{Self{job_id:None,status:"idle".into(),processed:0,total:0,failed:0,indexed:0,error:None,version:VERSION}}}
#[derive(Clone)]pub struct AudioIndexState{root:PathBuf,inner:Arc<Mutex<Inner>>,searching:Arc<AtomicBool>}
struct Inner{snapshot:IndexStatus,cancel:Option<Arc<AtomicBool>>}
impl AudioIndexState{
 pub fn new(root:PathBuf)->Self{Self{root,inner:Arc::new(Mutex::new(Inner{snapshot:Default::default(),cancel:None})),searching:Arc::new(AtomicBool::new(false))}}
 fn db(&self)->PathBuf{self.root.join("features.sqlite")}
 fn start(&self,scope:String,catalog:CatalogState)->Result<String,String>{
  library(&scope)?;let id=Uuid::new_v4().to_string();let cancel=Arc::new(AtomicBool::new(false));
  {let mut inner=self.inner.lock().map_err(|_|"声学索引状态锁不可用")?;if inner.cancel.is_some(){return Err("声学索引正在处理，请等待或取消。".into());}inner.snapshot=IndexStatus{job_id:Some(id.clone()),status:"preparing".into(),..Default::default()};inner.cancel=Some(cancel.clone());}
  let state=self.clone();let spawned=std::thread::Builder::new().name("sound-workbench-audio-index".into()).spawn(move||{
   let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||state.build(&scope,&catalog,&cancel))).unwrap_or_else(|_|Err("声学索引线程异常，源素材未修改。".into()));
   if let Ok(mut inner)=state.inner.lock(){inner.snapshot.status=if cancel.load(Ordering::Acquire){"cancelled"}else if result.is_ok(){"completed"}else{"failed"}.into();if let Err(e)=result{if !cancel.load(Ordering::Acquire){inner.snapshot.error=Some(e);}}inner.cancel=None;}
  });
  if let Err(e)=spawned{let mut inner=self.inner.lock().map_err(|_|"声学索引状态锁不可用")?;inner.cancel=None;inner.snapshot.status="failed".into();inner.snapshot.error=Some(e.to_string());return Err(e.to_string());}Ok(id)
 }
 fn build(&self,scope:&str,catalog:&CatalogState,cancel:&AtomicBool)->Result<(),String>{
  let rows=rows(catalog,scope)?;let conn=index_connection(&self.root)?;
  {let mut inner=self.inner.lock().map_err(|_|"声学索引状态锁不可用")?;inner.snapshot.total=rows.len() as u64;inner.snapshot.status="running".into();}
  for row in rows{
   if cancel.load(Ordering::Acquire){return Err("声学索引已取消，已完成特征保留。".into());}
   let result=(||{let stamp=stamp(catalog,&row)?;if load(&conn,&row.id,&stamp)?.is_none(){let feature=extract(&PathBuf::from(&stamp.path),cancel)?;if stamp!=stamp_for_id(catalog,&row.id)?{return Err("素材在分析期间发生变化，请复扫后重建。".into());}conn.execute("INSERT INTO features(asset_id,stamp,payload) VALUES(?1,?2,?3) ON CONFLICT(asset_id) DO UPDATE SET stamp=excluded.stamp,payload=excluded.payload",params![row.id,serde_json::to_string(&stamp).map_err(err)?,serde_json::to_string(&feature).map_err(err)?]).map_err(err)?;}Ok::<_,String>(())})();
   let mut inner=self.inner.lock().map_err(|_|"声学索引状态锁不可用")?;inner.snapshot.processed+=1;if let Err(e)=result{inner.snapshot.failed+=1;inner.snapshot.error=Some(e);}else{inner.snapshot.indexed+=1;}
  }Ok(())
 }
 fn status(&self,scope:&str,catalog:&CatalogState)->Result<IndexStatus,String>{
  let mut result=self.inner.lock().map_err(|_|"声学索引状态锁不可用")?.snapshot.clone();if !self.db().is_file(){result.indexed=0;return Ok(result);}let conn=index_connection(&self.root)?;result.indexed=0;for row in rows(catalog,scope)?{if load(&conn,&row.id,&Stamp{path:row.path,size:row.size,modified:row.modified,version:VERSION})?.is_some(){result.indexed+=1;}}Ok(result)
 }
 fn cancel(&self)->Result<(),String>{let mut inner=self.inner.lock().map_err(|_|"声学索引状态锁不可用")?;let flag=inner.cancel.as_ref().ok_or("当前没有声学索引任务。")?;flag.store(true,Ordering::Release);inner.snapshot.status="cancelling".into();Ok(())}
}
fn readonly(catalog:&CatalogState)->Result<Connection,String>{let conn=Connection::open_with_flags(catalog.database_path(),OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(err)?;conn.busy_timeout(Duration::from_secs(5)).map_err(err)?;Ok(conn)}
fn rows(catalog:&CatalogState,scope:&str)->Result<Vec<Row>,String>{library(scope)?;let conn=readonly(catalog)?;let mut stmt=conn.prepare("SELECT a.id,a.path,a.size,a.modified FROM assets a JOIN asset_libraries l ON l.asset_id=a.id WHERE a.status='ready' AND l.library=?1 ORDER BY a.id LIMIT 100001").map_err(err)?;let result=stmt.query_map([scope],|r|Ok(Row{id:r.get(0)?,path:r.get(1)?,size:r.get(2)?,modified:r.get(3)?})).map_err(err)?.collect::<Result<Vec<_>,_>>().map_err(err)?;if result.len()>100000{return Err("当前声学索引单次上限十万条，专项容量验收尚未完成。".into());}Ok(result)}
fn row_for_id(catalog:&CatalogState,id:&str)->Result<Row,String>{if id.is_empty()||id.len()>128{return Err("素材ID无效。".into());}let conn=readonly(catalog)?;conn.query_row("SELECT id,path,size,modified FROM assets WHERE id=?1",[id],|r|Ok(Row{id:r.get(0)?,path:r.get(1)?,size:r.get(2)?,modified:r.get(3)?})).map_err(err)}
fn stamp(catalog:&CatalogState,row:&Row)->Result<Stamp,String>{let path=catalog.resolve_asset(&row.id)?;let meta=fs::metadata(&path).map_err(err)?;let modified=meta.modified().map_err(err)?.duration_since(UNIX_EPOCH).map_err(err)?.as_nanos().to_string();if path!=PathBuf::from(&row.path)||meta.len()!=row.size||modified!=row.modified{return Err("素材已变化，请重新扫描后建立声学索引。".into());}Ok(Stamp{path:row.path.clone(),size:row.size,modified,version:VERSION})}
fn stamp_for_id(catalog:&CatalogState,id:&str)->Result<Stamp,String>{stamp(catalog,&row_for_id(catalog,id)?)}
fn reject_links(path:&Path)->Result<(),String>{for parent in path.ancestors(){if let Ok(meta)=fs::symlink_metadata(parent){#[cfg(windows)]{use std::os::windows::fs::MetadataExt;if meta.file_attributes()&0x400!=0{return Err("索引路径不允许链接或目录联接。".into());}}#[cfg(not(windows))]if meta.file_type().is_symlink(){return Err("索引路径不允许链接。".into());}}}Ok(())}
fn index_connection(root:&Path)->Result<Connection,String>{if !root.is_absolute(){return Err("声学索引目录必须是绝对路径。".into());}reject_links(root)?;fs::create_dir_all(root).map_err(err)?;let file=root.join("features.sqlite");reject_links(&file)?;let conn=Connection::open(file).map_err(err)?;conn.busy_timeout(Duration::from_secs(5)).map_err(err)?;conn.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS features(asset_id TEXT PRIMARY KEY,stamp TEXT NOT NULL,payload TEXT NOT NULL);").map_err(err)?;Ok(conn)}
fn load(conn:&Connection,id:&str,stamp:&Stamp)->Result<Option<Features>,String>{let stored:Option<(String,String)>=conn.query_row("SELECT stamp,payload FROM features WHERE asset_id=?1 AND length(payload)<=16384 AND length(stamp)<=131072",[id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(err)?;let Some((identity,payload))=stored else{return Ok(None)};let Ok(saved)=serde_json::from_str::<Stamp>(&identity)else{return Ok(None)};if saved!=*stamp{return Ok(None);}let Ok(feature)=serde_json::from_str::<Features>(&payload)else{return Ok(None)};if feature.values.len()!=12||feature.values.iter().any(|v|!v.is_finite())||!feature.analyzed_seconds.is_finite()||feature.analyzed_seconds<=0.0{return Ok(None);}Ok(Some(feature))}
fn spectrum(values:&[f64],rate:u32,bands:&mut [f64;8],centroid:&mut f64,power:&mut f64){
 let mut real=values.to_vec();real.resize(FFT,0.0);let mut imag=vec![0.0;FFT];for (i,v) in real.iter_mut().enumerate(){*v*=0.5-0.5*(std::f64::consts::TAU*i as f64/(FFT-1) as f64).cos();}
 let mut j=0;for i in 1..FFT{let mut bit=FFT>>1;while j&bit!=0{j^=bit;bit>>=1;}j^=bit;if i<j{real.swap(i,j);}}
 let twiddles=TWIDDLES.get_or_init(||(0..FFT/2).map(|k|(-std::f64::consts::TAU*k as f64/FFT as f64).sin_cos()).collect());let mut length=2;while length<=FFT{for start in (0..FFT).step_by(length){for k in 0..length/2{let (s,c)=twiddles[k*FFT/length];let index=start+k+length/2;let r=real[index]*c-imag[index]*s;let im=real[index]*s+imag[index]*c;real[index]=real[start+k]-r;imag[index]=imag[start+k]-im;real[start+k]+=r;imag[start+k]+=im;}}length*=2;}
 for i in 1..FFT/2{let hz=i as f64*rate as f64/FFT as f64;let energy=real[i]*real[i]+imag[i]*imag[i];let band=[125.0,250.0,500.0,1000.0,2000.0,4000.0,8000.0].iter().position(|edge|hz<*edge).unwrap_or(7);bands[band]+=energy;*centroid+=hz*energy;*power+=energy;}
}
// Quantized zeros carry no sign; retain the last nonzero sample so polarity and zero plateaus do not create crossings.
fn zero_crossing(previous:&mut f64,value:f64)->bool{if value==0.0{return false;}let crossed=*previous!=0.0&&(value>0.0)!=(*previous>0.0);*previous=value;crossed}
fn extract(path:&Path,cancel:&AtomicBool)->Result<Features,String>{
 let mut source=PlaybackSource::open(path)?;let rate=source.sample_rate();let channels=source.channels();if rate==0||channels==0||channels>8{return Err("声学基线仅支持1至8声道。".into());}
 let limit=(rate as u64*30).min(MAX_FRAMES);let mut frames=0;let mut sum=0.0;let mut peak=0.0f64;let mut previous=vec![0.0;channels as usize];let mut crossings=0u64;let mut windows=vec![Vec::with_capacity(FFT);channels as usize];let mut bands=[0.0;8];let mut centroid=0.0;let mut power=0.0;
 while frames<limit{if frames%2048==0&&cancel.load(Ordering::Acquire){return Err("声学分析已取消。".into());}let Some(first)=source.next()else{break};
  for channel in 0..channels as usize{let value=if channel==0{first}else{source.next().ok_or("声道采样不完整。")?} as f64;if !value.is_finite(){return Err("声学分析遇到非有限采样。".into());}sum+=value*value;peak=peak.max(value.abs());if zero_crossing(&mut previous[channel],value){crossings+=1;}windows[channel].push(value);}
  frames+=1;if windows[0].len()==FFT{for window in &mut windows{spectrum(window,rate,&mut bands,&mut centroid,&mut power);window.clear();}}
 }
 if let Some(error)=source.decode_error(){return Err(format!("声学解码失败：{error}"));}if frames==0{return Err("音频无有效采样。".into());}for window in &windows{if !window.is_empty(){spectrum(window,rate,&mut bands,&mut centroid,&mut power);}}
 let count=frames as f64*channels as f64;let rms=(sum/count).sqrt();let mut values=vec![(1.0+rms*100.0).ln()*0.02,crossings as f64/count*rate as f64/20000.0,if power>0.0{(1.0+centroid/power).ln()/12.0}else{0.0}];let total=bands.iter().sum::<f64>();values.extend(bands.iter().map(|v|if total>0.0{(v/total).sqrt()}else{0.0}));values.push(if rms>1e-10{(peak/rms).ln().min(5.0)*0.05}else{0.0});Ok(Features{values,analyzed_seconds:frames as f64/rate as f64})
}
#[derive(Serialize)]pub struct SimilarItem{pub id:String,pub distance:f64}
#[derive(Serialize)]pub struct SearchResult{pub items:Vec<SimilarItem>,pub stale:u64,pub reference_seconds:f64,pub algorithm:String}
fn search(state:&AudioIndexState,catalog:&CatalogState,id:&str,scope:&str,limit:usize)->Result<SearchResult,String>{
 state.searching.compare_exchange(false,true,Ordering::AcqRel,Ordering::Acquire).map_err(|_|"已有声学查询进行中，请稍后重试。")?;struct Guard(Arc<AtomicBool>);impl Drop for Guard{fn drop(&mut self){self.0.store(false,Ordering::Release);}}let _guard=Guard(state.searching.clone());
 if !(1..=100).contains(&limit){return Err("检索数量限1至100。".into());}let rows=rows(catalog,scope)?;if !rows.iter().any(|row|row.id==id){return Err("参考素材不属于当前库。".into());}let conn=index_connection(&state.root)?;let reference_stamp=stamp_for_id(catalog,id)?;let reference=match load(&conn,id,&reference_stamp)?{Some(v)=>v,None=>extract(Path::new(&reference_stamp.path),&AtomicBool::new(false))?};if stamp_for_id(catalog,id)?!=reference_stamp{return Err("参考素材在查询期间变化，请重试。".into());}let mut items=vec![];let mut stale=0;
 for row in rows{if row.id==id{continue;}let current=match stamp(catalog,&row){Ok(v)=>v,Err(_)=>{stale+=1;continue;}};let Some(feature)=load(&conn,&row.id,&current)?else{stale+=1;continue};let distance=reference.values.iter().zip(&feature.values).map(|(a,b)|(a-b).powi(2)).sum::<f64>().sqrt();items.push(SimilarItem{id:row.id,distance});}
 items.sort_by(|a,b|a.distance.total_cmp(&b.distance).then(a.id.cmp(&b.id)));items.truncate(limit);Ok(SearchResult{items,stale,reference_seconds:reference.analyzed_seconds,algorithm:"acoustic-features-v2".into()})
}
#[tauri::command]pub async fn audio_index_start(library:String,catalog:tauri::State<'_,CatalogState>,audio:tauri::State<'_,AudioIndexState>)->Result<String,String>{let audio=audio.inner().clone();let catalog=catalog.inner().clone();tauri::async_runtime::spawn_blocking(move||audio.start(library,catalog)).await.map_err(err)?}
#[tauri::command]pub async fn audio_index_status(library:String,catalog:tauri::State<'_,CatalogState>,audio:tauri::State<'_,AudioIndexState>)->Result<IndexStatus,String>{let audio=audio.inner().clone();let catalog=catalog.inner().clone();tauri::async_runtime::spawn_blocking(move||audio.status(&library,&catalog)).await.map_err(err)?}
#[tauri::command]pub fn audio_index_cancel(audio:tauri::State<'_,AudioIndexState>)->Result<(),String>{audio.cancel()}
#[tauri::command]pub async fn audio_index_search(id:String,library:String,limit:usize,catalog:tauri::State<'_,CatalogState>,audio:tauri::State<'_,AudioIndexState>)->Result<SearchResult,String>{let audio=audio.inner().clone();let catalog=catalog.inner().clone();tauri::async_runtime::spawn_blocking(move||search(&audio,&catalog,&id,&library,limit)).await.map_err(err)?}

#[cfg(test)]mod tests{
 use super::*;
 use std::{thread,time::Instant};
 struct Fixture{root:PathBuf,catalog:Option<CatalogState>}
 impl Fixture{
  fn new()->Self{let root=std::env::temp_dir().join(format!("sw-acoustic-test-{}",Uuid::new_v4()));fs::create_dir(&root).unwrap();let root=fs::canonicalize(root).unwrap();let catalog=CatalogState::new(root.join("catalog.sqlite")).unwrap();Self{root,catalog:Some(catalog)}}
  fn catalog(&self)->&CatalogState{self.catalog.as_ref().unwrap()}
  fn tone(&self,hz:f64,gain:f64,scope:&str)->String{self.signal(hz,gain,scope,false)}
  fn signal(&self,hz:f64,gain:f64,scope:&str,opposite:bool)->String{let id=Uuid::new_v4().to_string();let path=self.root.join(format!("{id}.wav"));let rate=16000u32;let channels=if opposite{2u16}else{1};let frames=4096usize;let mut values=vec![];for n in 0..frames{let sample=(gain*(std::f64::consts::TAU*hz*n as f64/rate as f64).sin()*32767.0) as i16;values.extend_from_slice(&sample.to_le_bytes());if opposite{values.extend_from_slice(&(-sample).to_le_bytes());}}let bytes=values.len() as u32;let mut data=vec![];data.extend_from_slice(b"RIFF");data.extend_from_slice(&(36+bytes).to_le_bytes());data.extend_from_slice(b"WAVEfmt ");data.extend_from_slice(&16u32.to_le_bytes());data.extend_from_slice(&1u16.to_le_bytes());data.extend_from_slice(&channels.to_le_bytes());data.extend_from_slice(&rate.to_le_bytes());data.extend_from_slice(&(rate*channels as u32*2).to_le_bytes());data.extend_from_slice(&(channels*2).to_le_bytes());data.extend_from_slice(&16u16.to_le_bytes());data.extend_from_slice(b"data");data.extend_from_slice(&bytes.to_le_bytes());data.extend_from_slice(&values);fs::write(&path,&data).unwrap();let path=fs::canonicalize(path).unwrap();let meta=fs::metadata(&path).unwrap();let stamp=meta.modified().unwrap().duration_since(UNIX_EPOCH).unwrap().as_nanos().to_string();let conn=Connection::open(self.catalog().database_path()).unwrap();conn.execute("INSERT INTO assets(id,path_key,path,name,format,size,modified,rate,channels,duration,status) VALUES(?1,?2,?2,?1,'WAV',?3,?4,?5,?6,?7,'ready')",params![id,path.display().to_string(),data.len() as u64,stamp,rate,channels,frames as f64/rate as f64]).unwrap();conn.execute("INSERT INTO asset_libraries(asset_id,library) VALUES(?1,?2)",params![id,scope]).unwrap();id}
 }
 impl Drop for Fixture{fn drop(&mut self){self.catalog.take();let _=fs::remove_dir_all(&self.root);}}
 fn build(state:&AudioIndexState,f:&Fixture,scope:&str){state.build(scope,f.catalog(),&AtomicBool::new(false)).unwrap();}
 #[test]fn acoustic_tone_similarity_is_content_based_and_library_scoped(){
  let f=Fixture::new();let reference=f.tone(400.0,0.4,"sound");let near=f.tone(400.0,0.2,"sound");let far=f.tone(3000.0,0.4,"sound");let music=f.tone(400.0,0.4,"music");let state=AudioIndexState::new(f.root.join("index"));build(&state,&f,"sound");build(&state,&f,"music");let result=search(&state,f.catalog(),&reference,"sound",30).unwrap();assert_eq!(result.items[0].id,near);assert_eq!(result.items[1].id,far);assert!(result.items.iter().all(|i|i.id!=reference&&i.id!=music));assert!(result.items[0].distance<result.items[1].distance);assert!(search(&state,f.catalog(),&music,"sound",30).is_err());
  let restored=AudioIndexState::new(f.root.join("index"));assert_eq!(search(&restored,f.catalog(),&reference,"sound",30).unwrap().items[0].id,near);
 }
 #[test]fn acoustic_source_changes_and_broken_cache_are_not_reused(){
  let f=Fixture::new();let reference=f.tone(400.0,0.4,"sound");let candidate=f.tone(450.0,0.4,"sound");let path=f.catalog().resolve_asset(&candidate).unwrap();let original=fs::read(&path).unwrap();let state=AudioIndexState::new(f.root.join("index"));build(&state,&f,"sound");assert_eq!(fs::read(&path).unwrap(),original);let conn=index_connection(&state.root).unwrap();conn.execute("UPDATE features SET payload='bad' WHERE asset_id=?1",[&candidate]).unwrap();assert_eq!(search(&state,f.catalog(),&reference,"sound",30).unwrap().stale,1);build(&state,&f,"sound");assert_eq!(search(&state,f.catalog(),&reference,"sound",30).unwrap().items.len(),1);let mut changed=original;changed.extend_from_slice(&[0,0]);fs::write(path,changed).unwrap();let result=search(&state,f.catalog(),&reference,"sound",30).unwrap();assert!(result.items.is_empty());assert_eq!(result.stale,1);
 }
 #[test]fn zero_crossings_ignore_zero_plateaus_and_polarity(){
  for polarity in [1.0,-1.0]{let mut previous=0.0;let samples=[0.0,1.0,0.0,0.0,-1.0,0.0,1.0,0.0];let crossings=samples.into_iter().filter(|v|zero_crossing(&mut previous,v*polarity)).count();assert_eq!(crossings,2);let mut previous=0.0;assert_eq!([0.0,0.0,1.0,0.0,1.0].into_iter().filter(|v|zero_crossing(&mut previous,v*polarity)).count(),0);}
 }
 #[test]fn acoustic_cancellation_limits_and_opposite_stereo_do_not_erase_energy(){
  let f=Fixture::new();let mono=f.tone(400.0,0.4,"sound");let stereo=f.signal(400.0,0.4,"sound",true);let a=extract(&f.catalog().resolve_asset(&mono).unwrap(),&AtomicBool::new(false)).unwrap();let b=extract(&f.catalog().resolve_asset(&stereo).unwrap(),&AtomicBool::new(false)).unwrap();assert!(a.values.iter().zip(&b.values).all(|(a,b)|(a-b).abs()<1e-12),"mono={:?}, opposite={:?}",a.values,b.values);assert_eq!(a.values.len(),12);assert!(extract(&f.catalog().resolve_asset(&mono).unwrap(),&AtomicBool::new(true)).is_err());let state=AudioIndexState::new(f.root.join("index"));assert!(state.build("sound",f.catalog(),&AtomicBool::new(true)).is_err());assert!(search(&state,f.catalog(),&mono,"invalid",30).is_err());assert!(search(&state,f.catalog(),&mono,"sound",0).is_err());
 }
 #[test]fn acoustic_background_build_reports_completion_and_preserves_source(){
  let f=Fixture::new();let id=f.tone(200.0,0.2,"sound");let path=f.catalog().resolve_asset(&id).unwrap();let original=fs::read(&path).unwrap();let state=AudioIndexState::new(f.root.join("index"));state.start("sound".into(),f.catalog().clone()).unwrap();let start=Instant::now();loop{let status=state.status("sound",f.catalog()).unwrap();if status.status=="completed"{assert_eq!(status.indexed,1);break;}assert_ne!(status.status,"failed","{:?}",status.error);assert!(start.elapsed()<Duration::from_secs(10));thread::sleep(Duration::from_millis(10));}assert_eq!(fs::read(path).unwrap(),original);
 }
}
