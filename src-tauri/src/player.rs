//! 单声卡工作线程；只有显式播放命令打开设备。波形读取不打开声卡。
use crate::catalog::CatalogState;
use rodio::{OutputStream, OutputStreamHandle, Sink, Source};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::Manager;
use std::{
    fs::{self, File}, io::Read, path::{Path, PathBuf},
    sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}, mpsc::{self, Receiver, SyncSender, TrySendError}},
    time::{Duration, Instant, UNIX_EPOCH},
};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(8);
const WAVE_SECONDS: u64 = 60;
const WAVE_MAX_SAMPLES: u64 = 24_000_000;
const WAVE_MAX_PEAKS: usize = 4096;

#[cfg(test)]
#[path = "wave_acceptance.rs"]
mod acceptance;

#[derive(Clone, Debug, Serialize)]
pub struct PlayerSnapshot {
    pub id: Option<String>, pub state: String, pub position: f64,
    pub duration: Option<f64>, pub volume: f32, pub looping: bool, pub error: Option<String>,
}
impl Default for PlayerSnapshot {
    fn default() -> Self {
        Self { id:None, state:"idle".into(), position:0.0, duration:None, volume:0.15, looping:false, error:None }
    }
}

#[derive(Clone)]
pub struct PlayerState { inner:Arc<Inner> }
struct Inner {
    sender:Mutex<Option<SyncSender<Request>>>,
    snapshot:Arc<Mutex<PlayerSnapshot>>,
    wave_busy:Arc<AtomicBool>,
}
enum Command { Load { id:String, path:PathBuf, position:f64 }, Control { action:String, value:Option<f64> } }
struct Request {
    command:Command, response:SyncSender<Result<PlayerSnapshot,String>>,
    cancelled:Arc<AtomicBool>, deadline:Instant,
}
impl PlayerState {
    pub fn new() -> Self {
        Self { inner:Arc::new(Inner {
            sender:Mutex::new(None), snapshot:Arc::new(Mutex::new(PlayerSnapshot::default())),
            wave_busy:Arc::new(AtomicBool::new(false)),
        }) }
    }
    pub(crate) fn play_derived(&self,id:String,path:PathBuf)->Result<PlayerSnapshot,String>{self.request(Command::Load{id,path,position:0.0})}
    fn snapshot(&self) -> Result<PlayerSnapshot,String> {
        self.inner.snapshot.lock().map(|v|v.clone()).map_err(|_|"播放器状态锁不可用".into())
    }
    fn request(&self,command:Command) -> Result<PlayerSnapshot,String> {
        let sender = {
            let mut slot=self.inner.sender.lock().map_err(|_|"播放器命令锁不可用".to_string())?;
            if slot.is_none() {
                let (tx,rx)=mpsc::sync_channel(1);
                let snapshot=self.inner.snapshot.clone();
                std::thread::Builder::new().name("sound-workbench-player".into())
                    .spawn(move||{
                        let recovery=snapshot.clone();
                        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(||worker(rx,snapshot))).is_err(){
                            if let Ok(mut state)=recovery.lock(){fail(&mut state,"音频线程异常退出，已释放播放资源；请重新发起播放。".into());}
                        }
                    }).map_err(|e|format!("无法启动音频线程：{e}"))?;
                *slot=Some(tx);
            }
            slot.as_ref().unwrap().clone()
        };
        let (response,answer)=mpsc::sync_channel(1);
        let cancelled=Arc::new(AtomicBool::new(false));
        let request=Request {command,response,cancelled:cancelled.clone(),deadline:Instant::now()+COMMAND_TIMEOUT};
        match sender.try_send(request) {
            Ok(())=>{},
            Err(TrySendError::Full(_))=>return Err("播放器正在处理其他操作，请稍后重试。".into()),
            Err(TrySendError::Disconnected(_))=>{
                if let Ok(mut slot)=self.inner.sender.lock(){*slot=None;}
                return Err("音频线程已退出；请重新发起播放。".into());
            }
        }
        match answer.recv_timeout(COMMAND_TIMEOUT) {
            Ok(result)=>result,
            Err(mpsc::RecvTimeoutError::Timeout)=>{
                cancelled.store(true,Ordering::Release);
                Err("音频操作等待超过8秒；设备或文件系统可能仍在处理，未宣称已强制取消。请查看播放器状态后重试。".into())
            },
            Err(_)=>Err("音频线程未能返回结果，请重新发起播放。".into()),
        }
    }
}

