//! 自算音频时长。
//!
//! 起因：rodio 0.20.1 的 `Decoder::total_duration()` 在 symphonia 分支里把小数部分取倒数
//! （`Duration::new(seconds, (1f64 / frac) as u32)`）。frac=0.5 得 2 纳秒，frac=0.0 时
//! `1/0` 饱和成 u32::MAX，凭空多出 4.294967295 秒。实测四个 0.5 秒样本全部读成 2 纳秒、
//! 一个 5 秒样本读成 9.294967295 秒。这里只读文件头，用 symphonia 的帧数与时间基自己算，
//! 不整段解码，也不猜：拿不到就返回 None，让界面显示"未知"。

use std::{fs::File, path::Path};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::probe::Hint;

/// 返回秒数；文件头不足以确定时长时返回 None。
pub fn seconds(path: &Path) -> Option<f64> {
    let file = File::open(path).ok()?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, stream, &Default::default(), &Default::default())
        .ok()?;
    let params = &probed.format.default_track()?.codec_params;
    let base = params.time_base?;
    // MP3 的帧数含编码器前导延迟与尾部填充，按 gapless 信息扣掉才是真实长度；
    // WAV/FLAC/AIFF 这两个字段为空，扣减量为 0。
    let skip = params.delay.unwrap_or(0) as u64 + params.padding.unwrap_or(0) as u64;
    let frames = params.n_frames?.saturating_sub(skip);
    if frames == 0 {
        return None;
    }
    let time = base.calc_time(frames);
    let value = time.seconds as f64 + time.frac;
    // 不接受非有限值或零，宁可显示未知，也不写一个假数。
    (value.is_finite() && value > 0.0).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::seconds;
    use std::path::{Path, PathBuf};

    fn sample(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../build/s1-samples").join(name)
    }

    #[test]
    fn four_formats_report_real_duration() {
        for (name, expected) in [
            ("pcm-44100.wav", 0.5),
            ("pcm-48000.aiff", 0.5),
            ("lossless-48000.flac", 0.5),
            ("lossy-44100.mp3", 0.5),
            ("long-44100.wav", 5.0),
        ] {
            let value = seconds(&sample(name)).unwrap_or_else(|| panic!("{name} 未取得时长"));
            assert!(
                (value - expected).abs() < 0.02,
                "{name} 时长 {value} 与实测 {expected} 不符"
            );
        }
    }

    #[test]
    fn missing_or_non_audio_file_returns_none() {
        assert!(seconds(Path::new("no-such-audio-file.wav")).is_none());
        let dir = std::env::temp_dir().join("sound-workbench-duration-probe");
        std::fs::create_dir_all(&dir).unwrap();
        let junk = dir.join("junk.wav");
        std::fs::write(&junk, b"not audio at all").unwrap();
        assert!(seconds(&junk).is_none());
    }
}
