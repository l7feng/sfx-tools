import React,{useRef,useState} from "react";
import {createRoot} from "react-dom/client";
import {invoke} from "@tauri-apps/api/core";
import {executeProbe,normalizeTestPath} from "./probe-session";
import type {ProbeCommand,ProbeEvent,ProbeLock} from "./probe-session";
import "./probe.css";
const names:Record<ProbeCommand,string>={native_info:"Rust 原生调用",test_tone:"声卡测试音",decode_sample:"文件解码",play_sample:"样本播放",drag_sample:"Windows 文件拖出"};
function App(){
 const lock=useRef<ProbeLock>({busy:false});
 const [path,setPath]=useState(""),[busy,setBusy]=useState(false),[event,setEvent]=useState<ProbeEvent|null>(null),[inputError,setInputError]=useState(""),[history,setHistory]=useState<ProbeEvent[]>([]);
 const receive=(next:ProbeEvent)=>{setEvent(next);setHistory(old=>[next,...old].slice(0,24));};
 function run(command:ProbeCommand){
  if(lock.current.busy)return;
  let args:Record<string,unknown>|undefined;
  if(["decode_sample","play_sample","drag_sample"].includes(command)){
   try{args={path:normalizeTestPath(path)};setInputError("");}catch(error){setInputError((error as Error).message);return;}
  }
  void executeProbe(lock.current,command,args,(cmd,input)=>invoke<string>(cmd,input),receive,setBusy);
 }
 return <main aria-busy={busy}><header><span>SFX TOOLS / S1.0</span><h1>原生能力验证</h1><a href="?">返回素材工作台</a><p>工程验证窗口。每次只执行一个请求；记录仅保留在本窗口，关闭后不保存。</p></header>
 <section><h2>01 · 桌面通路</h2><button disabled={busy} onClick={()=>run("native_info")}>检查 Rust 原生调用</button></section>
 <section><h2>02 · 声卡输出</h2><p>播放 0.5 秒、低音量 440 Hz 合成测试音。请确认耳机或音箱音量。</p><button disabled={busy} onClick={()=>run("test_tone")}>播放测试音</button></section>
 <section><h2>03 · 文件解码与播放</h2><label htmlFor="sample-path">测试音频的完整路径</label><input id="sample-path" value={path} onChange={e=>{setPath(e.target.value);setInputError("");}} placeholder="D:/Ai-Files/Repos/GitHub/sound-workbench/build/s1-samples/pcm-44100.wav" disabled={busy} spellCheck={false} aria-invalid={!!inputError} aria-describedby="path-help path-error"/><p id="path-help">仅使用专用测试样本。支持粘贴带双引号的完整路径；输入存在不代表文件已验证。最多分析或播放前 3 秒，不改写文件。</p><p id="path-error" className="error" role="alert">{inputError}</p>
 <div className="actions"><button disabled={busy||!path.trim()} onClick={()=>run("decode_sample")}>仅解码检查</button><button disabled={busy||!path.trim()} onClick={()=>run("play_sample")}>播放样本</button></div></section>
 <section><h2>04 · Windows 文件拖出</h2><p id="drag-help">仅允许工程 build/s1-samples 内测试文件。按住鼠标左键，将按钮拖到资源管理器测试文件夹后松开；Esc 取消。请求复制不代表目标已接收，需核验目标文件。</p><button className="drag-button" disabled={busy||!path.trim()} draggable={false} aria-describedby="drag-help" onPointerDown={e=>{if(!e.isTrusted||e.pointerType!=="mouse"||e.button!==0||e.buttons!==1||lock.current.busy||!path.trim())return;e.preventDefault();run("drag_sample");}}>按住鼠标左键拖出测试文件</button></section>
 <output role="status" aria-live="polite" data-phase={event?.phase??"idle"}><strong>{event?names[event.command]:"等待验证"}{busy?" · 操作锁定":""}</strong><span>{event?.message??"先检查 Rust 原生调用，然后选择专用测试样本。"}</span>{event?.path&&<small>{event.path}</small>}</output>
 {event?.phase==="waiting"&&<p className="timeout-note">目前没有前端强制取消接口。请等待结果；不要通过新窗口重复发起播放。若持续无响应，保留记录后由开发者检查原生进程。</p>}
 <details><summary>本次窗口验证记录（最多 24 条，最新在上）</summary><pre>{history.map(item=>`[${item.at}] ${names[item.command]} / ${item.phase}\n${item.path?"路径："+item.path+"\n":""}${item.message}`).join("\n\n")||"暂无记录"}</pre></details>
 <footer>按钮存在或请求成功均不代表验收通过。真实听感、四格式播放和目标文件接收，以项目验证记录为准。</footer></main>;
}
createRoot(document.getElementById("root")!).render(<React.StrictMode><App/></React.StrictMode>);
