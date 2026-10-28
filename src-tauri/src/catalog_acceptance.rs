//! 已授权真实库的并行验收；仅显式运行，不修改正式应用目录。
use super::*;
use std::time::Instant;

#[test]
#[ignore = "需显式提供SW_BENCH_ROOT与SW_BENCH_DATA_DIR"]
fn real_parallel_search_benchmark(){
    let source=preview_root(&std::env::var("SW_BENCH_ROOT").expect("缺少授权源目录")).unwrap();
    let output=PathBuf::from(std::env::var("SW_BENCH_DATA_DIR").expect("缺少授权数据目录"));
    assert!(output.is_absolute());fs::create_dir_all(&output).unwrap();
    let output=fs::canonicalize(output).unwrap();assert!(!output.starts_with(Path::new(&source)));
    let run=output.join(format!("parallel-{}",Uuid::new_v4()));fs::create_dir(&run).unwrap();
    let db=run.join("benchmark.sqlite");let state=CatalogState::new(db.clone()).unwrap();
    fn wait(state:&CatalogState){let deadline=Instant::now()+Duration::from_secs(300);loop{if state.inner.active.lock().unwrap().is_none(){break;}assert!(Instant::now()<deadline,"扫描超时");std::thread::sleep(Duration::from_millis(5));}}
    let seed=state.start(source.clone(),None).unwrap();wait(&state);
    let first=jobs(&db).unwrap().into_iter().find(|j|j.id==seed).unwrap();assert_eq!(first.failed,0);assert_eq!(first.status,"completed");
    let conn=connection(&db).unwrap();
    let mut stmt=conn.prepare("SELECT name,path,size,modified FROM assets ORDER BY path").unwrap();
    let snapshot:Vec<(String,String,u64,String)>=stmt.query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap().collect::<Result<_,_>>().unwrap();drop(stmt);drop(conn);
    assert!(!snapshot.is_empty());
    let queries:Vec<String>=(0..100).map(|i|snapshot[i*snapshot.len()/100].0.chars().take(128).collect()).collect();
    let expected:Vec<u64>=queries.iter().map(|q|search_page(&db,0,100,SearchFilter{query:Some(q),..Default::default()}).unwrap().total).collect();
    let first_started=Instant::now();let _=search_page(&db,0,100,SearchFilter{query:Some(&queries[0]),..Default::default()}).unwrap();let reopened_query_ms=first_started.elapsed().as_secs_f64()*1000.0;
    let done=Arc::new(AtomicBool::new(false));let stop=done.clone();let worker_state=state.clone();
    let (started,ready)=std::sync::mpsc::sync_channel(1);
    let worker=std::thread::spawn(move||{
        let mut scans=Vec::new();let mut signal=Some(started);
        while !stop.load(Ordering::Acquire){
            let begin=Instant::now();let id=worker_state.start(source.clone(),None).unwrap();
            if let Some(sender)=signal.take(){let _=sender.send(());}
            wait(&worker_state);
            let job=jobs(&worker_state.inner.db_path).unwrap().into_iter().find(|j|j.id==id).unwrap();
            assert_eq!(job.failed,0);assert_eq!(job.status,"completed");
            scans.push(serde_json::json!({"seconds":begin.elapsed().as_secs_f64(),"job":job}));
        }scans
    });
    ready.recv_timeout(Duration::from_secs(30)).unwrap();
    let mut measures=Vec::new();let mut timings=Vec::new();let mut overlapping=0usize;let mut consistent=true;
    for round in 0..5{for (index,q) in queries.iter().enumerate(){
        let active=state.inner.active.lock().unwrap().is_some();if active{overlapping+=1;}
        let begin=Instant::now();let page=search_page(&db,0,100,SearchFilter{query:Some(q),..Default::default()}).unwrap();let ms=begin.elapsed().as_secs_f64()*1000.0;
        consistent&=page.total==expected[index];timings.push(ms);
        measures.push(serde_json::json!({"round":round+1,"query":q,"ms":ms,"scan_active_at_start":active,"total":page.total}));
    }}
    done.store(true,Ordering::Release);let scans=worker.join().expect("后台扫描线程失败");
    let unchanged=snapshot.iter().all(|(_,path,size,modified)|fs::metadata(path).ok().is_some_and(|m|m.len()==*size&&metadata_stamp(&m).ok().as_ref()==Some(modified)));
    timings.sort_by(f64::total_cmp);let p95=timings[(timings.len()*95).div_ceil(100)-1];
    let result=serde_json::json!({"kind":"real_parallel_search","files":snapshot.len(),"database":db,"query_count":queries.len(),"rounds":5,"overlapping_queries":overlapping,"total_queries":timings.len(),"query_p95_ms":p95,"query_max_ms":timings.last(),"fresh_connection_query_ms":reopened_query_ms,"consistent":consistent,"source_metadata_unchanged":unchanged,"background_scans":scans,"measurements":measures,"limitations":"温热OS缓存；每次查询重新打开SQLite连接。不是OS冷缓存、IPC/UI或声卡并行测试；真实数量不足一万条时不视为万条验收。未逐文件内容哈希。"});
    fs::write(run.join("parallel-report.json"),serde_json::to_vec_pretty(&result).unwrap()).unwrap();
    eprintln!("并行验收 {}：{}条，重叠查询{}/500，P95 {:.3}ms",run.display(),snapshot.len(),overlapping,p95);
    assert!(consistent&&unchanged);assert!(overlapping>0);assert!(p95<=300.0,"实测查询P95超出300ms目标，需优化");
}