struct Playing {
    sink:Sink,
    // Decoder preview seeking happens before Sink's position tracker starts.
    position_offset:f64,
    handle:OutputStreamHandle,
    _stream:OutputStream,
}
// rodio 0.20.1's Symphonia adapter miscomputes fractional duration, then
// clamps valid seeks to EOF. Keep playback on a direct, time-base-aware source.
pub(crate) struct PlaybackSource {
    path:PathBuf,
    format:Box<dyn symphonia::core::formats::FormatReader>,
    codec:Box<dyn symphonia::core::codecs::Decoder>,
    track:u32, base:symphonia::core::units::TimeBase,
    rate:u32, channels:u16, duration:Option<Duration>,
    buffer:Vec<f32>, offset:usize, ended:bool, last_error:Option<String>,
}
impl PlaybackSource {
    pub(crate) fn open(path:&Path)->Result<Self,String>{
        use symphonia::core::{io::MediaSourceStream,probe::Hint};
        let stream=MediaSourceStream::new(Box::new(File::open(path).map_err(|e|e.to_string())?),Default::default());
        let mut hint=Hint::new();if let Some(ext)=path.extension().and_then(|e|e.to_str()){hint.with_extension(ext);}
        // MP3有效时长已在duration.rs扣除编码延迟/尾部填充；解码也必须做同一裁切。
        // 此处开启gapless只影响包时间戳与解码采样，不再次扣减独立计算的时长。
        let format_options=symphonia::core::formats::FormatOptions{enable_gapless:true,..Default::default()};
        let probed=symphonia::default::get_probe().format(&hint,stream,&format_options,&Default::default()).map_err(|e|e.to_string())?;
        let track=probed.format.default_track().ok_or("没有音频轨道")?;
        let params=&track.codec_params;
        let rate=params.sample_rate.ok_or("缺少采样率")?;
        let channels=params.channels.ok_or("缺少声道信息")?.count() as u16;
        if rate==0 || channels==0{return Err("音频规格无效".into());}
        let base=params.time_base.unwrap_or(symphonia::core::units::TimeBase::new(1,rate));
        let codec=symphonia::default::get_codecs().make(params,&Default::default()).map_err(|e|e.to_string())?;
        let mut source=Self{path:path.to_path_buf(),track:track.id,format:probed.format,codec,base,rate,channels,
            duration:crate::duration::seconds(path).map(Duration::from_secs_f64),buffer:Vec::new(),offset:0,ended:false,last_error:None};
        source.fill(None)?;Ok(source)
    }
    /// 流式渲染需要区分正常结束与解码失败；读取此状态不会打开声卡。
    pub(crate) fn decode_error(&self)->Option<String>{self.last_error.clone()}
    fn decode_failure(&mut self,message:String)->String{self.last_error=Some(message.clone());message}
    fn fill(&mut self,target:Option<u64>)->Result<(),String>{
        use symphonia::core::audio::SampleBuffer;
        loop {
            let packet=match self.format.next_packet(){
                Ok(packet)=>packet,
                Err(symphonia::core::errors::Error::IoError(error)) if error.kind()==std::io::ErrorKind::UnexpectedEof=>return Err(error.to_string()),
                Err(error)=>return Err(self.decode_failure(format!("读取音频包失败：{error}"))),
            };
            if packet.track_id()!=self.track{continue;}
            let decoded=match self.codec.decode(&packet){
                Ok(decoded)=>decoded,
                Err(error)=>{let message=format!("解码音频包失败：{error}");self.last_error=Some(message.clone());return Err(message);},
            };
            let spec=*decoded.spec();
            if spec.rate!=self.rate || spec.channels.count()!=self.channels as usize{return Err(self.decode_failure("不支持播放中改变采样规格".into()));}
            let mut buffer=SampleBuffer::<f32>::new(decoded.capacity() as u64,spec);
            buffer.copy_interleaved_ref(decoded);
            self.buffer=buffer.samples().to_vec();self.offset=0;
            if let Some(target)=target {
                // Packet timestamps are time-base ticks, not necessarily sample frames.
                let delta=self.base.calc_time(target.saturating_sub(packet.ts()));
                let frames=((delta.seconds as f64+delta.frac)*self.rate as f64).round() as usize;
                self.offset=frames.saturating_mul(self.channels as usize).min(self.buffer.len());
            }
            if self.offset<self.buffer.len(){return Ok(());}
        }
    }
    fn seek_to(&mut self,pos:Duration)->Result<(),String>{
        use symphonia::core::formats::{SeekMode,SeekTo};
        if self.duration.is_some_and(|d|pos>d){return Err("定位位置超出音频范围".into());}
        if self.duration==Some(pos){self.buffer.clear();self.offset=0;self.ended=true;return Ok(());}
        // FLAC's demuxer retains parser state at EOF: a reused seek(0) can
        // return only the last partial packet. Reopen for a clean demuxer and
        // codec, then use indexed seeking; never decode the whole prefix.
        // Commit only after success, so a failed seek keeps the current source.
        let mut next=Self::open(&self.path)?;
        if !pos.is_zero(){
            let result=next.format.seek(SeekMode::Accurate,SeekTo::Time{time:pos.as_secs_f64().into(),track_id:Some(next.track)}).map_err(|e|e.to_string())?;
            next.codec.reset();next.buffer.clear();next.offset=0;
            next.fill(Some(result.required_ts))?;
        }
        *self=next;Ok(())
    }
}
impl Iterator for PlaybackSource {
    type Item=f32;
    fn next(&mut self)->Option<f32>{
        if self.ended{return None;}
        if self.offset>=self.buffer.len() && self.fill(None).is_err(){self.ended=true;return None;}
        let value=self.buffer.get(self.offset).copied();self.offset+=1;value
    }
}
impl Source for PlaybackSource {
    fn current_frame_len(&self)->Option<usize>{Some(self.buffer.len().saturating_sub(self.offset))}
    fn channels(&self)->u16{self.channels}
    fn sample_rate(&self)->u32{self.rate}
    fn total_duration(&self)->Option<Duration>{self.duration}
    fn try_seek(&mut self,pos:Duration)->Result<(),rodio::source::SeekError>{
        self.seek_to(pos).map_err(|e|rodio::source::SeekError::Other(Box::new(std::io::Error::other(e))))
    }
}
fn prepared_source(path:&Path,position:f64)->Result<(PlaybackSource,Option<f64>),String>{
    let mut source=PlaybackSource::open(path)?;
    // 时长按文件头自算，不用 rodio 的 total_duration()（0.20.1 换算缺陷，见 duration.rs）。
    let duration=crate::duration::seconds(path);
    if source.channels()==0 || source.sample_rate()==0 {return Err("音频格式信息无效".into());}
    if position>0.0 {
        if !position.is_finite() || position<0.0 || duration.is_some_and(|seconds|position>seconds){return Err("试听位置超出音频范围".into());}
        source.try_seek(Duration::from_secs_f64(position)).map_err(|e|format!("当前音频无法定位试听：{e}"))?;
    } else {validate_preview(position,WAVE_SECONDS as f64)?;}
    Ok((source,duration))
}
fn validate_preview(position:f64,bound:f64)->Result<(),String>{
    if !position.is_finite() || position<0.0 || position>bound.min(WAVE_SECONDS as f64){
        return Err("试听位置超出已分析的前段音频范围".into());
    }
    Ok(())
}
fn start_audio(path:&Path,volume:f32,position:f64,cancelled:&AtomicBool)->Result<(Playing,Option<f64>),String> {
    let (source,duration)=prepared_source(path,position)?;
    if cancelled.load(Ordering::Acquire){return Err("播放准备已超时，本次未开始发声。".into());}
    let (stream,handle)=OutputStream::try_default().map_err(|e|format!("无法打开默认音频输出：{e}"))?;
    let sink=Sink::try_new(&handle).map_err(|e|format!("创建播放通路失败：{e}"))?;
    sink.pause();sink.set_volume(volume);sink.append(source);
    if cancelled.load(Ordering::Acquire){return Err("播放准备已超时，本次未开始发声。".into());}
    sink.play();
    Ok((Playing{sink,position_offset:position,handle,_stream:stream},duration))
}
fn publish(shared:&Mutex<PlayerSnapshot>,snapshot:&PlayerSnapshot) {
    if let Ok(mut value)=shared.lock(){*value=snapshot.clone();}
}
fn fail(snapshot:&mut PlayerSnapshot,message:String) {
    snapshot.state="error".into();snapshot.error=Some(message);
}
fn numeric(value:Option<f64>,label:&str)->Result<f64,String> {
    value.filter(|v|v.is_finite()).ok_or_else(||format!("{label}需要有限数值"))
}
fn validate_control(action:&str,value:Option<f64>,duration:Option<f64>)->Result<(),String> {
    match action {
        "pause"|"resume"|"stop"=>Ok(()),
        "volume"=>{let v=numeric(value,"音量")?;if !(0.0..=1.0).contains(&v){Err("音量必须在0到1之间".into())}else{Ok(())}},
        "loop"=>{let v=numeric(value,"循环状态")?;if v!=0.0&&v!=1.0{Err("循环状态只能为0或1".into())}else{Ok(())}},
        "seek"=>{
            let v=numeric(value,"定位秒数")?;
            if v<0.0 || duration.is_some_and(|d|v>d){return Err("定位位置超出音频范围".into());}
            Duration::try_from_secs_f64(v).map(|_|()).map_err(|_|"定位秒数超出可处理范围".into())
        },
        _=>Err("未知播放控制操作".into()),
    }
}
fn worker(receiver:Receiver<Request>,shared:Arc<Mutex<PlayerSnapshot>>) {
    let mut snapshot=PlayerSnapshot::default();
    let mut playing:Option<Playing>=None;
    let mut last:Option<(String,PathBuf)>=None;
    loop {
        match receiver.recv_timeout(Duration::from_millis(25)) {
            Ok(request)=>{
                if request.cancelled.load(Ordering::Acquire)||Instant::now()>request.deadline {
                    let _=request.response.send(Err("排队操作已超时，未执行。".into()));continue;
                }
                let result:Result<(),String>=(||{
                    match request.command {
                        Command::Load{id,path,position}=>{
                            if let Some(old)=playing.take(){old.sink.stop();}
                            snapshot.id=Some(id.clone());snapshot.state="loading".into();snapshot.position=0.0;
                            snapshot.duration=None;snapshot.error=None;last=Some((id,path.clone()));publish(&shared,&snapshot);
                            match start_audio(&path,snapshot.volume,position,&request.cancelled){
                                Ok((audio,duration))=>{playing=Some(audio);snapshot.duration=duration;snapshot.position=position;snapshot.state="playing".into();},
                                Err(e)=>{fail(&mut snapshot,e.clone());return Err(e);}
                            }
                        },
                        Command::Control{action,value}=>{
                            validate_control(&action,value,snapshot.duration)?;
                            match action.as_str() {
                                "stop"=>{if let Some(audio)=playing.take(){audio.sink.stop();}snapshot.state="stopped".into();snapshot.position=0.0;snapshot.error=None;},
                                "volume"=>{snapshot.volume=value.unwrap() as f32;if let Some(audio)=&playing{audio.sink.set_volume(snapshot.volume);}},
                                "loop"=>snapshot.looping=value==Some(1.0),
                                "pause"=>{let audio=playing.as_ref().ok_or("当前没有可暂停的音频")?;audio.sink.pause();snapshot.state="paused".into();snapshot.position=audio.position_offset+audio.sink.get_pos().as_secs_f64();},
                                "resume"=>{
                                    if let Some(audio)=&playing {audio.sink.play();snapshot.state="playing".into();}
                                    else {
                                        let (_,path)=last.as_ref().ok_or("请先选择一条素材并播放")?;
                                        match start_audio(path,snapshot.volume,0.0,&request.cancelled){
                                            Ok((audio,duration))=>{playing=Some(audio);snapshot.duration=duration;snapshot.position=0.0;snapshot.state="playing".into();},
                                            Err(e)=>{fail(&mut snapshot,e.clone());return Err(e);}
                                        }
                                    }
                                },
                                "seek"=>{
                                    let audio=playing.as_mut().ok_or("请先播放音频再定位")?;
                                    if audio.sink.empty(){return Err("音频已经结束，请重新播放后定位".into());}
                                    audio.sink.try_seek(Duration::try_from_secs_f64(value.unwrap()).map_err(|e|e.to_string())?)
                                        .map_err(|e|format!("当前音频无法定位：{e}"))?;
                                    audio.position_offset=0.0;
                                    snapshot.position=audio.sink.get_pos().as_secs_f64();
                                },
                                _=>unreachable!(),
                            }
                            snapshot.error=None;
                        }
                    }
                    Ok(())
                })();
                if let Err(message)=&result {snapshot.error=Some(message.clone());}
                publish(&shared,&snapshot);
                let _=request.response.send(result.map(|_|snapshot.clone()));
            },
            Err(mpsc::RecvTimeoutError::Disconnected)=>break,
            Err(mpsc::RecvTimeoutError::Timeout)=>{},
        }
        if let Some(audio)=&mut playing {
            snapshot.position=audio.position_offset+audio.sink.get_pos().as_secs_f64();
            if audio.sink.empty() {
                if snapshot.looping&&snapshot.state=="playing" {
                    let restart=(||{
                        let (_,path)=last.as_ref().ok_or("没有循环素材")?;
                        let source=PlaybackSource::open(path)?;
                        let sink=Sink::try_new(&audio.handle).map_err(|e|e.to_string())?;
                        sink.set_volume(snapshot.volume);sink.append(source);
                        audio.sink=sink;audio.position_offset=0.0;snapshot.position=0.0;Ok::<(),String>(())
                    })();
                    if let Err(e)=restart{fail(&mut snapshot,e);playing=None;}
                } else {
                    snapshot.state="ended".into();
                    if let Some(duration)=snapshot.duration{snapshot.position=duration;}
                    playing=None;
                }
            }
        }
        publish(&shared,&snapshot);
    }
    if let Some(audio)=playing{audio.sink.stop();}
}

