//! S1.0 bounded, read-only audio probes. No library scanning or source mutation.
use rodio::{Decoder, OutputStream, Sink, Source};
use std::{
    fs::File,
    io::{BufReader, Read},
    path::Path,
    time::{Duration, Instant},
};

const LIMIT_SECONDS: u64 = 3;

#[derive(Debug)]
pub struct DecodeReport {
    pub container: &'static str,
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: u64,
    pub peak: f32,
    pub reached_limit: bool,
}

impl DecodeReport {
    fn summary(&self) -> String {
        format!(
            "容器头识别：{}\n{} Hz / {} 声道\n已解码 {} 帧（{:.3} 秒）\n归一化样本峰值：{:.6}\n范围：{}；不代表全文件完整性或真实听感通过。",
            self.container,
            self.sample_rate,
            self.channels,
            self.frames,
            self.frames as f64 / self.sample_rate as f64,
            self.peak,
            if self.reached_limit { "最多前 3 秒" } else { "解码器返回的有限样本" }
        )
    }
}

fn container_header(path: &Path) -> Result<&'static str, String> {
    let mut file = File::open(path).map_err(|e| format!("读取失败：{e}"))?;
    let mut header = [0u8; 12];
    let count = file.read(&mut header).map_err(|e| format!("读取文件头失败：{e}"))?;
    Ok(if count >= 12 && &header[..4] == b"RIFF" && &header[8..12] == b"WAVE" {
        "WAV"
    } else if count >= 12 && &header[..4] == b"FORM"
        && (&header[8..12] == b"AIFF" || &header[8..12] == b"AIFC") {
        "AIFF/AIFC"
    } else if count >= 4 && &header[..4] == b"fLaC" {
        "FLAC"
    } else if count >= 3 && &header[..3] == b"ID3"
        || count >= 2 && header[0] == 0xff && header[1] & 0xe0 == 0xe0 {
        "MPEG/ID3（具体编码以解码结果为准）"
    } else {
        "未知（由解码器探测）"
    })
}

fn open_decoder(path: &Path) -> Result<Decoder<BufReader<File>>, String> {
    let file = File::open(path).map_err(|e| format!("读取失败：{e}"))?;
    Decoder::new(BufReader::new(file)).map_err(|e| format!("解码失败：{e}"))
}

/// Decode at most three seconds, without opening an output device.
/// Rodio's iterator cannot certify corruption-free EOF; this is a bounded probe.
pub fn decode_path(path: &Path) -> Result<DecodeReport, String> {
    let container = container_header(path)?;
    let decoder = open_decoder(path)?;
    let sample_rate = decoder.sample_rate();
    let channels = decoder.channels();
    if sample_rate == 0 || channels == 0 {
        return Err("解码器返回无效采样率或声道数".into());
    }
    let limit = sample_rate as u64 * channels as u64 * LIMIT_SECONDS;
    let mut samples = 0u64;
    let mut peak = 0f32;
    for sample in decoder.convert_samples::<f32>().take(limit as usize) {
        if !sample.is_finite() {
            return Err("解码样本含非有限数值".into());
        }
        peak = peak.max(sample.abs());
        samples += 1;
    }
    if samples == 0 {
        return Err("文件未产生可播放的解码样本".into());
    }
    if samples % channels as u64 != 0 {
        return Err("解码输出结束于不完整的音频帧".into());
    }
    Ok(DecodeReport {
        container, sample_rate, channels,
        frames: samples / channels as u64, peak,
        reached_limit: samples == limit,
    })
}

#[tauri::command]
pub async fn decode_sample(path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || decode_path(Path::new(&path)).map(|r| r.summary()))
        .await.map_err(|e| format!("解码任务失败：{e}"))?
}

fn wait_for_output(sink: &Sink) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !sink.empty() {
        if Instant::now() >= deadline {
            sink.stop();
            return Err("声卡输出等待超过8秒，已停止本次播放；请检查音频设备。".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    Ok(())
}
#[tauri::command]
pub async fn test_tone() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let (_stream, handle) = OutputStream::try_default().map_err(|e| format!("声卡初始化失败：{e}"))?;
        let sink = Sink::try_new(&handle).map_err(|e| e.to_string())?;
        sink.append(rodio::source::SineWave::new(440.0)
            .take_duration(Duration::from_millis(500)).amplify(0.05));
        wait_for_output(&sink)?;
        Ok("0.5 秒低音量测试音已提交默认声卡并结束；实际听感待人工确认。".into())
    }).await.map_err(|e| format!("播放任务失败：{e}"))?
}

#[tauri::command]
pub async fn play_sample(path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let report = decode_path(Path::new(&path))?;
        let decoder = open_decoder(Path::new(&path))?;
        let (_stream, handle) = OutputStream::try_default().map_err(|e| format!("声卡初始化失败：{e}"))?;
        let sink = Sink::try_new(&handle).map_err(|e| e.to_string())?;
        sink.set_volume(0.15);
        sink.append(decoder.take_duration(Duration::from_secs(LIMIT_SECONDS)));
        wait_for_output(&sink)?;
        Ok(format!("{}\n播放请求已结束；实际听感待人工确认。", report.summary()))
    }).await.map_err(|e| format!("播放任务失败：{e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn samples() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../build/s1-samples")
    }

    #[test]
    fn four_generated_formats_decode_with_expected_signal() {
        let cases = [
            ("pcm-44100.wav", 44100, "WAV"),
            ("pcm-48000.aiff", 48000, "AIFF/AIFC"),
            ("lossless-48000.flac", 48000, "FLAC"),
            ("lossy-44100.mp3", 44100, "MPEG/ID3（具体编码以解码结果为准）"),
        ];
        for (name, expected_rate, expected_container) in cases {
            let report = decode_path(&samples().join(name))
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(report.sample_rate, expected_rate, "{name}");
            assert_eq!(report.channels, 1, "{name}");
            assert_eq!(report.container, expected_container, "{name}");
            let seconds = report.frames as f64 / report.sample_rate as f64;
            assert!((0.45..=0.65).contains(&seconds), "{name}: {seconds}");
            assert!((0.05..=0.25).contains(&report.peak), "{name}: peak {}", report.peak);
            assert!(!report.reached_limit, "{name}");
        }
    }


    #[test]
    fn five_second_wav_is_bounded_to_three_seconds() {
        let report = decode_path(&samples().join("long-44100.wav"))
            .expect("five-second synthetic WAV must decode");
        assert_eq!(report.sample_rate, 44100);
        assert_eq!(report.channels, 1);
        assert_eq!(report.frames, 132300, "probe must stop at exactly three seconds");
        assert!(report.reached_limit, "long sample must report the bounded analysis");
        assert!((0.05..=0.25).contains(&report.peak));
    }

    #[test]
    fn malformed_file_is_rejected_without_output_device() {
        let path = samples().join(format!("invalid-probe-{}.wav", std::process::id()));
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true)
            .open(&path).expect("create dedicated malformed test fixture");
        file.write_all(b"this is not a WAV or any audio file").unwrap();
        drop(file);
        let result = decode_path(&path);
        std::fs::remove_file(&path).expect("remove only this test's generated fixture");
        assert!(result.is_err(), "malformed audio must not be accepted");
    }

    #[test]
    fn missing_file_is_reported() {
        let result = decode_path(&samples().join("does-not-exist-audio-probe.wav"));
        assert!(result.unwrap_err().starts_with("读取失败"));
    }
}
