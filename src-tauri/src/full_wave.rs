//! 选中素材完整峰值：流式读取到EOF，内存随显示点数有界，不打开声卡。
use crate::{catalog::CatalogState,player::{PlaybackSource,Waveform}};
use rodio::Source;
use serde::{Serialize,Deserialize};
use sha2::{Digest,Sha256};
use std::{fs::{self,File},io::Read,path::{Path,PathBuf},sync::{Arc,Mutex,atomic::{AtomicBool,AtomicUsize,Ordering}},time::{Duration,UNIX_EPOCH,Instant}};
use uuid::Uuid;
const MAX_POINTS:usize=8192;
const CACHE_BYTES:u64=8*1024*1024;
const CACHE_RECORD:u64=256*1024;
const VERSION:u32=2;
fn err(e:impl std::fmt::Display)->String{e.to_string()}
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq)]
struct Identity{path:PathBuf,size:u64,modified:String,version:u32}
fn identity(catalog:&CatalogState,id:&str)->Result<Identity,String>{
 let path=catalog.resolve_asset(id)?;let metadata=fs::symlink_metadata(&path).map_err(err)?;
 let modified=metadata.modified().map_err(err)?.duration_since(UNIX_EPOCH).map_err(err)?.as_nanos().to_string();
 let conn=rusqlite::Connection::open_with_flags(catalog.database_path(),rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(err)?;
 let (size,stamp):(u64,String)=conn.query_row("SELECT size,modified FROM assets WHERE id=?1",[id],|row|Ok((row.get(0)?,row.get(1)?))).map_err(err)?;
 if size!=metadata.len()||stamp!=modified{return Err("素材已变化，请重新扫描后分析完整波形。".into());}
 Ok(Identity{path,size,modified,version:VERSION})
}
#[derive(Clone,Debug,Serialize)]
pub struct FullSnapshot{pub job_id:String,pub asset_id:String,pub status:String,pub progress:Option<f64>,pub analyzed_seconds:f64,pub error:Option<String>,pub wave:Option<Waveform>}
struct Job{snapshot:FullSnapshot,cancel:Arc<AtomicBool>,identity:Identity,catalog:CatalogState}
#[derive(Clone)]
pub struct FullWaveState{root:PathBuf,jobs:Arc<Mutex<Vec<Job>>>,active:Arc<AtomicUsize>}
impl FullWaveState{
 pub fn new(root:PathBuf)->Self{Self{root,jobs:Arc::new(Mutex::new(Vec::new())),active:Arc::new(AtomicUsize::new(0))}}
 fn start(&self,id:String,catalog:CatalogState)->Result<String,String>{
  let source=identity(&catalog,&id)?;let job_id=Uuid::new_v4().to_string();let cancel=Arc::new(AtomicBool::new(false));
  {let mut jobs=self.jobs.lock().map_err(|_|"完整波形状态锁不可用")?;for job in jobs.iter(){if ["running","preparing"].contains(&job.snapshot.status.as_str()){job.cancel.store(true,Ordering::Release);}}
   if self.active.load(Ordering::Acquire)>=2{return Err("上一次波形读取正在取消，请稍后重试。".into());}
   while jobs.len()>=32{if let Some(index)=jobs.iter().position(|job|!["running","preparing"].contains(&job.snapshot.status.as_str())){jobs.remove(index);}else{return Err("完整波形任务繁忙。".into());}}
   self.active.fetch_add(1,Ordering::AcqRel);
   jobs.push(Job{snapshot:FullSnapshot{job_id:job_id.clone(),asset_id:id.clone(),status:"running".into(),progress:Some(0.0),analyzed_seconds:0.0,error:None,wave:None},cancel:cancel.clone(),identity:source.clone(),catalog:catalog.clone()});}
  let state=self.clone();let token=job_id.clone();
  let spawned=std::thread::Builder::new().name("sound-workbench-full-wave".into()).spawn(move||{
   struct ActiveGuard(Arc<AtomicUsize>);impl Drop for ActiveGuard{fn drop(&mut self){self.0.fetch_sub(1,Ordering::AcqRel);}}
   let _active=ActiveGuard(state.active.clone());
   let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{
    let wave=match read_cache(&state.root,&source){Some(wave)=>wave,None=>{
     let mut reader=PlaybackSource::open(&source.path)?;
     let wave=analyze(&mut reader,&cancel,|seconds,progress|{if let Ok(mut jobs)=state.jobs.lock(){if let Some(job)=jobs.iter_mut().find(|j|j.snapshot.job_id==token){job.snapshot.analyzed_seconds=seconds;job.snapshot.progress=progress;}}})?;
     if identity(&catalog,&id)?!=source{return Err("分析过程中素材已变化，请重新扫描。".into());}
     write_cache(&state.root,&source,&wave);wave
    }};
    if cancel.load(Ordering::Acquire){return Err("完整波形分析已取消。".into());}
    if identity(&catalog,&id)?!=source{return Err("素材在波形分析后发生变化，请重新扫描。".into());}Ok(wave)
   })).unwrap_or_else(|_|Err("完整波形线程异常退出，请重试。".into()));
   if let Ok(mut jobs)=state.jobs.lock(){if let Some(job)=jobs.iter_mut().find(|j|j.snapshot.job_id==token){
    if cancel.load(Ordering::Acquire){job.snapshot.status="cancelled".into();job.snapshot.wave=None;}
    else{match result{Ok(wave)=>{job.snapshot.analyzed_seconds=wave.analyzed_duration;job.snapshot.progress=Some(1.0);job.snapshot.status="completed".into();job.snapshot.wave=Some(wave);},Err(error)=>{job.snapshot.status="failed".into();job.snapshot.error=Some(error);}}}
   }}
  });
  if let Err(error)=spawned{self.active.fetch_sub(1,Ordering::AcqRel);if let Ok(mut jobs)=self.jobs.lock(){if let Some(job)=jobs.iter_mut().find(|j|j.snapshot.job_id==job_id){job.snapshot.status="failed".into();job.snapshot.error=Some(error.to_string());}}return Err(error.to_string());}
  Ok(job_id)
 }
 fn status(&self,id:&str)->Result<FullSnapshot,String>{self.jobs.lock().map_err(|_|"完整波形状态锁不可用")?.iter().find(|job|job.snapshot.job_id==id).map(|job|job.snapshot.clone()).ok_or("完整波形任务已过期，请重新选择素材。".into())}
 fn cancel(&self,id:&str)->Result<(),String>{let jobs=self.jobs.lock().map_err(|_|"完整波形状态锁不可用")?;let job=jobs.iter().find(|job|job.snapshot.job_id==id).ok_or("完整波形任务不存在。")?;job.cancel.store(true,Ordering::Release);Ok(())}
 pub(crate) fn resolve_position(&self,asset_id:&str,job_id:&str,position:f64)->Result<PathBuf,String>{
  let (source,catalog,duration)={let jobs=self.jobs.lock().map_err(|_|"完整波形状态锁不可用")?;let job=jobs.iter().find(|job|job.snapshot.job_id==job_id&&job.snapshot.asset_id==asset_id).ok_or("完整波形不属于此素材，请重新分析。")?;
   let wave=job.snapshot.wave.as_ref().filter(|_|job.snapshot.status=="completed").ok_or("完整波形尚未完成，不能据此定位。")?;(job.identity.clone(),job.catalog.clone(),wave.analyzed_duration)};
  if !position.is_finite()||position<0.0||position>duration{return Err("定位超出完整波形范围。".into());}
  if identity(&catalog,asset_id)?!=source{return Err("素材已变化，请重新分析波形。".into());}Ok(source.path)
 }
}
struct Buckets{peaks:Vec<f32>,width:u64,used:u64,current:f32,frames:u64}
impl Buckets{
 fn new()->Self{Self{peaks:Vec::with_capacity(MAX_POINTS),width:1,used:0,current:0.0,frames:0}}
 fn push(&mut self,peak:f32){self.current=self.current.max(peak);self.used+=1;self.frames+=1;if self.used==self.width{self.peaks.push(self.current);self.used=0;self.current=0.0;if self.peaks.len()==MAX_POINTS{for index in 0..MAX_POINTS/2{self.peaks[index]=self.peaks[index*2].max(self.peaks[index*2+1]);}self.peaks.truncate(MAX_POINTS/2);self.width=self.width.saturating_mul(2);}}}
 fn finish(mut self)->Vec<f32>{if self.used>0{self.peaks.push(self.current);}self.peaks}
}
fn analyze(source:&mut PlaybackSource,cancel:&AtomicBool,mut progress:impl FnMut(f64,Option<f64>))->Result<Waveform,String>{
 let rate=source.sample_rate();let channels=source.channels();if rate==0||channels==0||channels>64{return Err("音频规格不支持完整波形分析。".into());}
 let expected=source.total_duration().map(|value|value.as_secs_f64());let mut buckets=Buckets::new();
 // 立体声另记两份包络，供界面上下分轨；单声道不额外分配。
 let mut lanes:Vec<Buckets>=if channels>1{(0..channels).map(|_|Buckets::new()).collect()}else{Vec::new()};
 let mut last=Instant::now();
 loop{
  if buckets.frames%2048==0&&cancel.load(Ordering::Acquire){return Err("完整波形分析已取消。".into());}
  let Some(left)=source.next() else{break};if !left.is_finite(){return Err("音频含非有限采样，不能生成完整波形。".into());}let mut peak=left.abs();
  if !lanes.is_empty(){lanes[0].push(left.abs().min(1.0));}
  for channel in 1..channels{let value=source.next().ok_or("声道采样不完整，波形分析失败。")?;if !value.is_finite(){return Err("音频含非有限采样。".into());}peak=peak.max(value.abs());if !lanes.is_empty(){lanes[channel as usize].push(value.abs().min(1.0));}}
  buckets.push(peak.min(1.0));
  if last.elapsed()>=Duration::from_millis(100){let seconds=buckets.frames as f64/rate as f64;progress(seconds,expected.filter(|v|*v>0.0).map(|v|(seconds/v).min(0.999)));last=Instant::now();}
 }
 if let Some(error)=source.decode_error(){return Err(format!("完整波形解码失败：{error}"));}
 if buckets.frames==0{return Err("音频没有有效采样。".into());}
 let seconds=buckets.frames as f64/rate as f64;
 if expected.is_some_and(|value|(value-seconds).abs()>2.0/rate as f64+0.00001){return Err("实际采样长度与文件头时长不一致，请检查文件。".into());}
 let channel_peaks=lanes.into_iter().map(|bucket|bucket.finish()).collect();
 Ok(Waveform{peaks:buckets.finish(),duration:Some(seconds),analyzed_duration:seconds,truncated:false,channels:channels as u16,channel_peaks})
}
#[derive(Serialize,Deserialize)]struct Record{identity:Identity,wave:Waveform}
fn key(source:&Identity)->String{format!("{:x}.json",Sha256::digest(serde_json::to_vec(source).expect("identity serializable")))}
fn root_safe(root:&Path)->bool{root.is_absolute()&&root.ancestors().all(|path|fs::symlink_metadata(path).map(|meta|!is_link(&meta)).unwrap_or(true))}
fn is_link(meta:&fs::Metadata)->bool{
 #[cfg(windows)]{use std::os::windows::fs::MetadataExt;meta.file_attributes()&0x400!=0}
 #[cfg(not(windows))]{meta.file_type().is_symlink()}
}
fn valid(wave:&Waveform)->bool{!wave.peaks.is_empty()&&wave.peaks.len()<=MAX_POINTS&&!wave.truncated&&wave.analyzed_duration.is_finite()&&wave.analyzed_duration>0.0&&wave.duration==Some(wave.analyzed_duration)&&wave.peaks.iter().all(|peak|peak.is_finite()&&(0.0..=1.0).contains(peak))}
fn read_cache(root:&Path,source:&Identity)->Option<Waveform>{
 if !root_safe(root){return None;}let path=root.join(key(source));let metadata=fs::symlink_metadata(&path).ok()?;if !metadata.is_file()||is_link(&metadata)||metadata.len()>CACHE_RECORD{return None;}
 let mut bytes=Vec::new();File::open(path).ok()?.take(CACHE_RECORD+1).read_to_end(&mut bytes).ok()?;let record:Record=serde_json::from_slice(&bytes).ok()?;(record.identity==*source&&valid(&record.wave)).then_some(record.wave)
}
fn write_cache(root:&Path,source:&Identity,wave:&Waveform){
 if !root_safe(root)||fs::create_dir_all(root).is_err(){return;}let Ok(bytes)=serde_json::to_vec(&Record{identity:source.clone(),wave:wave.clone()})else{return};if bytes.len() as u64>CACHE_RECORD{return;}
 let target=root.join(key(source));if target.exists(){if read_cache(root,source).is_some(){return;}let Ok(metadata)=fs::symlink_metadata(&target)else{return};if !metadata.is_file()||is_link(&metadata)||fs::remove_file(&target).is_err(){return;}}
 let temporary=root.join(format!("{}.partial",Uuid::new_v4()));
 {use std::io::Write;let Ok(mut file)=std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary)else{return};let failed=file.write_all(&bytes).is_err();drop(file);if failed{let _=fs::remove_file(&temporary);return;}}if fs::hard_link(&temporary,&target).is_err(){let _=fs::remove_file(&temporary);return;}let _=fs::remove_file(temporary);
 let Ok(entries)=fs::read_dir(root)else{return};let mut records:Vec<_>=entries.flatten().filter_map(|entry|{let path=entry.path();let name=path.file_name()?.to_str()?;if name.len()!=69||!name.ends_with(".json")||!name.as_bytes()[..64].iter().all(|c|c.is_ascii_hexdigit()){return None;}let meta=fs::symlink_metadata(&path).ok()?;if !meta.is_file()||is_link(&meta){return None;}Some((meta.modified().ok(),path,meta.len()))}).collect();
 records.sort_by_key(|v|v.0);let mut bytes:u64=records.iter().map(|v|v.2).sum();let mut count=records.len();for (_,path,size) in records{if count<=128&&bytes<=CACHE_BYTES{break;}if fs::remove_file(path).is_ok(){count-=1;bytes=bytes.saturating_sub(size);}}
}
#[tauri::command]pub async fn full_wave_start(id:String,catalog:tauri::State<'_,CatalogState>,full:tauri::State<'_,FullWaveState>)->Result<String,String>{let catalog=catalog.inner().clone();let full=full.inner().clone();tauri::async_runtime::spawn_blocking(move||full.start(id,catalog)).await.map_err(err)?}
#[tauri::command]pub fn full_wave_status(job_id:String,full:tauri::State<'_,FullWaveState>)->Result<FullSnapshot,String>{full.status(&job_id)}
#[tauri::command]pub fn full_wave_cancel(job_id:String,full:tauri::State<'_,FullWaveState>)->Result<(),String>{full.cancel(&job_id)}