#[tauri::command]
pub async fn player_load(id:String,catalog:tauri::State<'_,CatalogState>,player:tauri::State<'_,PlayerState>)->Result<PlayerSnapshot,String>{
    let catalog=catalog.inner().clone();let player=player.inner().clone();
    tauri::async_runtime::spawn_blocking(move||{
        let path=catalog.resolve_asset(&id)?;
        let snapshot=player.request(Command::Load{id:id.clone(),path,position:0.0})?;
        let _=catalog.record_played(&id);Ok(snapshot)
    }).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub async fn player_preview_at(id:String,position:f64,catalog:tauri::State<'_,CatalogState>,player:tauri::State<'_,PlayerState>)->Result<PlayerSnapshot,String>{
    validate_preview(position,WAVE_SECONDS as f64)?;
    let catalog=catalog.inner().clone();let player=player.inner().clone();
    tauri::async_runtime::spawn_blocking(move||{
        let path=catalog.resolve_asset(&id)?;
        let source=PlaybackSource::open(&path)?;
        let limit=(WAVE_MAX_SAMPLES as f64/(source.sample_rate() as f64*source.channels() as f64)).min(WAVE_SECONDS as f64);
        validate_preview(position,source.total_duration().map(|duration|duration.as_secs_f64()).unwrap_or(limit).min(limit))?;
        let snapshot=player.request(Command::Load{id:id.clone(),path,position})?;
        let _=catalog.record_played(&id);Ok(snapshot)
    }).await.map_err(|e|e.to_string())?
}
/// 只接收本次会话已完成渲染的任务ID；调用方不能指定文件路径。
#[tauri::command]
pub async fn player_render_preview(job_id:String,render:tauri::State<'_,crate::layer_render::LayerRenderState>,player:tauri::State<'_,PlayerState>)->Result<PlayerSnapshot,String>{
    let render=render.inner().clone();let player=player.inner().clone();
    tauri::async_runtime::spawn_blocking(move||request_render_preview(&render,&player,&job_id)).await.map_err(|e|e.to_string())?
}
fn request_render_preview(render:&crate::layer_render::LayerRenderState,player:&PlayerState,job_id:&str)->Result<PlayerSnapshot,String>{
    // 必须先核验受控任务输出，未知任务不会启动播放器线程或打开设备。
    let path=render.resolve_output(job_id)?;
    // 派生渲染结果不作为素材库原文件，也不写入素材试听历史。
    player.request(Command::Load{id:format!("render:{job_id}"),path,position:0.0})
}
#[tauri::command]
pub async fn player_preview_full_at(id:String,position:f64,job_id:String,catalog:tauri::State<'_,CatalogState>,full:tauri::State<'_,crate::full_wave::FullWaveState>,player:tauri::State<'_,PlayerState>)->Result<PlayerSnapshot,String>{
 let catalog=catalog.inner().clone();let full=full.inner().clone();let player=player.inner().clone();
 tauri::async_runtime::spawn_blocking(move||{let path=full.resolve_position(&id,&job_id,position)?;let snapshot=player.request(Command::Load{id:id.clone(),path,position})?;let _=catalog.record_played(&id);Ok(snapshot)}).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub async fn player_control(action:String,value:Option<f64>,player:tauri::State<'_,PlayerState>)->Result<PlayerSnapshot,String>{
    // 在入队前验证基本输入，非法命令不会打开音频线程或设备。
    validate_control(&action,value,player.snapshot()?.duration)?;
    let player=player.inner().clone();
    tauri::async_runtime::spawn_blocking(move||player.request(Command::Control{action,value})).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub fn player_status(player:tauri::State<'_,PlayerState>)->Result<PlayerSnapshot,String>{player.snapshot()}

#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct Waveform {
    pub peaks:Vec<f32>, pub duration:Option<f64>, pub analyzed_duration:f64, pub truncated:bool,
}
// Bump when decoding, duration calculation or bucket semantics change.
const WAVE_CACHE_VERSION: u32 = 3;
const CACHE_ENTRIES: usize = 256;
const CACHE_BYTES: u64 = 16 * 1024 * 1024;
const CACHE_RECORD_BYTES: u64 = 128 * 1024;
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct WaveIdentity { path:PathBuf, size:u64, modified_ns:u128, version:u32 }
impl WaveIdentity {
    fn read(path:&Path)->std::io::Result<Self>{
        let path=path.canonicalize()?;
        let metadata=fs::metadata(&path)?;
        if !metadata.is_file(){return Err(std::io::Error::other("not a file"));}
        let modified_ns=metadata.modified()?.duration_since(UNIX_EPOCH).map_err(std::io::Error::other)?.as_nanos();
        let identity=Self{path,size:metadata.len(),modified_ns,version:WAVE_CACHE_VERSION};
        // Non-Unicode native paths may not serialize as JSON; skip caching, not playback.
        serde_json::to_vec(&identity).map_err(std::io::Error::other)?;
        Ok(identity)
    }
    fn filename(&self)->String{
        let encoded=serde_json::to_vec(self).expect("wave identity serialization");
        format!("{:x}.json",Sha256::digest(encoded))
    }
}
#[derive(Serialize,Deserialize)]
struct WaveRecord { identity:WaveIdentity, wave:Waveform }
fn valid_wave(wave:&Waveform)->bool{
    !wave.peaks.is_empty() && wave.peaks.len()<=WAVE_MAX_PEAKS
        && wave.peaks.iter().all(|p|p.is_finite() && (0.0..=1.0).contains(p))
        && wave.analyzed_duration.is_finite() && wave.analyzed_duration>0.0
        && wave.analyzed_duration<=WAVE_SECONDS as f64
        && wave.duration.is_none_or(|d|d.is_finite() && d>0.0)
}
fn read_cached(dir:&Path,identity:&WaveIdentity)->Option<Waveform>{
    let path=dir.join(identity.filename());
    let file=File::open(&path).ok()?;
    if file.metadata().ok()?.len()>CACHE_RECORD_BYTES{return None;}
    let mut data=Vec::new();
    file.take(CACHE_RECORD_BYTES+1).read_to_end(&mut data).ok()?;
    if data.len() as u64>CACHE_RECORD_BYTES{return None;}
    let record:WaveRecord=serde_json::from_slice(&data).ok()?;
    (record.identity==*identity && valid_wave(&record.wave)).then_some(record.wave)
}
// This directory is dedicated to disposable waveform records. Never follow links.
fn trim_cache(dir:&Path,max_entries:usize,max_bytes:u64)->std::io::Result<()>{
    let mut entries=Vec::new();
    for item in fs::read_dir(dir)?{
        let item=item?;let name=item.file_name();let name=name.to_string_lossy();
        if !(name.ends_with(".json") || name.ends_with(".tmp")){continue;}
        let meta=fs::symlink_metadata(item.path())?;
        if !meta.is_file() || meta.file_type().is_symlink(){continue;}
        entries.push((meta.modified().unwrap_or(UNIX_EPOCH),item.path(),meta.len()));
    }
    entries.sort_by(|a,b|a.0.cmp(&b.0).then_with(||a.1.cmp(&b.1)));
    let mut count=entries.len();let mut bytes=entries.iter().map(|e|e.2).sum::<u64>();
    for (_,path,size) in entries{
        if count<=max_entries && bytes<=max_bytes{break;}
        fs::remove_file(path)?;count-=1;bytes=bytes.saturating_sub(size);
    }
    Ok(())
}
fn write_cached(dir:&Path,identity:WaveIdentity,wave:&Waveform)->std::io::Result<()>{
    if !valid_wave(wave){return Ok(());}
    fs::create_dir_all(dir)?;
    // Leave room BEFORE writing; a failed eviction must not grow the cache further.
    trim_cache(dir,CACHE_ENTRIES-1,CACHE_BYTES-CACHE_RECORD_BYTES)?;
    let target=dir.join(identity.filename());
    let data=serde_json::to_vec(&WaveRecordRef{identity:&identity,wave}).map_err(std::io::Error::other)?;
    if data.len() as u64>CACHE_RECORD_BYTES{return Ok(());}
    let temporary=dir.join(format!("{}.tmp",uuid::Uuid::new_v4()));
    fs::write(&temporary,data)?;
    // Windows rename does not replace an existing file. A corrupt record is disposable.
    if target.exists(){let _=fs::remove_file(&target);}
    if let Err(error)=fs::rename(&temporary,&target){let _=fs::remove_file(&temporary);return Err(error);}
    Ok(())
}
#[derive(Serialize)]
struct WaveRecordRef<'a>{identity:&'a WaveIdentity,wave:&'a Waveform}
fn cached_waveform(path:&Path,cache:Option<&Path>)->Result<Waveform,String>{
    let before=WaveIdentity::read(path).ok();
    if let (Some(dir),Some(identity))=(cache,before.as_ref()){
        // Also trim on hits, so stale versions/leftover temporary files stay bounded.
        let _=trim_cache(dir,CACHE_ENTRIES,CACHE_BYTES);
        if let Some(wave)=read_cached(dir,identity){
            if WaveIdentity::read(path).ok().as_ref()==Some(identity){return Ok(wave);}
        }
    }
    let wave=analyze_waveform(path)?;
    if let (Some(dir),Some(identity))=(cache,before){
        if WaveIdentity::read(path).ok().as_ref()==Some(&identity){let _=write_cached(dir,identity,&wave);}
    }
    Ok(wave)
}
struct WaveGuard(Arc<AtomicBool>);
impl Drop for WaveGuard{fn drop(&mut self){self.0.store(false,Ordering::Release);}}
fn analyze_waveform(path:&Path)->Result<Waveform,String>{
    // 与试听/混音共用gapless解码时间轴，避免MP3峰值包含编码延迟和填充。
    let mut source=PlaybackSource::open(path)?;
    let wave=analyze_source(&mut source,crate::duration::seconds(path))?;
    if let Some(error)=source.decode_error(){return Err(format!("波形解码失败：{error}"));}
    Ok(wave)
}
fn analyze_source(source:impl Source<Item=f32>,duration:Option<f64>)->Result<Waveform,String>{
    let rate=source.sample_rate() as u64;let channels=source.channels() as u64;
    if rate==0||channels==0{return Err("音频规格无效".into());}
    let per_second=rate.checked_mul(channels).ok_or("音频规格超出分析范围")?;
    let limit=per_second.saturating_mul(WAVE_SECONDS).min(WAVE_MAX_SAMPLES);
    // ~0.1ms real-sample maxima retain short transients before final reduction.
    // At most 1.2 million temporary peaks within the existing 60s cap.
    // Reserve the bounded length explicitly (under 4.8MiB), avoiding Vec doubling.
    let bucket_samples=(per_second/10_000).max(1);
    let mut buckets:Vec<f32>=Vec::with_capacity(limit.div_ceil(bucket_samples) as usize);let mut count=0u64;
    for sample in source.take(limit as usize){
        if !sample.is_finite(){return Err("波形解码出现非有限采样值".into());}
        let bucket=(count/bucket_samples) as usize;
        if bucket==buckets.len(){buckets.push(0.0);}
        buckets[bucket]=buckets[bucket].max(sample.abs().min(1.0));count+=1;
    }
    if count==0{return Err("音频没有可分析的采样".into());}
    let bins=buckets.len().min(WAVE_MAX_PEAKS);
    let peaks=(0..bins).map(|i|{
        let start=i*buckets.len()/bins;let end=((i+1)*buckets.len()/bins).max(start+1);
        buckets[start..end].iter().copied().fold(0.0f32,f32::max)
    }).collect();
    Ok(Waveform{peaks,duration,analyzed_duration:count as f64/per_second as f64,truncated:count>=limit})
}
#[tauri::command]
pub async fn player_waveform(id:String,catalog:tauri::State<'_,CatalogState>,player:tauri::State<'_,PlayerState>,app:tauri::AppHandle)->Result<Waveform,String>{
    player.inner.wave_busy.compare_exchange(false,true,Ordering::AcqRel,Ordering::Acquire)
        .map_err(|_|"波形分析进行中，请稍后重试。".to_string())?;
    let guard=WaveGuard(player.inner.wave_busy.clone());let catalog=catalog.inner().clone();
    let cache=app.path().app_cache_dir().ok().map(|p|p.join("waveforms-v1"));
    tauri::async_runtime::spawn_blocking(move||{
        let _guard=guard;let path=catalog.resolve_asset(&id)?;cached_waveform(&path,cache.as_deref())
    }).await.map_err(|e|e.to_string())?
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn dense_waveform_retains_real_transients_without_interpolation(){
        let mut samples=vec![0.0f32;8192];samples[1024]=0.75;samples[4096]=-0.9;
        let source=rodio::buffer::SamplesBuffer::new(1,8192,samples);
        let wave=analyze_source(source,Some(1.0)).unwrap();
        assert_eq!(wave.peaks.len(),4096);
        assert_eq!(wave.peaks[512],0.75);assert_eq!(wave.peaks[2048],0.9);
        assert_eq!(wave.peaks.iter().filter(|v|**v!=0.0).count(),2);
        assert_eq!(wave.analyzed_duration,1.0);
    }
    #[test]
    fn very_short_waveform_keeps_only_existing_samples(){
        let wave=analyze_source(rodio::buffer::SamplesBuffer::new(1,1000,vec![0.1f32,-0.5,0.0]),Some(0.003)).unwrap();
        assert_eq!(wave.peaks,vec![0.1,0.5,0.0]);assert_eq!(wave.analyzed_duration,0.003);assert!(!wave.truncated);
    }
    #[test]
    fn old_wave_algorithm_records_are_not_reused(){
        let temp=TestDir::new();let path=sample("pcm-44100.wav");let identity=WaveIdentity::read(&path).unwrap();
        let mut old=WaveIdentity::read(&path).unwrap();old.version=2;
        let old_wave=Waveform{peaks:vec![0.2;256],duration:Some(0.5),analyzed_duration:0.5,truncated:false};
        assert_ne!(old.filename(),identity.filename());
        // Even a stale record placed at the new key is rejected by its embedded identity.
        fs::write(temp.0.join(identity.filename()),serde_json::to_vec(&WaveRecordRef{identity:&old,wave:&old_wave}).unwrap()).unwrap();
        assert!(read_cached(&temp.0,&identity).is_none());
        let wave=cached_waveform(&path,Some(&temp.0)).unwrap();assert_eq!(wave.peaks.len(),4096);
        assert!(fs::metadata(temp.0.join(identity.filename())).unwrap().len()<=CACHE_RECORD_BYTES);
        assert!(read_cached(&temp.0,&identity).is_some());
    }
    #[test]
    fn preview_prepares_seek_without_opening_device(){
        for position in [f64::NAN,f64::INFINITY,-0.1,60.1]{assert!(validate_preview(position,60.0).is_err());}
        assert!(validate_preview(0.6,0.5).is_err());
        let path=sample("pcm-44100.wav");
        assert!(prepared_source(&path,0.6).is_err());
        let (source,duration)=prepared_source(&path,0.25).unwrap();
        assert!(duration.is_some_and(|d|(d-0.5).abs()<0.01));
        let samples=source.sample_rate() as usize*source.channels() as usize;
        let remaining=source.count();
        assert!(remaining>samples/5 && remaining<samples*3/10,"seek should leave ~0.25 seconds, got {remaining} samples");
    }
    #[test]
    fn mp3_gapless_samples_match_effective_duration_without_padding(){
        let path=sample("lossy-44100.mp3");let before=fs::read(&path).unwrap();
        let mut source=PlaybackSource::open(&path).unwrap();
        assert_eq!(source.sample_rate(),44100);assert_eq!(source.channels(),1);
        assert!((source.total_duration().unwrap().as_secs_f64()-0.5).abs()<1e-9);
        let full:Vec<f32>=source.by_ref().collect();
        assert_eq!(full.len(),22050,"0.5秒MP3必须只输出有效采样，不能保留编码延迟或填充");
        assert!(source.decode_error().is_none());
        source.try_seek(Duration::from_secs_f64(0.25)).unwrap();let tail:Vec<f32>=source.by_ref().collect();
        assert_eq!(tail.len(),11025,"gapless时间戳应从有效音频起点定位");
        let error=tail.iter().zip(&full[11025..]).map(|(a,b)|(a-b).abs() as f64).sum::<f64>()/tail.len() as f64;
        assert!(error<0.005,"MP3非零定位应与有效音频对齐，平均采样误差{error}");
        source.try_seek(Duration::ZERO).unwrap();let reset:Vec<f32>=source.by_ref().collect();
        assert!(reset.len()==full.len()&&reset.iter().zip(&full).all(|(a,b)|a==b),"MP3 seek0应完整复现有效采样");
        assert_eq!(fs::read(&path).unwrap(),before);
    }
    #[test]
    fn playback_seek_matches_decoded_samples_in_four_formats(){
        for name in ["pcm-44100.wav","pcm-48000.aiff","lossless-48000.flac","lossy-44100.mp3"]{
            let path=sample(name);let mut source=PlaybackSource::open(&path).unwrap();
            let rate=source.sample_rate() as usize;let channels=source.channels() as usize;
            let full:Vec<f32>=PlaybackSource::open(&path).unwrap().collect();
            source.try_seek(Duration::from_secs_f64(0.25)).unwrap();
            let tail:Vec<f32>=source.by_ref().collect();let start=rate/4*channels;
            assert_eq!(tail.len(),full.len()-start,"{name}: incorrect remaining samples");
            let error=tail.iter().zip(&full[start..]).map(|(a,b)|(a-b).abs() as f64).sum::<f64>()/tail.len() as f64;
            // Lossy decoders may reconstruct a few boundary samples differently after reset.
            let tolerance=if name.ends_with("mp3"){0.005}else{0.000001};
            assert!(error<tolerance,"{name}: seek sample alignment error {error}");
            source.try_seek(Duration::ZERO).unwrap();let reset:Vec<f32>=source.by_ref().collect();
            let first_difference=reset.iter().zip(&full).position(|(a,b)|a!=b);
            assert!(reset.len()==full.len() && first_difference.is_none(),"{name}: seek0 len={} expected={} first_difference={:?} values={:?}",reset.len(),full.len(),first_difference,first_difference.map(|i|(reset[i],full[i])));
            source.try_seek(source.total_duration().unwrap()).unwrap();assert!(source.next().is_none(),"{name}: seek to end");
        }
    }
    struct TestDir(PathBuf);
    impl TestDir{fn new()->Self{let path=std::env::temp_dir().join(format!("sound-workbench-wave-test-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&path).unwrap();Self(path)}}
    impl Drop for TestDir{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
    #[test]
    fn waveform_cache_persists_and_recovers_from_corruption(){
        let temp=TestDir::new();let source=sample("pcm-44100.wav");
        let first=cached_waveform(&source,Some(&temp.0)).unwrap();
        let identity=WaveIdentity::read(&source).unwrap();let record=temp.0.join(identity.filename());
        let cached=read_cached(&temp.0,&identity).unwrap();assert_eq!(first.peaks,cached.peaks);
        // An invalid but well-formed record cannot be served.
        let invalid=Waveform{peaks:vec![2.0],duration:Some(1.0),analyzed_duration:1.0,truncated:false};
        fs::write(&record,serde_json::to_vec(&WaveRecordRef{identity:&identity,wave:&invalid}).unwrap()).unwrap();
        assert!(read_cached(&temp.0,&identity).is_none());
        let recovered=cached_waveform(&source,Some(&temp.0)).unwrap();assert_eq!(first.peaks,recovered.peaks);
        fs::write(&record,b"broken JSON").unwrap();assert!(read_cached(&temp.0,&identity).is_none());
        assert!(cached_waveform(&source,Some(&temp.0)).is_ok());
        fs::write(&record,vec![b' ';CACHE_RECORD_BYTES as usize+1]).unwrap();assert!(read_cached(&temp.0,&identity).is_none());
    }
    #[test]
    fn waveform_identity_invalidates_each_source_attribute(){
        let temp=TestDir::new();let path=temp.0.join("source.wav");fs::copy(sample("pcm-44100.wav"),&path).unwrap();
        let original=WaveIdentity::read(&path).unwrap();let key=original.filename();
        let mut changed=WaveIdentity::read(&path).unwrap();changed.version+=1;assert_ne!(key,changed.filename());
        changed=WaveIdentity::read(&path).unwrap();changed.path=temp.0.join("other.wav");assert_ne!(key,changed.filename());
        let file=File::options().write(true).open(&path).unwrap();
        file.set_modified(UNIX_EPOCH+Duration::from_secs(1_600_000_000)).unwrap();
        let modified=WaveIdentity::read(&path).unwrap();assert_ne!(original.modified_ns,modified.modified_ns);assert_ne!(key,modified.filename());
        file.set_len(original.size+1).unwrap();let resized=WaveIdentity::read(&path).unwrap();assert_ne!(resized.size,original.size);assert_ne!(modified.filename(),resized.filename());
    }
    #[test]
    fn waveform_cache_eviction_bounds_entries_and_bytes(){
        let temp=TestDir::new();
        for i in 0..10{fs::write(temp.0.join(format!("{i}.json")),vec![0u8;100]).unwrap();}
        fs::write(temp.0.join("unrelated.txt"),b"keep").unwrap();
        trim_cache(&temp.0,4,250).unwrap();
        let records:Vec<_>=fs::read_dir(&temp.0).unwrap().map(Result::unwrap).filter(|e|e.path().extension().is_some_and(|s|s=="json")).collect();
        assert_eq!(records.len(),2);assert!(temp.0.join("unrelated.txt").exists());
        trim_cache(&temp.0,1,1000).unwrap();
        assert_eq!(fs::read_dir(&temp.0).unwrap().count(),2);
    }
    #[test]
    fn unavailable_cache_does_not_block_waveform(){
        let temp=TestDir::new();let invalid=temp.0.join("file-not-directory");fs::write(&invalid,b"x").unwrap();
        assert!(cached_waveform(&sample("pcm-44100.wav"),Some(&invalid)).is_ok());
    }
    fn sample(name:&str)->PathBuf{Path::new(env!("CARGO_MANIFEST_DIR")).join("../build/s1-samples").join(name)}
    #[test]
    fn unknown_render_preview_is_rejected_before_starting_player_thread(){
        let temp=TestDir::new();let render=crate::layer_render::LayerRenderState::new(temp.0.join("render-cache"));let player=PlayerState::new();
        for id in ["", "../source.wav", "render:unknown", "00000000-0000-0000-0000-000000000000"]{
            assert!(request_render_preview(&render,&player,id).is_err(),"unknown render id {id:?} must not reach playback");
        }
        assert!(player.inner.sender.lock().unwrap().is_none());let snapshot=player.snapshot().unwrap();assert_eq!(snapshot.state,"idle");assert!(snapshot.id.is_none());
    }
    #[test]
    fn new_player_is_idle_without_opening_device(){let state=PlayerState::new();let s=state.snapshot().unwrap();assert_eq!(s.state,"idle");assert_eq!(s.volume,0.15);assert!(state.inner.sender.lock().unwrap().is_none());}
    #[test]
    fn controls_reject_invalid_and_out_of_range_values(){
        for value in [f64::NAN,f64::INFINITY,-1.0,1.1]{assert!(validate_control("volume",Some(value),None).is_err());}
        assert!(validate_control("seek",Some(-0.1),Some(5.0)).is_err());assert!(validate_control("seek",Some(5.1),Some(5.0)).is_err());
        assert!(validate_control("seek",Some(5.0),Some(5.0)).is_ok());assert!(validate_control("loop",Some(0.5),None).is_err());assert!(validate_control("unknown",None,None).is_err());
    }
    #[test]
    fn waveform_reads_four_formats_without_sound_device(){
        for name in ["pcm-44100.wav","pcm-48000.aiff","lossless-48000.flac","lossy-44100.mp3"]{
            let path=sample(name);let before=std::fs::read(&path).unwrap();let wave=analyze_waveform(&path).unwrap();
            assert_eq!(wave.peaks.len(),WAVE_MAX_PEAKS);assert!(wave.peaks.iter().all(|v|v.is_finite()&&(0.0..=1.0).contains(v)));assert!(wave.peaks.iter().any(|v|*v>0.0));
            assert!((0.4..0.7).contains(&wave.analyzed_duration));assert!(!wave.truncated);assert_eq!(before,std::fs::read(&path).unwrap());
        }
    }
    #[test]
    fn mp3_waveform_uses_same_gapless_timeline_as_playback(){
        let path=sample("lossy-44100.mp3");let before=fs::read(&path).unwrap();let wave=analyze_waveform(&path).unwrap();
        assert_eq!(WAVE_CACHE_VERSION,3,"含MP3填充的旧峰值不得复用");
        assert!((wave.analyzed_duration-0.5).abs()<1e-9,"波形时间轴必须只覆盖有效0.5秒，实际{}",wave.analyzed_duration);
        assert_eq!(wave.duration,Some(0.5));assert!(!wave.truncated);assert_eq!(wave.peaks.len(),WAVE_MAX_PEAKS);
        assert_eq!(fs::read(&path).unwrap(),before);
    }
    #[test]
    fn waveform_busy_guard_releases_on_error(){let busy=Arc::new(AtomicBool::new(true));{let _guard=WaveGuard(busy.clone());assert!(analyze_waveform(Path::new("missing-waveform-input.wav")).is_err());}assert!(!busy.load(Ordering::Acquire));}
    #[test]
    fn waveform_stops_at_sixty_seconds_without_output(){
        let wave=analyze_source(rodio::source::SineWave::new(440.0),None).unwrap();
        assert!(wave.truncated);assert_eq!(wave.peaks.len(),WAVE_MAX_PEAKS);assert!((wave.analyzed_duration-60.0).abs()<0.001);
    }
    #[test]
    fn volume_and_stop_commands_do_not_require_a_device(){
        let player=PlayerState::new();
        let volume=player.request(Command::Control{action:"volume".into(),value:Some(0.25)}).unwrap();
        assert_eq!(volume.volume,0.25);assert_eq!(volume.state,"idle");
        let stopped=player.request(Command::Control{action:"stop".into(),value:None}).unwrap();
        assert_eq!(stopped.state,"stopped");assert!(stopped.id.is_none());
    }
}
