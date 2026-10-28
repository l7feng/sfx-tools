export type ProbeCommand = "native_info" | "test_tone" | "decode_sample" | "play_sample" | "drag_sample";
export type ProbeLock = {busy:boolean};
export type ProbeEvent = {command:ProbeCommand;path?:string;phase:"running"|"waiting"|"success"|"error";message:string;at:string};
export function normalizeTestPath(value:string):string {
 const s=value.trim(); const path=s.startsWith('"')&&s.endsWith('"')?s.slice(1,-1):s;
 if(!path||/[\r\n\0]/.test(path))throw Error("请输入单个测试音频的完整路径。");
 if(!/^[a-zA-Z]:[\\/]/.test(path)&&!/^\\\\[^\\]+\\[^\\]+/.test(path))throw Error("需要 Windows 绝对路径，例如 D:/测试目录/样本.wav。");
 return path;
}
export async function executeProbe(lock:ProbeLock,command:ProbeCommand,args:Record<string,unknown>|undefined,invoke:(cmd:string,args?:Record<string,unknown>)=>Promise<string>,onEvent:(e:ProbeEvent)=>void,onBusy:(v:boolean)=>void,timeoutMs=15000):Promise<boolean>{
 if(lock.busy)return false;
 lock.busy=true; onBusy(true); // synchronous guard before the first await / React rerender
 const path=typeof args?.path==="string"?args.path:undefined;
 const emit=(phase:ProbeEvent["phase"],message:string)=>onEvent({command,path,phase,message,at:new Date().toISOString()});
 emit("running",command==="drag_sample"?"拖出处理中；松开鼠标完成或按 Esc 取消，等待原生结果。":"请求已发出，等待原生结果。");
 let timedOut=false;
 const timer=setTimeout(()=>{timedOut=true;emit("waiting","等待已超过 15 秒。后端尚未返回，任务未被取消；操作继续锁定，避免重复播放或拖出。");},timeoutMs);
 try{const result=await invoke(command,args);emit("success",(timedOut?"等待较久后收到结果：\n":"")+result);}
 catch(error){const message=error instanceof Error?error.message:typeof error==="string"?error:JSON.stringify(error)||String(error);emit("error","原生请求失败："+message);}
 finally{clearTimeout(timer);lock.busy=false;onBusy(false);}
 return true;
}
