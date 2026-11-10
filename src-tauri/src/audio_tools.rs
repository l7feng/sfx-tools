//! 原路径素材的非破坏音频处理。只创建受控派生WAV，不写回原音频。
use crate::{catalog::CatalogState,player::{PlaybackSource,PlayerState,PlayerSnapshot}};
use rodio::Source;
use serde::{Serialize,Deserialize};
use std::{collections::VecDeque,fs::{self,File,OpenOptions},io::{Read,Write,Seek,SeekFrom,BufWriter},path::{Path,PathBuf},sync::{Arc,Mutex,atomic::{AtomicBool,Ordering}},time::{Duration,UNIX_EPOCH}};
use uuid::Uuid;
const MAX_SECONDS:f64=600.0;
const CACHE_LIMIT:u64=1536*1024*1024;
const BLOCK:usize=2048;
fn err(e:impl std::fmt::Display)->String{e.to_string()}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct ToolOptions{#[serde(default="one")]pub speed:f64,#[serde(default)]pub distance:f64,#[serde(default="gain_name")]pub effect:String,#[serde(default="one")]pub amount:f64,#[serde(default="cutoff_default")]pub cutoff:f64,#[serde(default="delay_default")]pub delay_ms:f64,#[serde(default="wet_default")]pub wet:f64}
fn one()->f64{1.0}fn gain_name()->String{"gain".into()}fn cutoff_default()->f64{4000.0}fn delay_default()->f64{200.0}fn wet_default()->f64{0.25}
impl Default for ToolOptions{fn default()->Self{Self{speed:one(),distance:0.0,effect:gain_name(),amount:one(),cutoff:cutoff_default(),delay_ms:delay_default(),wet:wet_default()}}}
#[derive(Clone,Debug,Serialize)]pub struct ToolOutput{pub name:String,pub path:String,pub channels:u16,pub rate:u32,pub duration:f64,pub bytes:u64}
#[derive(Clone,Debug,Serialize)]pub struct ToolSnapshot{pub job_id:Option<String>,pub status:String,pub progress:f64,pub operation:String,pub asset_id:String,pub outputs:Vec<ToolOutput>,pub peak:f64,pub clipped_samples:u64,pub error:Option<String>}
impl Default for ToolSnapshot{fn default()->Self{Self{job_id:None,status:"idle".into(),progress:0.0,operation:String::new(),asset_id:String::new(),outputs:vec![],peak:0.0,clipped_samples:0,error:None}}}
#[derive(Clone,Debug,PartialEq)]struct Identity{size:u64,modified:String}
#[derive(Clone,Debug,PartialEq)]struct Stamp{id:String,path:PathBuf,identity:Identity}
#[derive(Clone)]struct Complete{job_id:String,outputs:Vec<ToolOutput>,identities:Vec<Identity>,source:Stamp,catalog:CatalogState}
struct Inner{snapshot:ToolSnapshot,cancel:Option<Arc<AtomicBool>>,complete:VecDeque<Complete>}
#[derive(Clone)]pub struct AudioToolsState{root:PathBuf,inner:Arc<Mutex<Inner>>}
impl AudioToolsState{
 pub fn new(root:PathBuf)->Self{Self{root,inner:Arc::new(Mutex::new(Inner{snapshot:Default::default(),cancel:None,complete:VecDeque::new()}))}}
 fn snapshot(&self)->Result<ToolSnapshot,String>{Ok(self.inner.lock().map_err(|_|"音频工具状态锁不可用")?.snapshot.clone())}
 fn start(&self,id:String,operation:String,options:ToolOptions,catalog:CatalogState)->Result<String,String>{
  validate(&operation,&options)?;let job_id=Uuid::new_v4().to_string();let cancel=Arc::new(AtomicBool::new(false));
  {let mut inner=self.inner.lock().map_err(|_|"音频工具状态锁不可用")?;if inner.cancel.is_some(){return Err("已有音频处理运行，请等待或取消。".into());}inner.snapshot=ToolSnapshot{job_id:Some(job_id.clone()),status:"preparing".into(),operation:operation.clone(),asset_id:id.clone(),..Default::default()};inner.cancel=Some(cancel.clone());}
  let preparation=(||{let stamp=source_stamp(&catalog,&id)?;let source=PlaybackSource::open(&stamp.path)?;let rate=source.sample_rate();let channels=source.channels();if !(1..=2).contains(&channels)||!(1..=192000).contains(&rate){return Err("本版音频工具支持不高于192kHz的单/双声道素材。".into());}let duration=source.total_duration().ok_or("源时长未知，请先扫描或转换素材。")?.as_secs_f64();if !duration.is_finite()||duration<=0.0||duration>MAX_SECONDS{return Err("本版单次处理支持不超过10分钟的音频。".into());}let root=cache_root(&self.root)?;let tail=if operation=="effect"&&options.effect=="echo"&&options.wet>0.0{(options.delay_ms/1000.0*rate as f64).round() as u64}else{0};let estimate=(duration*rate as f64).ceil() as u64*channels as u64*6+tail*channels as u64*2+256;capacity(&root,estimate)?;Ok::<_,String>((stamp,root,rate,channels))})();
  let (stamp,root,rate,channels)=match preparation{Ok(value)=>value,Err(message)=>{let mut inner=self.inner.lock().map_err(|_|"音频工具状态锁不可用")?;inner.cancel=None;inner.snapshot.status="failed".into();inner.snapshot.error=Some(message.clone());return Err(message);}};
  if cancel.load(Ordering::Acquire){let mut inner=self.inner.lock().map_err(|_|"音频工具状态锁不可用")?;inner.cancel=None;inner.snapshot.status="cancelled".into();return Err("音频处理准备已取消。".into());}
  self.inner.lock().map_err(|_|"音频工具状态锁不可用")?.snapshot.status="running".into();let state=self.clone();let worker_id=job_id.clone();
  let spawned=std::thread::Builder::new().name("sound-workbench-audio-tools".into()).spawn(move||{
   let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{let done=process(&root,&worker_id,&stamp,&operation,&options,rate,channels,&cancel,|progress|{if let Ok(mut inner)=state.inner.lock(){inner.snapshot.progress=progress;}})?;if source_stamp(&catalog,&stamp.id)?!=stamp{return Err("处理期间源素材发生变化，未交付结果。".into());}Ok(done)})).unwrap_or_else(|_|Err("音频处理线程异常，未交付结果。".into()));
   let mut inner=match state.inner.lock(){Ok(v)=>v,Err(_)=>return};let cancelled=cancel.load(Ordering::Acquire);
   match result{Ok(mut done) if !cancelled=>{let identities=done.outputs.iter().map(|o|identity(Path::new(&o.path))).collect::<Result<Vec<_>,_>>();match identities{Ok(identities)=>{inner.snapshot.status="completed".into();inner.snapshot.progress=1.0;inner.snapshot.outputs=done.outputs.clone();inner.snapshot.peak=done.peak;inner.snapshot.clipped_samples=done.clipped;inner.complete.push_back(Complete{job_id:worker_id,outputs:done.outputs.clone(),identities,source:stamp,catalog});while inner.complete.len()>256{inner.complete.pop_front();}done.retain_outputs();},Err(message)=>{inner.snapshot.status="failed".into();inner.snapshot.error=Some(message);}}},Ok(_)=>{inner.snapshot.status="cancelled".into();},Err(message)=>{inner.snapshot.status=if cancelled{"cancelled"}else{"failed"}.into();inner.snapshot.error=if cancelled{None}else{Some(message)};}}inner.cancel=None;
  });
  if let Err(e)=spawned{let mut inner=self.inner.lock().map_err(|_|"音频工具状态锁不可用")?;inner.cancel=None;inner.snapshot.status="failed".into();inner.snapshot.error=Some(format!("无法启动音频工具：{e}"));return Err(e.to_string());}Ok(job_id)
 }
 fn cancel(&self)->Result<(),String>{let mut inner=self.inner.lock().map_err(|_|"音频工具状态锁不可用")?;inner.cancel.as_ref().ok_or("没有正在运行的音频处理。")?.store(true,Ordering::Release);inner.snapshot.status="cancelling".into();Ok(())}
 fn completed(&self,job_id:&str)->Result<Complete,String>{self.inner.lock().map_err(|_|"音频工具状态锁不可用")?.complete.iter().find(|item|item.job_id==job_id).cloned().ok_or("该处理结果未完成或不属于当前会话，请重新处理。".into())}
 pub fn resolve_output(&self,job_id:&str,index:usize)->Result<PathBuf,String>{let done=self.completed(job_id)?;let output=done.outputs.get(index).ok_or("声道结果不存在。")?;let path=PathBuf::from(&output.path);let root=cache_root(&self.root)?;if path.parent()!=Some(root.as_path())||path.file_name().and_then(|v|v.to_str())!=Some(&format!("{job_id}-{index}.wav")){return Err("处理缓存路径无效。".into());}reject_links(&path)?;if identity(&path)?!=done.identities[index]||source_stamp(&done.catalog,&done.source.id)?!=done.source{return Err("处理缓存或原素材已变化，请重新处理。".into());}Ok(path)}
}
fn validate(operation:&str,o:&ToolOptions)->Result<(),String>{
 if !["reverse","mono_merge","mono_split","distance","effect","speed"].contains(&operation){return Err("未知音频处理操作。".into());}
 for (value,min,max,name) in [(o.speed,0.5,2.0,"变速"),(o.distance,0.0,50.0,"距离"),(o.amount,0.0,2.0,"增益"),(o.cutoff,20.0,20000.0,"滤波频率"),(o.delay_ms,10.0,1000.0,"延迟"),(o.wet,0.0,1.0,"湿声")]{if !value.is_finite()||value<min||value>max{return Err(format!("{name}参数超出允许范围。"));}}
 if !["gain","lowpass","highpass","echo"].contains(&o.effect.as_str()){return Err("未知内置效果。".into());}Ok(())
}
fn is_link(meta:&fs::Metadata)->bool{#[cfg(windows)]{use std::os::windows::fs::MetadataExt;meta.file_attributes()&0x400!=0}#[cfg(not(windows))]{meta.file_type().is_symlink()}}
fn identity(path:&Path)->Result<Identity,String>{let meta=fs::symlink_metadata(path).map_err(err)?;if !meta.is_file()||is_link(&meta){return Err("只处理普通文件，不跟随链接。".into());}Ok(Identity{size:meta.len(),modified:meta.modified().map_err(err)?.duration_since(UNIX_EPOCH).map_err(err)?.as_nanos().to_string()})}
fn reject_links(path:&Path)->Result<(),String>{for parent in path.ancestors(){if let Ok(meta)=fs::symlink_metadata(parent){if is_link(&meta){return Err("处理或导出位置包含目录联接/链接，未执行。".into());}}}Ok(())}
fn cache_root(root:&Path)->Result<PathBuf,String>{if !root.is_absolute(){return Err("音频工具缓存必须是绝对路径。".into());}reject_links(root)?;fs::create_dir_all(root).map_err(err)?;if !fs::metadata(root).map_err(err)?.is_dir(){return Err("音频工具缓存不可用。".into());}fs::canonicalize(root).map_err(err)}
fn source_stamp(catalog:&CatalogState,id:&str)->Result<Stamp,String>{let path=catalog.resolve_asset(id)?;let actual=identity(&path)?;let conn=rusqlite::Connection::open_with_flags(catalog.database_path(),rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(err)?;let (stored,size,modified):(String,u64,String)=conn.query_row("SELECT path,size,modified FROM assets WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(err)?;if Path::new(&stored)!=path||actual.size!=size||actual.modified!=modified{return Err("素材已变化，请重新扫描后再处理。".into());}Ok(Stamp{id:id.into(),path,identity:actual})}
fn capacity(root:&Path,expected:u64)->Result<(),String>{let mut used=0u64;for entry in fs::read_dir(root).map_err(err)?{let meta=fs::symlink_metadata(entry.map_err(err)?.path()).map_err(err)?;if is_link(&meta){return Err("工具缓存包含链接，已停止。".into());}if meta.is_file(){used=used.saturating_add(meta.len());}}if used.saturating_add(expected)>CACHE_LIMIT{return Err(format!("音频工具缓存达1.5GiB上限，请停止试听并关闭程序后清理：{}",root.display()));}#[cfg(windows)]unsafe{use windows::{core::HSTRING,Win32::Storage::FileSystem::GetDiskFreeSpaceExW};let mut free=0;GetDiskFreeSpaceExW(&HSTRING::from(root.display().to_string()),Some(&mut free),None,None).map_err(err)?;if free<expected.saturating_add(16*1024*1024){return Err("工具缓存磁盘空间不足。".into());}}Ok(())}
struct Cleanup{paths:Vec<PathBuf>}
impl Drop for Cleanup{fn drop(&mut self){for path in &self.paths{let _=fs::remove_file(path);}}}
struct Processed{outputs:Vec<ToolOutput>,peak:f64,clipped:u64,cleanup:Cleanup}
impl Processed{fn retain_outputs(&mut self){let outputs=self.outputs.iter().map(|o|PathBuf::from(&o.path)).collect::<Vec<_>>();self.cleanup.paths.retain(|p|!outputs.contains(p));}}
fn header(frames:u64,channels:u16,rate:u32)->Result<[u8;44],String>{let bytes=frames.checked_mul(channels as u64*2).filter(|v|*v<=u32::MAX as u64-36).ok_or("输出超出WAV RIFF范围。")? as u32;let mut h=[0u8;44];h[0..4].copy_from_slice(b"RIFF");h[4..8].copy_from_slice(&(36+bytes).to_le_bytes());h[8..12].copy_from_slice(b"WAVE");h[12..16].copy_from_slice(b"fmt ");h[16..20].copy_from_slice(&16u32.to_le_bytes());h[20..22].copy_from_slice(&1u16.to_le_bytes());h[22..24].copy_from_slice(&channels.to_le_bytes());h[24..28].copy_from_slice(&rate.to_le_bytes());h[28..32].copy_from_slice(&(rate*channels as u32*2).to_le_bytes());h[32..34].copy_from_slice(&(channels*2).to_le_bytes());h[34..36].copy_from_slice(&16u16.to_le_bytes());h[36..40].copy_from_slice(b"data");h[40..44].copy_from_slice(&bytes.to_le_bytes());Ok(h)}
fn cancelled(flag:&AtomicBool)->Result<(),String>{if flag.load(Ordering::Acquire){Err("音频处理已取消。".into())}else{Ok(())}}
fn process(root:&Path,job_id:&str,source:&Stamp,operation:&str,options:&ToolOptions,rate:u32,channels:u16,cancel:&AtomicBool,mut progress:impl FnMut(f64))->Result<Processed,String>{
 validate(operation,options)?;let raw_path=root.join(format!("{job_id}.raw.partial"));let file=OpenOptions::new().create_new(true).write(true).open(&raw_path).map_err(err)?;let mut cleanup=Cleanup{paths:vec![raw_path.clone()]};let mut raw=BufWriter::with_capacity(65536,file);let mut decoder=PlaybackSource::open(&source.path)?;if decoder.channels()!=channels||decoder.sample_rate()!=rate{return Err("源音频规格发生变化。".into());}let expected=decoder.total_duration().ok_or("源时长未知")?.as_secs_f64();let expected_frames=(expected*rate as f64).round() as u64;let mut frames=0u64;
 loop{cancelled(cancel)?;let first=match decoder.next(){Some(value)=>value,None=>break};if !first.is_finite(){return Err("源音频含非有限采样。".into());}raw.write_all(&first.to_le_bytes()).map_err(err)?;for _ in 1..channels{let value=decoder.next().ok_or("源声道采样不完整。")?;if !value.is_finite(){return Err("源音频含非有限采样。".into());}raw.write_all(&value.to_le_bytes()).map_err(err)?;}frames+=1;if frames>rate as u64*MAX_SECONDS as u64{return Err("源音频超过10分钟处理上限。".into());}if frames%BLOCK as u64==0{progress((frames as f64/expected_frames.max(1) as f64*0.45).min(0.45));}}
 if let Some(error)=decoder.decode_error(){return Err(format!("音频解码失败：{error}"));}if frames==0||frames.abs_diff(expected_frames)>1{return Err("有效采样长度与源时长不一致，源文件可能损坏。".into());}raw.flush().map_err(err)?;raw.get_ref().sync_all().map_err(err)?;drop(raw);drop(decoder);cancelled(cancel)?;
 let echo_tail=if operation=="effect"&&options.effect=="echo"&&options.wet>0.0{((options.delay_ms/1000.0*rate as f64).round() as u64).max(1)}else{0};let output_frames=frames+echo_tail;let output_channels=if ["mono_merge","mono_split"].contains(&operation){1}else{channels};let output_count=if operation=="mono_split"{channels as usize}else{1};let output_rate=if operation=="speed"{(rate as f64*options.speed).round() as u32}else{rate};let mut writers=Vec::with_capacity(output_count);let mut outputs=Vec::with_capacity(output_count);let mut finals=Vec::with_capacity(output_count);
 for index in 0..output_count{let partial=root.join(format!("{job_id}-{index}.partial"));let target=root.join(format!("{job_id}-{index}.wav"));let file=OpenOptions::new().create_new(true).write(true).open(&partial).map_err(err)?;cleanup.paths.push(partial.clone());let mut writer=BufWriter::with_capacity(65536,file);writer.write_all(&header(output_frames,output_channels,output_rate)?).map_err(err)?;writers.push(writer);let name=if operation=="mono_split"{if channels==2{if index==0{"左声道"}else{"右声道"}}else{"单声道"}}else{match operation{"reverse"=>"反转","mono_merge"=>"单声道合并","distance"=>"距离感","effect"=>"内置效果","speed"=>"变速",_=>"处理结果"}};outputs.push(ToolOutput{name:name.into(),path:target.display().to_string(),channels:output_channels,rate:output_rate,duration:output_frames as f64/output_rate as f64,bytes:44+output_frames*output_channels as u64*2});finals.push((partial,target));}
 let mut reader=File::open(&raw_path).map_err(err)?;let mut previous=[0.0f64;2];let mut last_input=[0.0f64;2];let cutoff=if operation=="distance"{18000.0/(1.0+options.distance*0.2)}else{options.cutoff};let alpha=1.0-(-2.0*std::f64::consts::PI*cutoff.min(rate as f64*0.45)/rate as f64).exp();let attenuation=1.0/(1.0+options.distance*0.15);let delay_frames=((options.delay_ms/1000.0*rate as f64).round() as usize).max(1);let mut delay=if operation=="effect"&&options.effect=="echo"{vec![[0.0f64;2];delay_frames]}else{vec![]};let mut peak=0.0f64;let mut clipped=0u64;let mut done_frames=0u64;let mut bytes=vec![0u8;BLOCK*channels as usize*4];
 while done_frames<output_frames{cancelled(cancel)?;let count=(output_frames-done_frames).min(BLOCK as u64) as usize;let available=frames.saturating_sub(done_frames).min(count as u64) as usize;bytes[..count*channels as usize*4].fill(0);if available>0{let start=if operation=="reverse"{frames-done_frames-count as u64}else{done_frames};reader.seek(SeekFrom::Start(start*channels as u64*4)).map_err(err)?;reader.read_exact(&mut bytes[..available*channels as usize*4]).map_err(err)?;}
  for index in 0..count{let input_index=if operation=="reverse"{count-1-index}else{index};let mut values=[0.0f64;2];for channel in 0..channels as usize{let offset=(input_index*channels as usize+channel)*4;values[channel]=f32::from_le_bytes(bytes[offset..offset+4].try_into().unwrap()) as f64;}
   if operation=="mono_merge"{values[0]=values[..channels as usize].iter().sum::<f64>()/channels as f64;}
   else if operation=="distance"&&options.distance>0.0{for channel in 0..channels as usize{previous[channel]+=alpha*(values[channel]-previous[channel]);values[channel]=previous[channel]*attenuation;}}
   else if operation=="effect"{for channel in 0..channels as usize{let input=values[channel];values[channel]=match options.effect.as_str(){"gain"=>input*options.amount,"lowpass"=>{previous[channel]+=alpha*(input-previous[channel]);previous[channel]},"highpass"=>{let value=(1.0-alpha)*(previous[channel]+input-last_input[channel]);last_input[channel]=input;previous[channel]=value;value},"echo"=>{let position=(done_frames as usize+index)%delay_frames;let past=delay[position][channel];delay[position][channel]=input;input*(1.0-options.wet)+past*options.wet},_=>unreachable!()};}}
   for output in 0..output_count{for channel in 0..output_channels as usize{let sample=if operation=="mono_split"{values[output]}else{values[channel]};if !sample.is_finite(){return Err("处理结果含非有限采样。".into());}peak=peak.max(sample.abs());if sample.abs()>1.0{clipped+=1;}let clamped=sample.clamp(-1.0,1.0);let pcm=(clamped*if clamped>=0.0{32767.0}else{32768.0}).round() as i16;writers[output].write_all(&pcm.to_le_bytes()).map_err(err)?;}}
  }done_frames+=count as u64;progress(0.45+done_frames as f64/output_frames as f64*0.54);
 }
 for writer in &mut writers{writer.flush().map_err(err)?;writer.get_ref().sync_all().map_err(err)?;}drop(writers);drop(reader);cancelled(cancel)?;
 for ((partial,target),output) in finals.iter().zip(&outputs){let mut h=[0u8;44];File::open(partial).map_err(err)?.read_exact(&mut h).map_err(err)?;if h!=header(output_frames,output_channels,output_rate)?||identity(partial)?.size!=output.bytes{return Err("处理WAV头或采样长度核验失败。".into());}rename_new(partial,target)?;cleanup.paths.push(target.clone());}
 Ok(Processed{outputs,peak,clipped,cleanup})
}
fn rename_new(from:&Path,to:&Path)->Result<(),String>{#[cfg(windows)]unsafe{use windows::{core::HSTRING,Win32::Storage::FileSystem::{MoveFileExW,MOVE_FILE_FLAGS}};MoveFileExW(&HSTRING::from(from.display().to_string()),&HSTRING::from(to.display().to_string()),MOVE_FILE_FLAGS(0)).map_err(err)}#[cfg(not(windows))]{fs::hard_link(from,to).map_err(err)?;fs::remove_file(from).map_err(err)}}
#[derive(Serialize)]pub struct ExportResult{pub paths:Vec<String>,pub bytes:u64}
fn export(state:&AudioToolsState,job_id:&str,folder:&str)->Result<ExportResult,String>{let complete=state.completed(job_id)?;let destination=Path::new(folder);if !destination.is_absolute(){return Err("请选择绝对路径目标目录。".into());}reject_links(destination)?;if !destination.is_dir(){return Err("导出目标目录不存在。".into());}let destination=fs::canonicalize(destination).map_err(err)?;if destination.starts_with(cache_root(&state.root)?){return Err("请选择缓存以外的导出目录。".into());}let mut cleanup=Cleanup{paths:vec![]};let mut paths=vec![];let mut total=0;
 for index in 0..complete.outputs.len(){let source=state.resolve_output(job_id,index)?;let target=destination.join(format!("SoundWorkbench-Tool-{job_id}-{index}.wav"));let file=OpenOptions::new().write(true).create_new(true).open(&target).map_err(|e|format!("导出目标已存在或不可写，未覆盖：{e}"))?;cleanup.paths.push(target.clone());let mut output=BufWriter::new(file);let copied=std::io::copy(&mut File::open(&source).map_err(err)?,&mut output).map_err(err)?;output.flush().map_err(err)?;output.get_ref().sync_all().map_err(err)?;drop(output);if copied!=complete.identities[index].size{ return Err("导出长度发生变化，未交付结果。".into());}state.resolve_output(job_id,index)?;total+=copied;paths.push(target.display().to_string());}cleanup.paths.clear();Ok(ExportResult{paths,bytes:total})}
#[tauri::command]pub async fn audio_tools_start(id:String,operation:String,options:ToolOptions,catalog:tauri::State<'_,CatalogState>,tools:tauri::State<'_,AudioToolsState>)->Result<String,String>{let catalog=catalog.inner().clone();let tools=tools.inner().clone();tauri::async_runtime::spawn_blocking(move||tools.start(id,operation,options,catalog)).await.map_err(err)?}
#[tauri::command]pub fn audio_tools_status(tools:tauri::State<'_,AudioToolsState>)->Result<ToolSnapshot,String>{tools.snapshot()}
#[tauri::command]pub fn audio_tools_cancel(tools:tauri::State<'_,AudioToolsState>)->Result<(),String>{tools.cancel()}
#[tauri::command]pub async fn audio_tools_preview(job_id:String,index:Option<usize>,tools:tauri::State<'_,AudioToolsState>,player:tauri::State<'_,PlayerState>)->Result<PlayerSnapshot,String>{let tools=tools.inner().clone();let player=player.inner().clone();tauri::async_runtime::spawn_blocking(move||{let index=index.unwrap_or(0);let path=tools.resolve_output(&job_id,index)?;player.play_derived(format!("tool:{job_id}:{index}"),path)}).await.map_err(err)?}
#[tauri::command]pub async fn audio_tools_export(job_id:String,folder:String,tools:tauri::State<'_,AudioToolsState>)->Result<ExportResult,String>{let tools=tools.inner().clone();tauri::async_runtime::spawn_blocking(move||export(&tools,&job_id,&folder)).await.map_err(err)?}

// 波形片段导出：按选区复制为独立 PCM16 WAV，原音频只读；同名文件不覆盖。
const MAX_CLIP_SECONDS:f64=600.0;
fn clip_bounds(start:f64,end:f64,total:f64)->Result<(f64,f64),String>{
 if !start.is_finite()||!end.is_finite(){return Err("片段起止时间无效。".into());}
 if start<0.0||end<=start{return Err("片段起止时间无效。".into());}
 if end-start>MAX_CLIP_SECONDS{return Err("单次片段导出不超过10分钟。".into());}
 if !total.is_finite()||total<=0.0{return Err("源时长无效。".into());}
 if start>=total{return Err("片段起点超出音频时长。".into());}
 if end>total+0.02{return Err("片段终点超出音频时长。".into());}
 Ok((start,end.min(total)))
}
fn clip_name(source:&Path,start:f64,end:f64)->String{
 let raw=source.file_stem().and_then(|value|value.to_str()).unwrap_or("clip");
 let stem:String=raw.chars().map(|c|if c.is_alphanumeric()||matches!(c,'-'|'_'|' '|'('|')'){c}else{'_'}).take(60).collect();
 format!("{}-片段-{start:.2}s-{end:.2}s.wav",if stem.trim().is_empty(){"clip"}else{stem.trim()})
}
fn clip_export(id:&str,start:f64,end:f64,folder:&str,catalog:&CatalogState)->Result<ExportResult,String>{
 let stamp=source_stamp(catalog,id)?;
 let destination=Path::new(folder);
 if !destination.is_absolute(){return Err("请选择绝对路径目标目录。".into());}
 reject_links(destination)?;
 if !destination.is_dir(){return Err("导出目标目录不存在。".into());}
 let destination=fs::canonicalize(destination).map_err(err)?;
 let mut source=PlaybackSource::open(&stamp.path)?;
 let rate=source.sample_rate();let channels=source.channels();
 if !(1..=2).contains(&channels)||!(1..=192000).contains(&rate){return Err("片段导出支持不高于192kHz的单/双声道素材。".into());}
 let total=source.total_duration().ok_or("源时长未知，请先扫描或转换素材。")?.as_secs_f64();
 let (start,end)=clip_bounds(start,end,total)?;
 let frames=((end-start)*rate as f64).round() as u64;
 if frames==0{return Err("片段长度不足一个采样。".into());}
 source.try_seek(Duration::from_secs_f64(start)).map_err(|e|format!("无法定位到片段起点：{e}"))?;
 let target=destination.join(clip_name(&stamp.path,start,end));
 if target.exists(){return Err("导出目标同名文件已存在，未覆盖。".into());}
 let partial=destination.join(format!(".{}.{}.partial",clip_name(&stamp.path,start,end),Uuid::new_v4()));
 let mut cleanup=Cleanup{paths:vec![partial.clone()]};
 let file=OpenOptions::new().create_new(true).write(true).open(&partial).map_err(err)?;
 let mut writer=BufWriter::with_capacity(65536,file);
 writer.write_all(&header(frames,channels,rate)?).map_err(err)?;
 let mut buffer=vec![0u8;BLOCK*channels as usize*2];let mut written=0u64;
 while written<frames{
  let count=(frames-written).min(BLOCK as u64) as usize;
  for index in 0..count{for channel in 0..channels as usize{
   let value=source.next().ok_or("片段解码到文件末尾，源时长可能与文件头不一致。")? as f64;
   if !value.is_finite(){return Err("源音频含非有限采样。".into());}
   let clamped=value.clamp(-1.0,1.0);let pcm=(clamped*if clamped>=0.0{32767.0}else{32768.0}).round() as i16;
   let offset=(index*channels as usize+channel)*2;buffer[offset..offset+2].copy_from_slice(&pcm.to_le_bytes());
  }}
  writer.write_all(&buffer[..count*channels as usize*2]).map_err(err)?;
  written+=count as u64;
 }
 if let Some(error)=source.decode_error(){return Err(format!("音频解码失败：{error}"));}
 writer.flush().map_err(err)?;writer.get_ref().sync_all().map_err(err)?;drop(writer);drop(source);
 let bytes=44+frames*channels as u64*2;
 if identity(&partial)?.size!=bytes{return Err("片段WAV长度核验失败。".into());}
 if source_stamp(catalog,id)?!=stamp{return Err("导出期间源素材发生变化，未交付结果。".into());}
 rename_new(&partial,&target)?;cleanup.paths.push(target.clone());cleanup.paths.clear();
 Ok(ExportResult{paths:vec![target.display().to_string()],bytes})
}
#[tauri::command]pub async fn audio_clip_export(id:String,start:f64,end:f64,folder:String,catalog:tauri::State<'_,CatalogState>)->Result<ExportResult,String>{let catalog=catalog.inner().clone();tauri::async_runtime::spawn_blocking(move||clip_export(&id,start,end,&folder,&catalog)).await.map_err(err)?}


#[cfg(test)]mod tests{
 use super::*;use std::{thread,time::{Duration,Instant}};
 struct Fixture{root:PathBuf,catalog:Option<CatalogState>}
 impl Fixture{
  fn new()->Self{let root=std::env::temp_dir().join(format!("sound-workbench-tool-test-{}",Uuid::new_v4()));fs::create_dir(&root).unwrap();let root=fs::canonicalize(root).unwrap();let catalog=CatalogState::new(root.join("catalog.sqlite")).unwrap();Self{root,catalog:Some(catalog)}}
  fn catalog(&self)->&CatalogState{self.catalog.as_ref().unwrap()}
  fn source(&self,rate:u32,channels:u16,values:&[i16])->String{let path=self.root.join(format!("{}.wav",Uuid::new_v4()));let mut file=File::create(&path).unwrap();file.write_all(&header(values.len() as u64/channels as u64,channels,rate).unwrap()).unwrap();for value in values{file.write_all(&value.to_le_bytes()).unwrap();}drop(file);let id=Uuid::new_v4().to_string();let actual=identity(&path).unwrap();let conn=rusqlite::Connection::open(self.catalog().database_path()).unwrap();conn.execute("INSERT INTO assets(id,path_key,path,name,format,size,modified,rate,channels,duration,status) VALUES(?1,?2,?2,'fixture.wav','WAV',?3,?4,?5,?6,?7,'ready')",rusqlite::params![id,path.display().to_string(),actual.size,actual.modified,rate,channels,values.len() as f64/channels as f64/rate as f64]).unwrap();id}
  fn cache(&self)->PathBuf{cache_root(&self.root.join("cache")).unwrap()}
  fn direct(&self,id:&str,op:&str,options:ToolOptions)->Processed{let stamp=source_stamp(self.catalog(),id).unwrap();let source=PlaybackSource::open(&stamp.path).unwrap();process(&self.cache(),&Uuid::new_v4().to_string(),&stamp,op,&options,source.sample_rate(),source.channels(),&AtomicBool::new(false),|_|{}).unwrap()}
 }
 impl Drop for Fixture{fn drop(&mut self){self.catalog.take();let _=fs::remove_dir_all(&self.root);}}
 fn samples(path:&str)->Vec<i16>{fs::read(path).unwrap()[44..].chunks_exact(2).map(|bytes|i16::from_le_bytes(bytes.try_into().unwrap())).collect()}
 fn wait(state:&AudioToolsState)->ToolSnapshot{let start=Instant::now();loop{let value=state.snapshot().unwrap();if !["preparing","running","cancelling"].contains(&value.status.as_str()){return value;}assert!(start.elapsed()<Duration::from_secs(10));thread::sleep(Duration::from_millis(5));}}
 #[test]fn tools_reverse_preserves_frame_channels_across_blocks_and_source(){
  let f=Fixture::new();let original:Vec<i16>=(0..3001).flat_map(|n|[n as i16,(-n-1000) as i16]).collect();let id=f.source(8000,2,&original);let path=f.catalog().resolve_asset(&id).unwrap();let before=fs::read(&path).unwrap();let result=f.direct(&id,"reverse",Default::default());let output=samples(&result.outputs[0].path);assert_eq!(output.len(),original.len());for (i,frame) in output.chunks_exact(2).enumerate(){let index=3000-i;assert!((frame[0] as i32-original[index*2] as i32).abs()<=1);assert!((frame[1] as i32-original[index*2+1] as i32).abs()<=1);}assert_eq!(fs::read(path).unwrap(),before);
 }
 #[test]fn tools_mono_merge_and_split_are_correct_and_speed_changes_header_duration(){
  let f=Fixture::new();let id=f.source(1000,2,&vec![8192,24576].repeat(20));let merged=f.direct(&id,"mono_merge",Default::default());assert_eq!(merged.outputs[0].channels,1);assert!(samples(&merged.outputs[0].path).iter().all(|s|(*s as i32-16384).abs()<=1));
  let split=f.direct(&id,"mono_split",Default::default());assert_eq!(split.outputs.len(),2);for (index,expected) in [8192,24576].into_iter().enumerate(){assert_eq!(split.outputs[index].channels,1);assert!(samples(&split.outputs[index].path).iter().all(|s|(*s as i32-expected).abs()<=1));}
  let speed=f.direct(&id,"speed",ToolOptions{speed:2.0,..Default::default()});assert_eq!(speed.outputs[0].rate,2000);assert_eq!(speed.outputs[0].duration,0.01);assert_eq!(samples(&speed.outputs[0].path).len(),40);
 }
 #[test]fn tools_effects_change_signal_and_report_clipping(){
  let f=Fixture::new();let constant=f.source(8000,1,&vec![24576;800]);let gain=f.direct(&constant,"effect",ToolOptions{amount:2.0,..Default::default()});assert_eq!(gain.peak,1.5);assert_eq!(gain.clipped,800);assert!(samples(&gain.outputs[0].path).iter().all(|s|*s==32767));
  let highpass=f.direct(&constant,"effect",ToolOptions{effect:"highpass".into(),cutoff:1000.0,..Default::default()});assert!(samples(&highpass.outputs[0].path).last().unwrap().abs()<3);
  let alternating=f.source(8000,1,&vec![16384,-16384].repeat(400));let lowpass=f.direct(&alternating,"effect",ToolOptions{effect:"lowpass".into(),cutoff:100.0,..Default::default()});assert!(lowpass.peak<0.1);
  let distance=f.direct(&constant,"distance",ToolOptions{distance:10.0,..Default::default()});assert!(distance.peak<0.31);
  let mut impulse=vec![0;100];impulse[0]=8192;let impulse=f.source(1000,1,&impulse);let echo=f.direct(&impulse,"effect",ToolOptions{effect:"echo".into(),delay_ms:10.0,wet:0.5,..Default::default()});let echo=samples(&echo.outputs[0].path);assert!((echo[0]-4096).abs()<=1);assert!((echo[10]-4096).abs()<=1);assert_eq!(echo[9],0);assert_eq!(echo.len(),110);let short=f.source(1000,1,&[8192,0]);let tail=f.direct(&short,"effect",ToolOptions{effect:"echo".into(),delay_ms:10.0,wet:0.5,..Default::default()});assert_eq!(tail.outputs[0].duration,0.012);let values=samples(&tail.outputs[0].path);assert_eq!(values.len(),12);assert!((values[10]-4096).abs()<=1);
 }
 #[test]fn tools_cancel_and_existing_cache_conflicts_do_not_delete_unowned_data(){
  let f=Fixture::new();let id=f.source(8000,1,&vec![1234;8000]);let stamp=source_stamp(f.catalog(),&id).unwrap();let root=f.cache();let raw=root.join("fixed.raw.partial");fs::write(&raw,b"keep raw").unwrap();assert!(process(&root,"fixed",&stamp,"reverse",&Default::default(),8000,1,&AtomicBool::new(false),|_|{}).is_err());assert_eq!(fs::read(&raw).unwrap(),b"keep raw");
  let occupied=root.join("other-0.wav");fs::write(&occupied,b"keep output").unwrap();assert!(process(&root,"other",&stamp,"reverse",&Default::default(),8000,1,&AtomicBool::new(false),|_|{}).is_err());assert_eq!(fs::read(&occupied).unwrap(),b"keep output");assert!(!root.join("other.raw.partial").exists());
  let before=fs::read(&stamp.path).unwrap();let cancel=AtomicBool::new(false);assert!(process(&root,"cancelled",&stamp,"reverse",&Default::default(),8000,1,&cancel,|_|cancel.store(true,Ordering::Release)).is_err());assert!(!root.join("cancelled.raw.partial").exists());assert!(!root.join("cancelled-0.wav").exists());assert_eq!(fs::read(&stamp.path).unwrap(),before);
 }
 #[test]fn tools_job_export_is_non_overwriting_and_rejects_source_or_cache_changes(){
  let f=Fixture::new();let id=f.source(8000,2,&vec![1000,-2000].repeat(80));let state=AudioToolsState::new(f.cache());let job=state.start(id.clone(),"mono_split".into(),Default::default(),f.catalog().clone()).unwrap();let value=wait(&state);assert_eq!(value.status,"completed","{:?}",value.error);assert_eq!(value.outputs.len(),2);let left=state.resolve_output(&job,0).unwrap();assert!(state.resolve_output("unknown",0).is_err());assert!(state.resolve_output(&job,2).is_err());let folder=f.root.join("export");fs::create_dir(&folder).unwrap();let result=export(&state,&job,&folder.display().to_string()).unwrap();assert_eq!(result.paths.len(),2);let rollback=f.root.join("rollback-export");fs::create_dir(&rollback).unwrap();let occupied=rollback.join(format!("SoundWorkbench-Tool-{job}-1.wav"));fs::write(&occupied,b"keep existing output").unwrap();assert!(export(&state,&job,&rollback.display().to_string()).is_err());assert!(!rollback.join(format!("SoundWorkbench-Tool-{job}-0.wav")).exists());assert_eq!(fs::read(occupied).unwrap(),b"keep existing output");let first=fs::read(&result.paths[0]).unwrap();assert!(export(&state,&job,&folder.display().to_string()).is_err());assert_eq!(fs::read(&result.paths[0]).unwrap(),first);OpenOptions::new().append(true).open(left).unwrap().write_all(&[0]).unwrap();assert!(state.resolve_output(&job,0).is_err());OpenOptions::new().append(true).open(f.catalog().resolve_asset(&id).unwrap()).unwrap().write_all(&[0]).unwrap();assert!(state.resolve_output(&job,1).is_err());
 }
 #[test]fn tools_bad_options_and_unknown_sources_fail_without_outputs(){
  for options in [ToolOptions{speed:f64::NAN,..Default::default()},ToolOptions{speed:0.0,..Default::default()},ToolOptions{distance:51.0,..Default::default()},ToolOptions{effect:"vst".into(),..Default::default()},ToolOptions{wet:2.0,..Default::default()}]{assert!(validate("effect",&options).is_err());}assert!(validate("unknown",&Default::default()).is_err());let f=Fixture::new();let state=AudioToolsState::new(f.root.join("not-created"));assert!(state.start("not-an-id".into(),"reverse".into(),Default::default(),f.catalog().clone()).is_err());assert!(!state.root.exists());assert_eq!(state.snapshot().unwrap().status,"failed");
 }
 #[test]fn clip_export_copies_selected_range_and_never_overwrites(){
  let f=Fixture::new();let original:Vec<i16>=(0..1000).flat_map(|n|[n as i16,n as i16]).collect();let id=f.source(8000,2,&original);let path=f.catalog().resolve_asset(&id).unwrap();let before=fs::read(&path).unwrap();let folder=f.root.join("clips");fs::create_dir(&folder).unwrap();
  let result=clip_export(&id,0.01,0.02,&folder.display().to_string(),f.catalog()).unwrap();assert_eq!(result.paths.len(),1);assert_eq!(result.bytes,44+80*2*2);let output=samples(&result.paths[0]);assert_eq!(output.len(),160);let first=output[0] as usize;assert!((72..=88).contains(&first),"seek drifted to frame {first}");for index in 0..80{assert_eq!(output[index*2] as usize,first+index);assert_eq!(output[index*2+1] as usize,first+index);}assert_eq!(fs::read(&path).unwrap(),before);
  assert!(clip_export(&id,0.01,0.02,&folder.display().to_string(),f.catalog()).is_err());
  for (start,end) in [(-0.1,0.02),(0.02,0.01),(0.0,f64::NAN),(0.1,0.16),(0.2,0.21)]{assert!(clip_export(&id,start,end,&folder.display().to_string(),f.catalog()).is_err());}
 }
}
