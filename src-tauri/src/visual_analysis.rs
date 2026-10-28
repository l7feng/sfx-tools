//! 真实全长RMS与有界频谱概览。离线读取，不打开声卡、不改源音频。
use crate::{catalog::CatalogState,player::PlaybackSource};
use rodio::Source;
use serde::{Serialize,Deserialize};
use sha2::{Digest,Sha256};
use std::{collections::VecDeque,fs::{self,File,OpenOptions},io::{Read,Write},path::{Path,PathBuf},sync::{Arc,Mutex,atomic::{AtomicBool,AtomicUsize,Ordering}},time::{Duration,Instant,UNIX_EPOCH}};
use uuid::Uuid;
const FFT:usize=1024;const TIME_BINS:usize=256;const FREQ_BINS:usize=64;const ENVELOPE_BINS:usize=2048;const VERSION:u32=1;const RECORD_LIMIT:u64=1024*1024;const CACHE_LIMIT:u64=32*1024*1024;
fn err(e:impl std::fmt::Display)->String{e.to_string()}
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq)]struct Identity{path:PathBuf,size:u64,modified:String,version:u32}
fn identity(catalog:&CatalogState,id:&str)->Result<Identity,String>{
 let path=catalog.resolve_asset(id)?;let meta=fs::symlink_metadata(&path).map_err(err)?;
 let modified=meta.modified().map_err(err)?.duration_since(UNIX_EPOCH).map_err(err)?.as_nanos().to_string();
 let conn=rusqlite::Connection::open_with_flags(catalog.database_path(),rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(err)?;
 let (size,stamp):(u64,String)=conn.query_row("SELECT size,modified FROM assets WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(err)?;
 if size!=meta.len()||stamp!=modified{return Err("素材已变化，请先重新扫描再分析频谱。".into());}
 Ok(Identity{path,size,modified,version:VERSION})
}
#[derive(Clone,Debug,Serialize,Deserialize)]pub struct VisualData{pub duration:f64,pub rate:u32,pub peaks:Vec<f32>,pub rms:Vec<f32>,pub spectrum:Vec<Vec<f32>>,pub frequencies:Vec<f64>,pub centroids:Vec<f64>,pub fft_size:usize,pub window_count:usize}
#[derive(Clone,Debug,Serialize)]pub struct VisualSnapshot{pub job_id:String,pub asset_id:String,pub status:String,pub progress:Option<f64>,pub analyzed_seconds:f64,pub error:Option<String>,pub result:Option<VisualData>}
struct Job{snapshot:VisualSnapshot,cancel:Arc<AtomicBool>}
#[derive(Clone)]pub struct VisualAnalysisState{root:PathBuf,jobs:Arc<Mutex<Vec<Job>>>,active:Arc<AtomicUsize>}
impl VisualAnalysisState{
 pub fn new(root:PathBuf)->Self{Self{root,jobs:Arc::new(Mutex::new(vec![])),active:Arc::new(AtomicUsize::new(0))}}
 fn start(&self,id:String,catalog:CatalogState)->Result<String,String>{
  let source=identity(&catalog,&id)?;let token=Uuid::new_v4().to_string();let cancel=Arc::new(AtomicBool::new(false));
  {let mut jobs=self.jobs.lock().map_err(|_|"频谱任务锁不可用")?;
   for job in jobs.iter(){if job.snapshot.status=="running"{job.cancel.store(true,Ordering::Release);}}
   if self.active.load(Ordering::Acquire)>=2{return Err("上一频谱任务正在取消，请稍后重试。".into());}
   while jobs.len()>=16{if let Some(index)=jobs.iter().position(|j|j.snapshot.status!="running"){jobs.remove(index);}else{return Err("频谱分析繁忙。".into());}}
   self.active.fetch_add(1,Ordering::AcqRel);jobs.push(Job{snapshot:VisualSnapshot{job_id:token.clone(),asset_id:id.clone(),status:"running".into(),progress:Some(0.0),analyzed_seconds:0.0,error:None,result:None},cancel:cancel.clone()});}
  let state=self.clone();let job_id=token.clone();
  if let Err(error)=std::thread::Builder::new().name("sound-workbench-visual-analysis".into()).spawn(move||{
   struct Guard(Arc<AtomicUsize>);impl Drop for Guard{fn drop(&mut self){self.0.fetch_sub(1,Ordering::AcqRel);}}let _guard=Guard(state.active.clone());
   let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{
    let result=match read_cache(&state.root,&source){Some(data)=>data,None=>{let mut reader=PlaybackSource::open(&source.path)?;let data=analyze(&mut reader,&cancel,|seconds,progress|{if let Ok(mut jobs)=state.jobs.lock(){if let Some(job)=jobs.iter_mut().find(|j|j.snapshot.job_id==job_id){job.snapshot.analyzed_seconds=seconds;job.snapshot.progress=Some(progress);}}})?;
     if identity(&catalog,&id)?!=source{return Err("分析时素材发生变化，请重新扫描。".into());}if !cancel.load(Ordering::Acquire){write_cache(&state.root,&source,&data);}data}};
    if identity(&catalog,&id)?!=source{return Err("频谱来源已变化。".into());}Ok::<_,String>(result)
   })).unwrap_or_else(|_|Err("频谱分析线程异常，请重试。".into()));
   if let Ok(mut jobs)=state.jobs.lock(){if let Some(job)=jobs.iter_mut().find(|j|j.snapshot.job_id==job_id){if cancel.load(Ordering::Acquire){job.snapshot.status="cancelled".into();}else{match result{Ok(data)=>{job.snapshot.analyzed_seconds=data.duration;job.snapshot.progress=Some(1.0);job.snapshot.result=Some(data);job.snapshot.status="completed".into();},Err(e)=>{job.snapshot.status="failed".into();job.snapshot.error=Some(e);}}}}}
  }){self.active.fetch_sub(1,Ordering::AcqRel);if let Ok(mut jobs)=self.jobs.lock(){if let Some(job)=jobs.iter_mut().find(|j|j.snapshot.job_id==token){job.snapshot.status="failed".into();job.snapshot.error=Some(error.to_string());}}return Err(error.to_string());}
  Ok(token)
 }
 fn status(&self,id:&str)->Result<VisualSnapshot,String>{self.jobs.lock().map_err(|_|"频谱状态锁不可用")?.iter().find(|j|j.snapshot.job_id==id).map(|j|j.snapshot.clone()).ok_or("频谱任务已过期，请重试。".into())}
 fn cancel(&self,id:&str)->Result<(),String>{let jobs=self.jobs.lock().map_err(|_|"频谱任务锁不可用")?;jobs.iter().find(|j|j.snapshot.job_id==id).ok_or("频谱任务不存在。")?.cancel.store(true,Ordering::Release);Ok(())}
}
// 原位radix-2 FFT；只用于固定1024采样真实窗，不是合成或随机纹理。
fn fft_power(input:&[f32])->Vec<f64>{
 let mut re=vec![0f64;FFT];let mut im=vec![0f64;FFT];
 let length=input.len().min(FFT);for (i,&v) in input.iter().take(FFT).enumerate(){let weight=if length<=1{1.0}else{0.5-0.5*(2.0*std::f64::consts::PI*i as f64/(length-1) as f64).cos()};re[i]=v as f64*weight;}
 let mut j=0;for i in 1..FFT{let mut bit=FFT>>1;while j&bit!=0{j^=bit;bit>>=1;}j^=bit;if i<j{re.swap(i,j);im.swap(i,j);}}
 let mut len=2;while len<=FFT{let angle=-2.0*std::f64::consts::PI/len as f64;let (wi,wr)=angle.sin_cos();for start in (0..FFT).step_by(len){let (mut ur,mut ui)=(1.0,0.0);for i in 0..len/2{let a=start+i;let b=a+len/2;let vr=re[b]*ur-im[b]*ui;let vi=re[b]*ui+im[b]*ur;let ar=re[a];let ai=im[a];re[a]=ar+vr;im[a]=ai+vi;re[b]=ar-vr;im[b]=ai-vi;let next=ur*wr-ui*wi;ui=ur*wi+ui*wr;ur=next;}}len*=2;}
 (0..=FFT/2).map(|i|re[i]*re[i]+im[i]*im[i]).collect()
}
fn bands(rings:&[VecDeque<f32>],rate:u32)->(Vec<f32>,f64){
 let mut power=vec![0.0;FFT/2+1];for ring in rings{let values:Vec<f32>=ring.iter().copied().collect();let next=fft_power(&values);for (p,n) in power.iter_mut().zip(next){*p+=n/rings.len() as f64;}}
 let high=rate as f64/2.0;let low=20.0f64.min(high/2.0);let ratio=high/low;
 let spectrum=(0..FREQ_BINS).map(|band|{let lo=low*ratio.powf(band as f64/FREQ_BINS as f64);let hi=low*ratio.powf((band+1) as f64/FREQ_BINS as f64);let start=((lo*FFT as f64/rate as f64).floor() as usize).max(1).min(FFT/2);let end=((hi*FFT as f64/rate as f64).ceil() as usize).max(start).min(FFT/2);let p=power[start..=end].iter().copied().fold(0.0,f64::max);let normalization=if rings[0].len()<=1{1.0}else{(rings[0].len()-1) as f64/2.0};let amplitude=p.sqrt()*2.0/normalization;(20.0*amplitude.max(1e-5).log10()).clamp(-100.0,0.0) as f32}).collect();
 let sum=power.iter().skip(1).sum::<f64>();let centroid=if sum>0.0{power.iter().enumerate().skip(1).map(|(i,p)|i as f64*rate as f64/FFT as f64*p).sum::<f64>()/sum}else{0.0};(spectrum,centroid)
}
fn analyze(source:&mut PlaybackSource,cancel:&AtomicBool,mut progress:impl FnMut(f64,f64))->Result<VisualData,String>{
 let rate=source.sample_rate();let channels=source.channels() as usize;if rate==0||channels==0||channels>8{return Err("高级波形暂支持1至8声道。".into());}
 let expected=source.total_duration().ok_or("高级波形需要可读取的文件头时长，请重新扫描或转换格式。")?.as_secs_f64();
 if !expected.is_finite()||expected<=0.0{return Err("音频时长无效。".into());}
 let total=(expected*rate as f64).round() as u64;if total==0{return Err("音频没有采样。".into());}
 let bins=ENVELOPE_BINS.min(total as usize);let mut peak=vec![0f32;bins];let mut squares=vec![0f64;bins];let mut count=vec![0u64;bins];
 let window=FFT.min(total as usize);let ends:Vec<u64>=(0..TIME_BINS).map(|i|{let center=((i as f64+0.5)*total as f64/TIME_BINS as f64).round() as u64;center.saturating_sub(window as u64/2).min(total-window as u64)+window as u64}).collect();
 let mut rings:Vec<VecDeque<f32>>=(0..channels).map(|_|VecDeque::with_capacity(FFT)).collect();let mut spectrum=Vec::with_capacity(TIME_BINS);let mut centroids=Vec::with_capacity(TIME_BINS);let (mut frame,mut next)=(0u64,0usize);let mut last=Instant::now();
 loop{if frame%2048==0&&cancel.load(Ordering::Acquire){return Err("高级波形分析已取消。".into());}
  let Some(first)=source.next()else{break};let bin=((frame as f64*bins as f64/total as f64) as usize).min(bins-1);let mut energy=0f64;
  for channel in 0..channels{let v=if channel==0{first}else{source.next().ok_or("声道采样不完整。")?};if !v.is_finite(){return Err("源音频包含非有限采样。".into());}peak[bin]=peak[bin].max(v.abs().min(1.0));energy+=v as f64*v as f64;let ring=&mut rings[channel];if ring.len()==FFT{ring.pop_front();}ring.push_back(v);}
  squares[bin]+=energy/channels as f64;count[bin]+=1;frame+=1;
  if next<TIME_BINS&&frame>=ends[next]{let (db,centroid)=bands(&rings,rate);while next<TIME_BINS&&frame>=ends[next]{spectrum.push(db.clone());centroids.push(centroid);next+=1;}}
  if last.elapsed()>=Duration::from_millis(100){progress(frame as f64/rate as f64,(frame as f64/total as f64).min(0.999));last=Instant::now();}
 }
 if let Some(error)=source.decode_error(){return Err(format!("高级波形解码失败：{error}"));}
 if frame.abs_diff(total)>2{return Err("实际采样长度与文件头不一致。".into());}if frame==0{return Err("音频没有有效采样。".into());}
 if spectrum.len()!=TIME_BINS{return Err("频谱窗分析不完整。".into());}
 let rms=squares.iter().zip(count).map(|(sum,n)|if n==0{0.0}else{(sum/n as f64).sqrt().min(1.0) as f32}).collect();
 let high=rate as f64/2.0;let low=20f64.min(high/2.0);let frequencies=(0..FREQ_BINS).map(|b|low*(high/low).powf((b as f64+0.5)/FREQ_BINS as f64)).collect();
 Ok(VisualData{duration:frame as f64/rate as f64,rate,peaks:peak,rms,spectrum,frequencies,centroids,fft_size:FFT,window_count:TIME_BINS})
}
#[derive(Serialize,Deserialize)]struct Record{identity:Identity,data:VisualData}
fn link(meta:&fs::Metadata)->bool{#[cfg(windows)]{use std::os::windows::fs::MetadataExt;meta.file_attributes()&0x400!=0}#[cfg(not(windows))]{meta.file_type().is_symlink()}}
fn safe_root(root:&Path)->bool{root.is_absolute()&&root.ancestors().all(|p|fs::symlink_metadata(p).map(|m|!link(&m)).unwrap_or(true))}
fn key(source:&Identity)->String{format!("{:x}.json",Sha256::digest(serde_json::to_vec(source).expect("identity")))}
fn valid(data:&VisualData)->bool{data.duration.is_finite()&&data.duration>0.0&&data.rate>0&&data.peaks.len()>0&&data.peaks.len()<=ENVELOPE_BINS&&data.peaks.len()==data.rms.len()&&data.spectrum.len()==TIME_BINS&&data.centroids.len()==TIME_BINS&&data.frequencies.len()==FREQ_BINS&&data.fft_size==FFT&&data.window_count==TIME_BINS&&data.peaks.iter().chain(&data.rms).all(|x|x.is_finite()&&(0.0..=1.0).contains(x))&&data.spectrum.iter().all(|row|row.len()==FREQ_BINS&&row.iter().all(|x|x.is_finite()&&(-100.0..=0.0).contains(x)))&&data.centroids.iter().all(|x|x.is_finite()&&*x>=0.0&&*x<=data.rate as f64/2.0)&&data.frequencies.iter().all(|x|x.is_finite()&&*x>0.0)}
fn read_cache(root:&Path,source:&Identity)->Option<VisualData>{if !safe_root(root){return None;}let path=root.join(key(source));let meta=fs::symlink_metadata(&path).ok()?;if !meta.is_file()||link(&meta)||meta.len()>RECORD_LIMIT{return None;}let mut bytes=vec![];File::open(path).ok()?.take(RECORD_LIMIT+1).read_to_end(&mut bytes).ok()?;let record:Record=serde_json::from_slice(&bytes).ok()?;(record.identity==*source&&valid(&record.data)).then_some(record.data)}
fn write_cache(root:&Path,source:&Identity,data:&VisualData){
 if !safe_root(root)||fs::create_dir_all(root).is_err(){return;}let Ok(bytes)=serde_json::to_vec(&Record{identity:source.clone(),data:data.clone()})else{return};if bytes.len() as u64>RECORD_LIMIT{return;}let target=root.join(key(source));if target.exists(){if read_cache(root,source).is_some(){return;}if fs::symlink_metadata(&target).map(|m|m.is_file()&&!link(&m)).unwrap_or(false){if fs::remove_file(&target).is_err(){return;}}else{return;}}
 let temporary=root.join(format!("{}.partial",Uuid::new_v4()));let Ok(mut file)=OpenOptions::new().write(true).create_new(true).open(&temporary)else{return};let failed=file.write_all(&bytes).is_err();drop(file);if failed||fs::hard_link(&temporary,&target).is_err(){let _=fs::remove_file(&temporary);return;}let _=fs::remove_file(temporary);
 let Ok(entries)=fs::read_dir(root)else{return};let mut files:Vec<_>=entries.flatten().filter_map(|e|{let p=e.path();let n=p.file_name()?.to_str()?;if n.len()!=69||!n.ends_with(".json")||!n.as_bytes()[..64].iter().all(|x|x.is_ascii_hexdigit()){return None;}let m=fs::symlink_metadata(&p).ok()?;if !m.is_file()||link(&m){return None;}Some((m.modified().ok(),p,m.len()))}).collect();files.sort_by_key(|e|e.0);let mut used=files.iter().map(|e|e.2).sum::<u64>();let mut count=files.len();for (_,p,size) in files{if count<=64&&used<=CACHE_LIMIT{break;}if fs::remove_file(p).is_ok(){count-=1;used=used.saturating_sub(size);}}
}
#[tauri::command]pub async fn visual_analysis_start(id:String,catalog:tauri::State<'_,CatalogState>,visual:tauri::State<'_,VisualAnalysisState>)->Result<String,String>{let catalog=catalog.inner().clone();let visual=visual.inner().clone();tauri::async_runtime::spawn_blocking(move||visual.start(id,catalog)).await.map_err(err)?}
#[tauri::command]pub fn visual_analysis_status(job_id:String,visual:tauri::State<'_,VisualAnalysisState>)->Result<VisualSnapshot,String>{visual.status(&job_id)}
#[tauri::command]pub fn visual_analysis_cancel(job_id:String,visual:tauri::State<'_,VisualAnalysisState>)->Result<(),String>{visual.cancel(&job_id)}


#[cfg(test)]mod tests{
 use super::*;
 use std::{io::BufWriter,thread};
 struct Fixture{root:PathBuf,catalog:Option<CatalogState>}
 impl Fixture{
  fn new()->Self{let root=std::env::temp_dir().join(format!("sound-workbench-spectrum-{}",Uuid::new_v4()));fs::create_dir(&root).unwrap();let root=fs::canonicalize(root).unwrap();let catalog=CatalogState::new(root.join("catalog.sqlite")).unwrap();Self{root,catalog:Some(catalog)}}
  fn catalog(&self)->&CatalogState{self.catalog.as_ref().unwrap()}
  fn source(&self,rate:u32,frames:u32,channels:u16,mut value:impl FnMut(u32,u16)->f64)->(String,PathBuf){
   let path=self.root.join(format!("{}.wav",Uuid::new_v4()));let bytes=frames*channels as u32*2;let mut out=BufWriter::new(File::create(&path).unwrap());
   out.write_all(b"RIFF").unwrap();out.write_all(&(36+bytes).to_le_bytes()).unwrap();out.write_all(b"WAVEfmt ").unwrap();out.write_all(&16u32.to_le_bytes()).unwrap();out.write_all(&1u16.to_le_bytes()).unwrap();out.write_all(&channels.to_le_bytes()).unwrap();out.write_all(&rate.to_le_bytes()).unwrap();out.write_all(&(rate*channels as u32*2).to_le_bytes()).unwrap();out.write_all(&(channels*2).to_le_bytes()).unwrap();out.write_all(&16u16.to_le_bytes()).unwrap();out.write_all(b"data").unwrap();out.write_all(&bytes.to_le_bytes()).unwrap();
   for frame in 0..frames{for channel in 0..channels{out.write_all(&((value(frame,channel).clamp(-1.0,1.0)*32767.0).round() as i16).to_le_bytes()).unwrap();}}out.flush().unwrap();drop(out);
   let path=fs::canonicalize(path).unwrap();let metadata=fs::metadata(&path).unwrap();let modified=metadata.modified().unwrap().duration_since(UNIX_EPOCH).unwrap().as_nanos().to_string();let id=Uuid::new_v4().to_string();let conn=rusqlite::Connection::open(self.catalog().database_path()).unwrap();
   conn.execute("INSERT INTO assets(id,path_key,path,name,format,size,modified,rate,channels,duration,status) VALUES(?1,?2,?2,'spectrum.wav','WAV',?3,?4,?5,?6,?7,'ready')",rusqlite::params![id,path.display().to_string(),metadata.len(),modified,rate,channels,frames as f64/rate as f64]).unwrap();(id,path)
  }
 }
 impl Drop for Fixture{fn drop(&mut self){self.catalog.take();let _=fs::remove_dir_all(&self.root);}}
 fn data(path:&Path)->VisualData{let mut reader=PlaybackSource::open(path).unwrap();analyze(&mut reader,&AtomicBool::new(false),|_,_|{}).unwrap()}
 fn finish(state:&VisualAnalysisState,id:&str)->VisualSnapshot{let until=Instant::now()+Duration::from_secs(10);loop{let snapshot=state.status(id).unwrap();if snapshot.status!="running"{return snapshot;}assert!(Instant::now()<until,"visual analysis timeout");thread::sleep(Duration::from_millis(5));}}
 #[test]fn visual_fft_and_rms_measure_real_sine_and_silence(){
  let signal:Vec<f32>=(0..FFT).map(|i|(0.5*(2.0*std::f64::consts::PI*21.0*i as f64/FFT as f64).sin()) as f32).collect();let power=fft_power(&signal);assert_eq!(power.iter().enumerate().max_by(|a,b|a.1.total_cmp(b.1)).unwrap().0,21);
  let f=Fixture::new();let (_,path)=f.source(48000,24000,1,|frame,_|0.5*(2.0*std::f64::consts::PI*984.375*frame as f64/48000.0).sin());let result=data(&path);
  let rms=(result.rms.iter().map(|v|(*v as f64).powi(2)).sum::<f64>()/result.rms.len() as f64).sqrt();assert!((rms-0.5/2.0f64.sqrt()).abs()<0.005,"measured RMS {rms}");let strongest=result.spectrum[100].iter().enumerate().max_by(|a,b|a.1.total_cmp(b.1)).unwrap().0;assert!((result.frequencies[strongest]/984.375-1.0).abs()<0.2);
  assert_eq!(result.spectrum.len(),TIME_BINS);assert_eq!(result.spectrum[0].len(),FREQ_BINS);assert_eq!(result.duration,0.5);assert!(valid(&result));
  let (_,silence)=f.source(48000,4800,1,|_,_|0.0);let quiet=data(&silence);assert!(quiet.peaks.iter().chain(&quiet.rms).all(|v|*v==0.0));assert!(quiet.spectrum.iter().flatten().all(|v|*v==-100.0));assert!(quiet.centroids.iter().all(|v|*v==0.0));
 }
 #[test]fn visual_stereo_power_does_not_cancel_opposite_channels(){
  let f=Fixture::new();let (_,path)=f.source(48000,12000,2,|frame,channel|{let value=0.5*(2.0*std::f64::consts::PI*3000.0*frame as f64/48000.0).sin();if channel==0{value}else{-value}});let result=data(&path);assert!(result.rms.iter().any(|v|*v>0.3));assert!(result.spectrum[100].iter().copied().fold(-100.0,f32::max)>-10.0);assert!((result.centroids[100]-3000.0).abs()<10.0);
 }
 #[test]fn visual_whole_duration_cache_and_source_identity_are_checked(){
  let f=Fixture::new();let (id,path)=f.source(1000,72000,1,|frame,_|if frame>=70000{0.5*(2.0*std::f64::consts::PI*100.0*frame as f64/1000.0).sin()}else{0.0});let before=fs::read(&path).unwrap();let result=data(&path);assert_eq!(result.duration,72.0);assert_eq!(result.peaks.len(),ENVELOPE_BINS);assert!(result.peaks.iter().take(1900).all(|v|*v==0.0));assert!(result.rms.last().copied().unwrap()>0.3);assert!(result.spectrum.last().unwrap().iter().any(|v|*v>-10.0));
  let source=identity(f.catalog(),&id).unwrap();let cache=f.root.join("cache");write_cache(&cache,&source,&result);assert!(read_cache(&cache,&source).is_some());fs::write(cache.join(key(&source)),b"broken").unwrap();assert!(read_cache(&cache,&source).is_none());write_cache(&cache,&source,&result);assert!(read_cache(&cache,&source).is_some());let mut wrong=source.clone();wrong.version+=1;assert!(read_cache(&cache,&wrong).is_none());
  let state=VisualAnalysisState::new(cache);let token=state.start(id.clone(),f.catalog().clone()).unwrap();let snapshot=finish(&state,&token);assert_eq!(snapshot.asset_id,id);assert_eq!(snapshot.status,"completed","{:?}",snapshot.error);assert_eq!(snapshot.result.unwrap().duration,72.0);assert_eq!(fs::read(&path).unwrap(),before);
  OpenOptions::new().append(true).open(path).unwrap().write_all(&[0]).unwrap();assert!(state.start(id,f.catalog().clone()).is_err());
 }
 #[test]fn visual_cancellation_and_malformed_shape_do_not_create_results(){
  let f=Fixture::new();let (_,path)=f.source(48000,4800,1,|_,_|0.1);let mut source=PlaybackSource::open(&path).unwrap();assert!(analyze(&mut source,&AtomicBool::new(true),|_,_|{}).unwrap_err().contains("取消"));let mut result=data(&path);result.spectrum[0].pop();assert!(!valid(&result));assert!(VisualAnalysisState::new(f.root.join("cache")).status("unknown").is_err());
 }
}
