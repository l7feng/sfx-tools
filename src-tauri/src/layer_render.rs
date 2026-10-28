//! 叠层离线混音：只解析已登记素材，流式写出48kHz双声道PCM16，不打开声卡。
use crate::{catalog::CatalogState,player::PlaybackSource};
use rodio::Source;
use serde::{Deserialize,Serialize};
use std::{collections::{HashMap,VecDeque},fs::{self,File,OpenOptions},io::{Read,Write,BufReader,BufWriter},path::{Path,PathBuf},sync::{Arc,Mutex,atomic::{AtomicBool,Ordering}},time::UNIX_EPOCH};
use uuid::Uuid;
const RATE:u32=48_000;
const CHANNELS:u16=2;
const MAX_SECONDS:f64=3600.0;
const MAX_CLIPS:usize=200;
const MAX_OVERLAP:usize=32;
const BLOCK:usize=1024;
const CACHE_LIMIT:u64=1536*1024*1024;
const DISK_RESERVE:u64=16*1024*1024;
fn err(e:impl std::fmt::Display)->String{e.to_string()}
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct RenderClip {pub asset_id:String,pub offset:f64,pub gain:f64,pub duration:f64}
#[derive(Clone,Debug,Serialize,PartialEq)]
pub struct SourceStamp{pub asset_id:String,pub path:String,pub size:u64,pub modified:String}
#[derive(Clone,Debug,Serialize)]
pub struct RenderSnapshot{
 pub job_id:Option<String>,pub status:String,pub progress:f64,pub rendered_seconds:f64,pub duration:f64,
 pub output_path:Option<String>,pub bytes:u64,pub peak:f64,pub clipped_samples:u64,pub error:Option<String>,
 pub clips:Vec<RenderClip>,pub sources:Vec<SourceStamp>,
}
impl Default for RenderSnapshot{fn default()->Self{Self{job_id:None,status:"idle".into(),progress:0.0,rendered_seconds:0.0,duration:0.0,output_path:None,bytes:0,peak:0.0,clipped_samples:0,error:None,clips:vec![],sources:vec![]}}}
#[derive(Clone)]
pub struct LayerRenderState{root:PathBuf,inner:Arc<Mutex<RenderInner>>}
struct RenderInner{snapshot:RenderSnapshot,cancel:Option<Arc<AtomicBool>>,completed:VecDeque<Completed>}
#[derive(Clone)]
struct Completed{job_id:String,path:PathBuf,identity:Identity,sources:Vec<SourceStamp>,catalog:CatalogState}
#[derive(Clone,Debug,PartialEq)]
struct Identity{size:u64,modified:String}
impl LayerRenderState{
 pub fn new(root:PathBuf)->Self{Self{root,inner:Arc::new(Mutex::new(RenderInner{snapshot:Default::default(),cancel:None,completed:VecDeque::new()}))}}
 fn snapshot(&self)->Result<RenderSnapshot,String>{Ok(self.inner.lock().map_err(|_|"渲染状态锁不可用")?.snapshot.clone())}
 pub fn resolve_output(&self,job_id:&str)->Result<PathBuf,String>{
  // 未知令牌在任何文件I/O前拒绝，不接收调用者提供的输出路径。
  let complete=self.inner.lock().map_err(|_|"渲染状态锁不可用")?.completed.iter().find(|v|v.job_id==job_id).cloned().ok_or("该渲染结果不存在、未完成或不属于本次程序运行，请重新渲染。")?;
  let root=cache_root(&self.root)?;
  if complete.path.parent()!=Some(root.as_path())||complete.path.file_name().and_then(|s|s.to_str())!=Some(&format!("{}.wav",complete.job_id)){return Err("渲染缓存路径无效。".into());}
  reject_links(&complete.path)?;
  if identity(&complete.path)?!=complete.identity{return Err("渲染缓存已经变化，请重新渲染。".into());}
  validate_sources(&complete.catalog,&complete.sources)?;
  Ok(complete.path)
 }
 fn start(&self,clips:Vec<RenderClip>,catalog:CatalogState)->Result<String,String>{
  if self.inner.lock().map_err(|_|"渲染状态锁不可用")?.cancel.is_some(){return Err("已有叠层渲染进行中，请等待或取消。".into());}
  let job_id=Uuid::new_v4().to_string();let cancel=Arc::new(AtomicBool::new(false));
  {
   let mut inner=self.inner.lock().map_err(|_|"渲染状态锁不可用")?;
   if inner.cancel.is_some(){return Err("已有叠层渲染进行中，请等待或取消。".into());}
   inner.snapshot=RenderSnapshot{job_id:Some(job_id.clone()),status:"preparing".into(),clips:clips.clone(),..Default::default()};inner.cancel=Some(cancel.clone());
  }
  // 预检前即占据单任务槽，防两个请求沿用过期配额；昂贵I/O不持有状态锁。
  let preparation=(||{let plan=prepare(&catalog,clips)?;let root=cache_root(&self.root)?;check_capacity(&root,44+plan.frames*4)?;Ok::<_,String>((plan,root))})();
  let (plan,root)=match preparation{Ok(value)=>value,Err(message)=>{let mut inner=self.inner.lock().map_err(|_|"渲染状态锁不可用")?;inner.snapshot.status="failed".into();inner.snapshot.error=Some(message.clone());inner.cancel=None;return Err(message);}};
  {
   let mut inner=self.inner.lock().map_err(|_|"渲染状态锁不可用")?;
   if cancel.load(Ordering::Acquire){inner.snapshot.status="cancelled".into();inner.cancel=None;return Err("渲染准备已取消。".into());}
   inner.snapshot=RenderSnapshot{job_id:Some(job_id.clone()),status:"running".into(),duration:plan.frames as f64/RATE as f64,clips:plan.recipe.clone(),sources:plan.sources.clone(),..Default::default()};
   inner.cancel=Some(cancel.clone());
  }
  let state=self.clone();let id=job_id.clone();let worker_id=job_id.clone();
  let spawn=std::thread::Builder::new().name("sound-workbench-layer-render".into()).spawn(move||{
   let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{let done=render(&root,&worker_id,&plan,&cancel,|frames|{
    if let Ok(mut inner)=state.inner.lock(){if inner.snapshot.job_id.as_deref()==Some(&worker_id){inner.snapshot.rendered_seconds=frames as f64/RATE as f64;inner.snapshot.progress=(frames as f64/plan.frames as f64).min(0.999);inner.snapshot.bytes=44+frames*4;}}
   })?;if let Err(e)=validate_sources(&catalog,&plan.sources){let _=fs::remove_file(&done.path);return Err(e);}Ok(done)})).unwrap_or_else(|_|Err("渲染线程异常，未交付结果。".into()));
   let mut inner=match state.inner.lock(){Ok(v)=>v,Err(_)=>return};
   let cancelled=cancel.load(Ordering::Acquire);
   match result{
    Ok(mut done) if !cancelled=>{
     inner.snapshot.status="completed".into();inner.snapshot.progress=1.0;inner.snapshot.rendered_seconds=inner.snapshot.duration;
     inner.snapshot.output_path=Some(done.path.display().to_string());inner.snapshot.bytes=done.identity.size;
     inner.snapshot.peak=done.peak;inner.snapshot.clipped_samples=done.clipped_samples;
     inner.completed.push_back(Completed{job_id:worker_id,path:done.path.clone(),identity:done.identity.clone(),sources:plan.sources,catalog});
     while inner.completed.len()>256{inner.completed.pop_front();}done.retained=true;
     // 成功缓存可能仍在试听，本会话不自动删除；总磁盘配额限制后续任务。
    },
    Ok(_done)=>{inner.snapshot.status="cancelled".into();inner.snapshot.output_path=None;inner.snapshot.bytes=0;},
    Err(message)=>{inner.snapshot.status=if cancelled{"cancelled"}else{"failed"}.into();inner.snapshot.error=if cancelled{None}else{Some(message)};inner.snapshot.output_path=None;inner.snapshot.bytes=0;},
   }
   inner.cancel=None;
  });
  if let Err(e)=spawn{let mut inner=self.inner.lock().map_err(|_|"渲染状态锁不可用")?;if inner.snapshot.job_id.as_deref()==Some(&id){inner.cancel=None;inner.snapshot.status="failed".into();inner.snapshot.error=Some(format!("无法启动渲染线程：{e}"));}return Err(format!("无法启动渲染线程：{e}"));}
  Ok(job_id)
 }
 fn cancel(&self)->Result<(),String>{let mut inner=self.inner.lock().map_err(|_|"渲染状态锁不可用")?;let flag=inner.cancel.as_ref().ok_or("当前没有正在运行的渲染任务。")?;flag.store(true,Ordering::Release);inner.snapshot.status="cancelling".into();Ok(())}
}
#[derive(Clone,Debug)]
struct PreparedClip{input:RenderClip,path:PathBuf,start:u64,frames:u64}
#[derive(Debug)]
struct RenderPlan{recipe:Vec<RenderClip>,clips:Vec<PreparedClip>,sources:Vec<SourceStamp>,frames:u64}
fn checked_frames(seconds:f64)->Result<u64,String>{if !seconds.is_finite()||!(0.0..=MAX_SECONDS).contains(&seconds){return Err("编排时间必须是0至3600秒的有限数值。".into());}Ok((seconds*RATE as f64).round() as u64)}
fn identity(path:&Path)->Result<Identity,String>{let data=fs::symlink_metadata(path).map_err(err)?;if !data.is_file()||is_link(&data){return Err("只接受普通文件，不跟随链接。".into());}Ok(Identity{size:data.len(),modified:data.modified().map_err(err)?.duration_since(UNIX_EPOCH).map_err(err)?.as_nanos().to_string()})}
fn is_link(meta:&fs::Metadata)->bool{
 #[cfg(windows)]{use std::os::windows::fs::MetadataExt;meta.file_attributes()&0x400!=0}
 #[cfg(not(windows))]{meta.file_type().is_symlink()}
}
fn reject_links(path:&Path)->Result<(),String>{for ancestor in path.ancestors(){if let Ok(meta)=fs::symlink_metadata(ancestor){if is_link(&meta){return Err("不使用链接或目录联接作为渲染/导出路径。".into());}}}Ok(())}
fn cache_root(path:&Path)->Result<PathBuf,String>{if !path.is_absolute(){return Err("渲染缓存位置必须为绝对路径。".into());}reject_links(path)?;fs::create_dir_all(path).map_err(err)?;let meta=fs::symlink_metadata(path).map_err(err)?;if !meta.is_dir()||is_link(&meta){return Err("渲染缓存目录不可用。".into());}fs::canonicalize(path).map_err(err)}
fn source_stamp(catalog:&CatalogState,id:&str)->Result<SourceStamp,String>{
 let path=catalog.resolve_asset(id)?;let actual=identity(&path)?;
 let conn=rusqlite::Connection::open_with_flags(catalog.database_path(),rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(err)?;
 let (stored,size,modified):(String,u64,String)=conn.query_row("SELECT path,size,modified FROM assets WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(err)?;
 if Path::new(&stored)!=path||actual.size!=size||actual.modified!=modified{return Err("源素材已变化或移动，请先重新扫描，再重新渲染。".into());}
 Ok(SourceStamp{asset_id:id.into(),path:stored,size,modified})
}
fn validate_sources(catalog:&CatalogState,sources:&[SourceStamp])->Result<(),String>{for source in sources{if source_stamp(catalog,&source.asset_id)?!=*source{return Err("渲染来源已变化，请重新扫描并重新渲染。".into());}}Ok(())}
fn prepare(catalog:&CatalogState,clips:Vec<RenderClip>)->Result<RenderPlan,String>{
 if clips.is_empty()||clips.len()>MAX_CLIPS{return Err("请选择1至200个叠层片段。".into());}
 let recipe=clips.clone();let mut prepared=Vec::with_capacity(clips.len());let mut sources=HashMap::<String,SourceStamp>::new();let mut events=vec![];let mut total=0;
 for input in clips{
  if input.asset_id.is_empty()||input.asset_id.len()>128||!input.gain.is_finite()||!(0.0..=2.0).contains(&input.gain){return Err("素材ID或音量无效，音量限0至200%。".into());}
  let start=checked_frames(input.offset)?;let frames=checked_frames(input.duration)?;
  if frames==0||start+frames>RATE as u64*MAX_SECONDS as u64{return Err("片段时长应大于0，编排总长不能超过60分钟。".into());}
  let stamp=match sources.get(&input.asset_id){Some(v)=>v.clone(),None=>{let v=source_stamp(catalog,&input.asset_id)?;sources.insert(input.asset_id.clone(),v.clone());v}};
  let path=PathBuf::from(&stamp.path);let source=PlaybackSource::open(&path)?;
  if source.channels()==0||source.channels()>2{return Err("当前叠层仅支持单声道和双声道，多声道请先转换。".into());}
  if !(1..=RATE).contains(&source.sample_rate()){return Err("本版仅支持不高于48kHz的源音频，高采样率请先使用带限重采样转换。".into());}
  let duration=source.total_duration().ok_or("源素材时长未知，请先重新扫描或转换格式。")?.as_secs_f64();
  if !duration.is_finite()||input.duration>duration+1.0/RATE as f64{return Err("编排片段时长超过源音频实际时长，请更新素材并重新加入编排。".into());}
  total=total.max(start+frames);events.push((start,1i32));events.push((start+frames,-1));prepared.push(PreparedClip{input,path,start,frames});
 }
 events.sort_by_key(|&(frame,delta)|(frame,delta));let mut overlap=0i32;for (_,delta) in events{overlap+=delta;if overlap>MAX_OVERLAP as i32{return Err("同一时刻最多叠放32个片段，请减少重叠或使用散布。".into());}}
 prepared.sort_by_key(|clip|clip.start);let mut sources:Vec<_>=sources.into_values().collect();sources.sort_by(|a,b|a.asset_id.cmp(&b.asset_id));validate_sources(catalog,&sources)?;
 Ok(RenderPlan{recipe,clips:prepared,sources,frames:total})
}
fn check_capacity(root:&Path,bytes:u64)->Result<(),String>{
 let mut used=0u64;for entry in fs::read_dir(root).map_err(err)?{let entry=entry.map_err(err)?;let meta=fs::symlink_metadata(entry.path()).map_err(err)?;if is_link(&meta){return Err("渲染缓存内存在链接，未启动新任务。".into());}if meta.is_file(){used=used.saturating_add(meta.len());}}
 if used.saturating_add(bytes)>CACHE_LIMIT{return Err(format!("渲染缓存达到1.5GiB上限。请停止试听并关闭应用后清理此缓存目录，再重新渲染：{}",root.display()));}
 #[cfg(windows)] unsafe{
  use windows::{core::HSTRING,Win32::Storage::FileSystem::GetDiskFreeSpaceExW};
  let mut available=0;GetDiskFreeSpaceExW(&HSTRING::from(root.display().to_string()),Some(&mut available),None,None).map_err(err)?;
  if available<bytes.saturating_add(DISK_RESERVE){return Err("缓存磁盘剩余空间不足，请释放空间后再渲染。".into());}
 }
 Ok(())
}
struct StereoResampler{source:PlaybackSource,channels:u16,rate:u32,index:u64,output:u64,a:[f64;2],b:Option<[f64;2]>}
impl StereoResampler{
 fn new(path:&Path)->Result<Self,String>{let mut source=PlaybackSource::open(path)?;let channels=source.channels();let rate=source.sample_rate();if !(1..=2).contains(&channels)||rate==0{return Err("只支持单声道或双声道音频。".into());}let a=read_frame(&mut source,channels)?.ok_or("源素材没有可解码采样。")?;let b=read_frame(&mut source,channels)?;Ok(Self{source,channels,rate,index:0,output:0,a,b})}
 fn next(&mut self)->Result<[f64;2],String>{
  let position=self.output*self.rate as u64;let frame=position/RATE as u64;let fraction=(position%RATE as u64) as f64/RATE as f64;
  while self.index<frame{self.a=self.b.ok_or("源音频在编排时长之前结束，请重新扫描或检查损坏文件。")?;self.b=read_frame(&mut self.source,self.channels)?;self.index+=1;}
  if let Some(error)=self.source.decode_error(){return Err(format!("源素材解码失败：{error}"));}
  let b=self.b.unwrap_or(self.a);self.output+=1;Ok([self.a[0]+(b[0]-self.a[0])*fraction,self.a[1]+(b[1]-self.a[1])*fraction])
 }
}
fn read_frame(source:&mut PlaybackSource,channels:u16)->Result<Option<[f64;2]>,String>{
 let left=match source.next(){Some(v)=>v as f64,None=>{if let Some(error)=source.decode_error(){return Err(format!("源素材解码失败：{error}"));}return Ok(None)}};
 let right=if channels==1{left}else{source.next().ok_or("双声道源素材的末尾采样不完整。")? as f64};
 if !left.is_finite()||!right.is_finite(){return Err("源素材包含非有限采样，未生成输出。".into());}Ok(Some([left,right]))
}
struct Active{source:StereoResampler,end:u64,gain:f64}
struct Rendered{path:PathBuf,identity:Identity,peak:f64,clipped_samples:u64,retained:bool}
impl Drop for Rendered{fn drop(&mut self){if !self.retained{let _=fs::remove_file(&self.path);}}}
struct Partial(Option<PathBuf>);impl Drop for Partial{fn drop(&mut self){if let Some(path)=&self.0{let _=fs::remove_file(path);}}}
fn wave_header(frames:u64)->Result<[u8;44],String>{
 let bytes=frames.checked_mul(4).filter(|v|*v<=u32::MAX as u64-36).ok_or("WAV长度超出RIFF范围。")? as u32;let mut header=[0u8;44];
 header[0..4].copy_from_slice(b"RIFF");header[4..8].copy_from_slice(&(bytes+36).to_le_bytes());header[8..12].copy_from_slice(b"WAVE");header[12..16].copy_from_slice(b"fmt ");header[16..20].copy_from_slice(&16u32.to_le_bytes());header[20..22].copy_from_slice(&1u16.to_le_bytes());header[22..24].copy_from_slice(&CHANNELS.to_le_bytes());header[24..28].copy_from_slice(&RATE.to_le_bytes());header[28..32].copy_from_slice(&(RATE*4).to_le_bytes());header[32..34].copy_from_slice(&4u16.to_le_bytes());header[34..36].copy_from_slice(&16u16.to_le_bytes());header[36..40].copy_from_slice(b"data");header[40..44].copy_from_slice(&bytes.to_le_bytes());Ok(header)
}
fn render(root:&Path,job_id:&str,plan:&RenderPlan,cancel:&AtomicBool,mut progress:impl FnMut(u64))->Result<Rendered,String>{
 let partial=root.join(format!("{job_id}.partial"));let output=root.join(format!("{job_id}.wav"));let file=OpenOptions::new().write(true).create_new(true).open(&partial).map_err(err)?;let cleanup=Partial(Some(partial.clone()));let mut writer=BufWriter::with_capacity(64*1024,file);writer.write_all(&wave_header(plan.frames)?).map_err(err)?;
 let mut active=Vec::<Active>::new();let mut pending=0;let mut peak=0.0f64;let mut clipped=0u64;let mut bytes=Vec::with_capacity(BLOCK*4);let mut frame=0;
 while frame<plan.frames{
  if cancel.load(Ordering::Acquire){return Err("渲染已取消。".into());}bytes.clear();let stop=(frame+BLOCK as u64).min(plan.frames);
  while frame<stop{
   active.retain(|clip|clip.end>frame);
   while pending<plan.clips.len()&&plan.clips[pending].start==frame{let clip=&plan.clips[pending];active.push(Active{source:StereoResampler::new(&clip.path)?,end:clip.start+clip.frames,gain:clip.input.gain});pending+=1;}
   let mut mixed=[0.0f64;2];for clip in &mut active{let value=clip.source.next()?;mixed[0]+=value[0]*clip.gain;mixed[1]+=value[1]*clip.gain;}
   for sample in mixed{if !sample.is_finite(){return Err("混音结果包含非有限采样。".into());}peak=peak.max(sample.abs());if sample.abs()>1.0{clipped+=1;}let sample=sample.clamp(-1.0,1.0);let pcm=(sample*if sample>=0.0{32767.0}else{32768.0}).round() as i16;bytes.extend_from_slice(&pcm.to_le_bytes());}frame+=1;
  }
  writer.write_all(&bytes).map_err(err)?;progress(frame);
 }
 writer.flush().map_err(err)?;writer.get_ref().sync_all().map_err(err)?;drop(writer);
 if cancel.load(Ordering::Acquire){return Err("渲染已取消。".into());}
 let expected=44+plan.frames*4;if identity(&partial)?.size!=expected{return Err("渲染文件长度与预期采样帧数不符。".into());}
 let mut actual=[0u8;44];File::open(&partial).map_err(err)?.read_exact(&mut actual).map_err(err)?;if actual!=wave_header(plan.frames)?{return Err("渲染WAV文件头校验失败。".into());}
 rename_new(&partial,&output)?;let mut final_cleanup=Partial(Some(output.clone()));drop(cleanup);
 let result=Rendered{identity:identity(&output)?,path:output,peak,clipped_samples:clipped,retained:false};final_cleanup.0=None;Ok(result)
}
fn rename_new(from:&Path,to:&Path)->Result<(),String>{
 #[cfg(windows)] unsafe{use windows::{core::HSTRING,Win32::Storage::FileSystem::{MoveFileExW,MOVE_FILE_FLAGS}};MoveFileExW(&HSTRING::from(from.display().to_string()),&HSTRING::from(to.display().to_string()),MOVE_FILE_FLAGS(0)).map_err(err)}
 #[cfg(not(windows))]{fs::hard_link(from,to).map_err(err)?;fs::remove_file(from).map_err(err)}
}
#[derive(Serialize)]pub struct ExportResult{pub path:String,pub bytes:u64}
fn export(state:&LayerRenderState,job_id:&str,folder:&str)->Result<ExportResult,String>{
 let source=state.resolve_output(job_id)?;let expected=identity(&source)?;let destination=Path::new(folder);if !destination.is_absolute(){return Err("请选择导出目标文件夹的绝对路径。".into());}reject_links(destination)?;if !destination.is_dir(){return Err("导出目标文件夹不存在。".into());}let destination=fs::canonicalize(destination).map_err(err)?;let cache=cache_root(&state.root)?;if destination.starts_with(&cache){return Err("请选择渲染缓存目录之外的导出位置。".into());}
 let target=destination.join(format!("SoundWorkbench-Layer-{job_id}.wav"));let file=OpenOptions::new().write(true).create_new(true).open(&target).map_err(|e|format!("导出目标已存在或不可写，未覆盖：{e}"))?;
 let result=(||{let mut input=BufReader::new(File::open(&source).map_err(err)?);let mut output=BufWriter::new(file);let copied=std::io::copy(&mut input,&mut output).map_err(err)?;output.flush().map_err(err)?;output.get_ref().sync_all().map_err(err)?;if copied!=expected.size||identity(&source)?!=expected{return Err("复制过程中缓存发生变化，未交付导出。".into());}state.resolve_output(job_id)?;Ok(ExportResult{path:target.display().to_string(),bytes:copied})})();
 if result.is_err(){let _=fs::remove_file(&target);}result
}
#[tauri::command]
pub async fn layer_render_start(clips:Vec<RenderClip>,catalog:tauri::State<'_,CatalogState>,render:tauri::State<'_,LayerRenderState>)->Result<String,String>{let catalog=catalog.inner().clone();let render=render.inner().clone();tauri::async_runtime::spawn_blocking(move||render.start(clips,catalog)).await.map_err(err)?}
#[tauri::command]
pub fn layer_render_status(render:tauri::State<'_,LayerRenderState>)->Result<RenderSnapshot,String>{render.snapshot()}
#[tauri::command]
pub fn layer_render_cancel(render:tauri::State<'_,LayerRenderState>)->Result<(),String>{render.cancel()}
#[tauri::command]
pub async fn layer_render_export(job_id:String,folder:String,render:tauri::State<'_,LayerRenderState>)->Result<ExportResult,String>{let render=render.inner().clone();tauri::async_runtime::spawn_blocking(move||export(&render,&job_id,&folder)).await.map_err(err)?}


#[cfg(test)]mod tests{
 use super::*;
 use std::{thread,time::{Duration,Instant}};
 struct Fixture{root:PathBuf,catalog:Option<CatalogState>}
 impl Fixture{
  fn new()->Self{let root=std::env::temp_dir().join(format!("sound-workbench-layer-test-{}",Uuid::new_v4()));fs::create_dir(&root).unwrap();let root=fs::canonicalize(root).unwrap();let catalog=CatalogState::new(root.join("catalog.sqlite")).unwrap();Self{root,catalog:Some(catalog)}}
  fn catalog(&self)->&CatalogState{self.catalog.as_ref().unwrap()}
  fn source(&self,rate:u32,channels:u16,values:&[i16])->String{
   let path=self.root.join(format!("{}.wav",Uuid::new_v4()));let data=(values.len()*2) as u32;let mut header=wave_header(1).unwrap();header[4..8].copy_from_slice(&(data+36).to_le_bytes());header[22..24].copy_from_slice(&channels.to_le_bytes());header[24..28].copy_from_slice(&rate.to_le_bytes());header[28..32].copy_from_slice(&(rate*channels as u32*2).to_le_bytes());header[32..34].copy_from_slice(&(channels*2).to_le_bytes());header[40..44].copy_from_slice(&data.to_le_bytes());let mut file=File::create(&path).unwrap();file.write_all(&header).unwrap();for sample in values{file.write_all(&sample.to_le_bytes()).unwrap();}drop(file);self.register(&path,rate,channels)
  }
  fn register(&self,path:&Path,rate:u32,channels:u16)->String{
   let path=fs::canonicalize(path).unwrap();let meta=identity(&path).unwrap();let id=Uuid::new_v4().to_string();let conn=rusqlite::Connection::open(self.catalog().database_path()).unwrap();conn.execute("INSERT INTO assets(id,path_key,path,name,format,size,modified,rate,channels,duration,status,error) VALUES(?1,?2,?2,'test','WAV',?3,?4,?5,?6,?7,'ready',NULL)",rusqlite::params![id,path.display().to_string(),meta.size,meta.modified,rate,channels,crate::duration::seconds(&path)]).unwrap();id
  }
  fn cache(&self)->PathBuf{cache_root(&self.root.join("cache")).unwrap()}
  fn clip(&self,id:&str,duration:f64,offset:f64,gain:f64)->RenderClip{RenderClip{asset_id:id.into(),duration,offset,gain}}
  fn direct(&self,clips:Vec<RenderClip>)->Rendered{let plan=prepare(self.catalog(),clips).unwrap();render(&self.cache(),&Uuid::new_v4().to_string(),&plan,&AtomicBool::new(false),|_|{}).unwrap()}
 }
 impl Drop for Fixture{fn drop(&mut self){self.catalog.take();let _=fs::remove_dir_all(&self.root);}}
 fn samples(path:&Path)->Vec<i16>{let bytes=fs::read(path).unwrap();bytes[44..].chunks_exact(2).map(|b|i16::from_le_bytes([b[0],b[1]])).collect()}
 fn wait(state:&LayerRenderState)->RenderSnapshot{let end=Instant::now()+Duration::from_secs(10);loop{let value=state.snapshot().unwrap();if !["preparing","running","cancelling"].contains(&value.status.as_str()){return value;}assert!(Instant::now()<end,"render test timed out");thread::sleep(Duration::from_millis(2));}}
 #[test]fn layer_render_mix_offset_gain_and_pcm_header(){
  let f=Fixture::new();let id=f.source(8_000,1,&vec![8192;80]);let path=f.catalog().resolve_asset(&id).unwrap();let original=fs::read(&path).unwrap();
  let done=f.direct(vec![f.clip(&id,0.01,0.01,1.0),f.clip(&id,0.01,0.01,0.5)]);let values=samples(&done.path);
  assert_eq!(values.len(),960*2);assert!(values[..480*2].iter().all(|v|*v==0));assert!(values[480*2..].iter().all(|v|(*v-12288).abs()<=1));assert!((done.peak-0.375).abs()<1e-7);assert_eq!(done.clipped_samples,0);
  let bytes=fs::read(&done.path).unwrap();assert_eq!(&bytes[..44],&wave_header(960).unwrap());assert_eq!(bytes.len(),44+960*4);assert_eq!(fs::read(path).unwrap(),original);
 }
 #[test]fn layer_render_resamples_stereo_scatter_and_keeps_silent_gap(){
  let f=Fixture::new();let stereo:Vec<i16>=(0..240).flat_map(|_|[8192,-16384]).collect();let id=f.source(24_000,2,&stereo);let done=f.direct(vec![f.clip(&id,0.01,0.0,1.0),f.clip(&id,0.01,0.02,1.0)]);let values=samples(&done.path);
  assert_eq!(values.len(),1440*2);assert!(values[480*2..960*2].iter().all(|v|*v==0));for frame in values[..480*2].chunks_exact(2).chain(values[960*2..].chunks_exact(2)){assert!((frame[0]-8192).abs()<=1);assert_eq!(frame[1],-16384);}
 }
 #[test]fn layer_render_clipping_is_reported_without_normalization(){
  let f=Fixture::new();let values:Vec<i16>=(0..480).flat_map(|_|[24576,-24576]).collect();let id=f.source(RATE,2,&values);let done=f.direct(vec![f.clip(&id,0.01,0.0,2.0)]);assert_eq!(done.peak,1.5);assert_eq!(done.clipped_samples,960);
  for frame in samples(&done.path).chunks_exact(2){assert_eq!(frame,[32767,-32768]);}
 }
 #[test]fn layer_render_rejects_invalid_inputs_and_excess_overlap(){
  let f=Fixture::new();let id=f.source(RATE,1,&vec![1;480]);let normal=f.clip(&id,0.01,0.0,1.0);
  assert!(prepare(f.catalog(),vec![]).is_err());assert!(prepare(f.catalog(),vec![normal.clone();201]).is_err());assert!(prepare(f.catalog(),vec![normal.clone();33]).unwrap_err().contains("32"));
  for clip in [f.clip(&id,0.01,0.0,f64::NAN),f.clip(&id,0.01,-1.0,1.0),f.clip(&id,0.02,0.0,1.0),f.clip(&id,0.01,3600.0,1.0),f.clip("unknown",0.01,0.0,1.0)]{assert!(prepare(f.catalog(),vec![clip]).is_err());}
  let high=f.source(96_000,1,&vec![1;960]);assert!(prepare(f.catalog(),vec![f.clip(&high,0.01,0.0,1.0)]).unwrap_err().contains("48kHz"));
  let multi=f.source(RATE,4,&vec![1;480*4]);assert!(prepare(f.catalog(),vec![f.clip(&multi,0.01,0.0,1.0)]).unwrap_err().contains("多声道"));
 }
 #[test]fn layer_render_cancel_cleans_only_current_partial_and_keeps_source(){
  let f=Fixture::new();let id=f.source(RATE,1,&vec![300;RATE as usize]);let path=f.catalog().resolve_asset(&id).unwrap();let before=fs::read(&path).unwrap();let root=f.cache();let marker=root.join("keep.txt");fs::write(&marker,b"not owned by job").unwrap();let plan=prepare(f.catalog(),vec![f.clip(&id,1.0,0.0,1.0)]).unwrap();let cancel=AtomicBool::new(false);
  assert!(render(&root,&Uuid::new_v4().to_string(),&plan,&cancel,|_|cancel.store(true,Ordering::Release)).is_err());assert_eq!(fs::read_dir(&root).unwrap().count(),1);assert_eq!(fs::read(marker).unwrap(),b"not owned by job");assert_eq!(fs::read(path).unwrap(),before);
 }
 #[test]fn layer_render_completed_result_export_and_stale_sources_are_checked(){
  let f=Fixture::new();let id=f.source(RATE,1,&vec![1000;480]);let state=LayerRenderState::new(f.cache());let recipe=vec![f.clip(&id,0.01,0.0,1.0),f.clip(&id,0.01,0.02,1.0),f.clip(&id,0.01,0.0,0.5)];let job=state.start(recipe.clone(),f.catalog().clone()).unwrap();let status=wait(&state);assert_eq!(status.clips,recipe);assert_eq!(status.status,"completed","{:?}",status.error);assert_eq!(status.progress,1.0);let path=state.resolve_output(&job).unwrap();assert_eq!(status.bytes,44+1440*4);
  let destination=f.root.join("export");fs::create_dir(&destination).unwrap();let first=export(&state,&job,&destination.display().to_string()).unwrap();assert_eq!(fs::read(&first.path).unwrap(),fs::read(&path).unwrap());assert!(export(&state,&job,&destination.display().to_string()).is_err());
  assert!(state.resolve_output("unknown").is_err());let source=f.catalog().resolve_asset(&id).unwrap();OpenOptions::new().append(true).open(source).unwrap().write_all(&[0]).unwrap();assert!(state.resolve_output(&job).is_err());assert_eq!(fs::read(&first.path).unwrap(),fs::read(path).unwrap());
 }
 #[test]fn layer_render_nonfinite_and_short_sources_never_succeed(){
  let f=Fixture::new();let path=f.root.join("nonfinite.wav");let mut header=wave_header(480).unwrap();header[20..22].copy_from_slice(&3u16.to_le_bytes());header[22..24].copy_from_slice(&1u16.to_le_bytes());header[34..36].copy_from_slice(&32u16.to_le_bytes());
  {let mut file=File::create(&path).unwrap();file.write_all(&header).unwrap();for _ in 0..480{file.write_all(&f32::NAN.to_le_bytes()).unwrap();}}
  let id=f.register(&path,RATE,1);let state=LayerRenderState::new(f.cache());let _=state.start(vec![f.clip(&id,0.01,0.0,1.0)],f.catalog().clone());let status=wait(&state);assert_eq!(status.status,"failed");assert!(status.error.unwrap().contains("非有限"));assert_eq!(fs::read_dir(f.cache()).unwrap().count(),0);
  let path=f.root.join("short.wav");let mut header=wave_header(480).unwrap();header[22..24].copy_from_slice(&1u16.to_le_bytes());header[28..32].copy_from_slice(&(RATE*2).to_le_bytes());header[32..34].copy_from_slice(&2u16.to_le_bytes());
  {let mut file=File::create(&path).unwrap();file.write_all(&header).unwrap();file.write_all(&vec![0;480*2]).unwrap();}
  let id=f.register(&path,RATE,1);let _=state.start(vec![f.clip(&id,0.02,0.0,1.0)],f.catalog().clone());assert_eq!(wait(&state).status,"failed");assert_eq!(fs::read_dir(f.cache()).unwrap().count(),0);
 }
 #[test]fn layer_render_panic_partial_cleanup_and_resource_limits(){
  let f=Fixture::new();let id=f.source(RATE,1,&vec![300;RATE as usize]);let plan=prepare(f.catalog(),vec![f.clip(&id,1.0,0.0,1.0)]).unwrap();let root=f.cache();
  let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||render(&root,&Uuid::new_v4().to_string(),&plan,&AtomicBool::new(false),|_|panic!("injected worker failure"))));assert!(result.is_err());assert_eq!(fs::read_dir(&root).unwrap().count(),0);
  assert!(wave_header(u64::MAX).is_err());assert!(check_capacity(&root,CACHE_LIMIT+1).is_err());
 }
 #[test]fn layer_render_cache_tamper_unknown_job_and_four_formats(){
  let f=Fixture::new();let fresh=LayerRenderState::new(f.root.join("never-created"));assert!(fresh.resolve_output("unknown").is_err());assert!(!fresh.root.exists());
  for name in ["pcm-44100.wav","pcm-48000.aiff","lossless-48000.flac","lossy-44100.mp3"]{
   let source=Path::new(env!("CARGO_MANIFEST_DIR")).join("../build/s1-samples").join(name);let original=fs::read(&source).unwrap();let id=f.register(&source,44_100,1);let done=f.direct(vec![f.clip(&id,0.5,0.0,0.5)]);assert_eq!(samples(&done.path).len(),24_000*2);assert!(done.peak>0.01&&done.peak<0.1,"{name}: {}",done.peak);assert_eq!(fs::read(source).unwrap(),original);
  }
  let id=f.source(RATE,1,&vec![1000;480]);let state=LayerRenderState::new(f.cache());let job=state.start(vec![f.clip(&id,0.01,0.0,1.0)],f.catalog().clone()).unwrap();assert_eq!(wait(&state).status,"completed");let path=state.resolve_output(&job).unwrap();OpenOptions::new().append(true).open(path).unwrap().write_all(&[0]).unwrap();assert!(state.resolve_output(&job).is_err());
 }
}
