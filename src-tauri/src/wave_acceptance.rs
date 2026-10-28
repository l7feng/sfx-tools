//! 前段波形缓存的实测，不初始化声卡或操作产品窗口。
use super::*;
#[test]
#[ignore = "需显式指定SW_WAVE_SOURCES_JSON和SW_BENCH_DATA_DIR"]
fn real_wave_cache_benchmark(){
    let paths:Vec<PathBuf>=serde_json::from_str(&std::env::var("SW_WAVE_SOURCES_JSON").expect("缺少授权样本清单")).unwrap();assert!(!paths.is_empty());
    let output=PathBuf::from(std::env::var("SW_BENCH_DATA_DIR").expect("缺少授权数据目录"));assert!(output.is_absolute());
    fs::create_dir_all(&output).unwrap();let output=output.canonicalize().unwrap();
    let run=output.join(format!("wave-cache-{}",uuid::Uuid::new_v4()));fs::create_dir(&run).unwrap();let cache=run.join("cache");let mut records=Vec::new();
    for path in paths{
        let path=path.canonicalize().unwrap();assert!(!run.starts_with(&path));let before=WaveIdentity::read(&path).unwrap();
        let begin=Instant::now();let wave=cached_waveform(&path,Some(&cache)).unwrap();let first_ms=begin.elapsed().as_secs_f64()*1000.0;
        assert!(read_cached(&cache,&before).is_some(),"首次分析没有产生有效缓存");
        assert!(!wave.peaks.is_empty() && wave.peaks.len()<=WAVE_MAX_PEAKS);
        let mut timings=Vec::new();for _ in 0..20{let begin=Instant::now();let cached=cached_waveform(&path,Some(&cache)).unwrap();timings.push(begin.elapsed().as_secs_f64()*1000.0);assert_eq!(cached.peaks,wave.peaks);}
        let mut sorted=timings.clone();sorted.sort_by(f64::total_cmp);let p95=sorted[18];assert_eq!(before,WaveIdentity::read(&path).unwrap());
        records.push(serde_json::json!({"source":path,"bytes":before.size,"algorithm_version":WAVE_CACHE_VERSION,"peak_count":wave.peaks.len(),"peak_limit":WAVE_MAX_PEAKS,"first_analysis_ms":first_ms,"cache_hit_p95_ms":p95,"hit_measurements_ms":timings,"analyzed_seconds":wave.analyzed_duration,"truncated":wave.truncated,"source_metadata_unchanged":true}));
    }
    let result=serde_json::json!({"kind":"real_wave_cache","records":records,"limitations":"独立新缓存；OS文件缓存未清空；只计后台函数，不含IPC/UI。前段最多60秒或2400万采样，不是整段波形。没有打开声卡或播放音频。"});
    fs::write(run.join("wave-report.json"),serde_json::to_vec_pretty(&result).unwrap()).unwrap();eprintln!("波形验收报告 {}",run.display());
    for item in result["records"].as_array().unwrap(){assert!(item["cache_hit_p95_ms"].as_f64().unwrap()<=500.0,"缓存命中超过500ms目标");}
}