#[cfg(test)]mod tests{
 use super::*;
 use std::{io::Write,thread};
 struct Fixture{root:PathBuf,catalog:Option<CatalogState>}
 impl Fixture{
  fn new()->Self{let root=std::env::temp_dir().join(format!("sound-workbench-full-wave-{}",Uuid::new_v4()));fs::create_dir(&root).unwrap();let root=fs::canonicalize(root).unwrap();let catalog=CatalogState::new(root.join("catalog.sqlite")).unwrap();Self{root,catalog:Some(catalog)}}
  fn catalog(&self)->&CatalogState{self.catalog.as_ref().unwrap()}
  fn source(&self,seconds:u32)->String{
   let rate=1000u32;let frames=seconds*rate;let bytes=frames*2;let path=self.root.join(format!("{}.wav",Uuid::new_v4()));let mut output=File::create(&path).unwrap();
   output.write_all(b"RIFF").unwrap();output.write_all(&(36+bytes).to_le_bytes()).unwrap();output.write_all(b"WAVEfmt ").unwrap();output.write_all(&16u32.to_le_bytes()).unwrap();output.write_all(&1u16.to_le_bytes()).unwrap();output.write_all(&1u16.to_le_bytes()).unwrap();output.write_all(&rate.to_le_bytes()).unwrap();output.write_all(&(rate*2).to_le_bytes()).unwrap();output.write_all(&2u16.to_le_bytes()).unwrap();output.write_all(&16u16.to_le_bytes()).unwrap();output.write_all(b"data").unwrap();output.write_all(&bytes.to_le_bytes()).unwrap();
   for frame in 0..frames{let sample=if frame==12||frame==frames-10{24576i16}else{0};output.write_all(&sample.to_le_bytes()).unwrap();}drop(output);
   let path=fs::canonicalize(path).unwrap();let metadata=fs::metadata(&path).unwrap();let modified=metadata.modified().unwrap().duration_since(UNIX_EPOCH).unwrap().as_nanos().to_string();let id=Uuid::new_v4().to_string();
   let conn=rusqlite::Connection::open(self.catalog().database_path()).unwrap();conn.execute("INSERT INTO assets(id,path_key,path,name,format,size,modified,rate,channels,duration,status) VALUES(?1,?2,?2,'long.wav','WAV',?3,?4,1000,1,?5,'ready')",rusqlite::params![id,path.display().to_string(),metadata.len(),modified,seconds]).unwrap();id
  }
 }
 impl Drop for Fixture{fn drop(&mut self){self.catalog.take();let _=fs::remove_dir_all(&self.root);}}
 fn finished(state:&FullWaveState,token:&str)->FullSnapshot{let until=Instant::now()+Duration::from_secs(10);loop{let value=state.status(token).unwrap();if !["running","preparing"].contains(&value.status.as_str()){return value;}assert!(Instant::now()<until,"full wave test timed out");thread::sleep(Duration::from_millis(5));}}
 #[test]fn full_wave_analyzes_beyond_sixty_seconds_with_bounded_real_peaks(){
  let fixture=Fixture::new();let id=fixture.source(72);let path=fixture.catalog().resolve_asset(&id).unwrap();let original=fs::read(&path).unwrap();let state=FullWaveState::new(fixture.root.join("cache"));let token=state.start(id.clone(),fixture.catalog().clone()).unwrap();let snapshot=finished(&state,&token);assert_eq!(snapshot.status,"completed","{:?}",snapshot.error);let wave=snapshot.wave.unwrap();assert_eq!(wave.analyzed_duration,72.0);assert!(!wave.truncated);assert!(wave.peaks.len()<=MAX_POINTS);assert_eq!(wave.peaks[0],0.75);assert_eq!(wave.peaks.last().copied(),Some(0.75));assert_eq!(state.resolve_position(&id,&token,65.0).unwrap(),path);assert!(state.resolve_position("wrong",&token,65.0).is_err());assert!(state.resolve_position(&id,&token,72.1).is_err());assert_eq!(fs::read(path).unwrap(),original);
 }
 #[test]fn full_wave_cache_recovers_corruption_and_rejects_changed_source(){
  let fixture=Fixture::new();let id=fixture.source(2);let source=identity(fixture.catalog(),&id).unwrap();let mut reader=PlaybackSource::open(&source.path).unwrap();let wave=analyze(&mut reader,&AtomicBool::new(false),|_,_|{}).unwrap();let cache=fixture.root.join("cache");write_cache(&cache,&source,&wave);assert!(read_cache(&cache,&source).is_some());fs::write(cache.join(key(&source)),b"bad json").unwrap();assert!(read_cache(&cache,&source).is_none());write_cache(&cache,&source,&wave);assert!(read_cache(&cache,&source).is_some());let mut wrong=source.clone();wrong.version+=1;assert!(read_cache(&cache,&wrong).is_none());let state=FullWaveState::new(cache);let token=state.start(id.clone(),fixture.catalog().clone()).unwrap();assert_eq!(finished(&state,&token).status,"completed");std::fs::OpenOptions::new().append(true).open(&source.path).unwrap().write_all(&[0]).unwrap();assert!(state.resolve_position(&id,&token,0.0).is_err());
 }
 #[test]fn full_wave_cancel_rejects_partial_result_and_compaction_remains_bounded(){
  let fixture=Fixture::new();let id=fixture.source(72);let mut reader=PlaybackSource::open(&fixture.catalog().resolve_asset(&id).unwrap()).unwrap();assert!(analyze(&mut reader,&AtomicBool::new(true),|_,_|{}).is_err());let mut buckets=Buckets::new();for frame in 0..1_000_000{buckets.push(if frame==1||frame==999_999{0.9}else if frame==345_678{0.8}else{0.0});assert!(buckets.peaks.len()<MAX_POINTS);}let bucket_width=buckets.width;let peaks=buckets.finish();assert!(peaks.len()<=MAX_POINTS);assert_eq!(peaks[0],0.9);assert_eq!(peaks.last().copied(),Some(0.9));let transient=peaks.iter().position(|peak|*peak==0.8).unwrap();let estimated=transient as f64/(peaks.len()-1) as f64*999_999.0;assert!((estimated-345_678.0).abs()<=bucket_width as f64,"overview transient error should stay within one bin");
 }
}
