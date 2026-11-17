import React from 'react';
import './crash-screen.css';

/** 把一条崩溃信息写进 %APPDATA%\sfx-tools\crash.log；浏览器态只打控制台。 */
async function reportCrash(kind:string,detail:string){
  const text=`[${new Date().toISOString()}] ${kind}\n${detail}\nUA: ${navigator.userAgent}\nURL: ${location.href}\n`;
  console.error(`[sfx-tools] ${kind}\n${detail}`);
  try{
    const {invoke,isTauri}=await import('@tauri-apps/api/core');
    if(isTauri()) return await invoke<string>('crash_log',{text});
  }catch{ /* 日志写不进去也不能再抛，否则原地递归 */ }
  return '';
}

/** 装上全局兜底：未捕获异常与未处理的 Promise 都落一封日志。应用启动时调用一次即可。 */
export function installCrashHandlers(){
  window.addEventListener('error',event=>{
    void reportCrash('window.onerror',`${event.message}\n${event.filename}:${event.lineno}:${event.colno}\n${event.error?.stack??''}`);
  });
  window.addEventListener('unhandledrejection',event=>{
    const reason=event.reason;
    void reportCrash('unhandledrejection',reason?.stack??String(reason));
  });
}

type CrashState={error:Error|null;stack:string;logPath:string;copied:boolean};

/** 渲染异常兜底：白屏换成一张能读的卡片，并把栈写进日志。 */
export class ErrorBoundary extends React.Component<{children:React.ReactNode},CrashState>{
  state:CrashState={error:null,stack:'',logPath:'',copied:false};
  static getDerivedStateFromError(error:Error){return {error};}
  componentDidCatch(error:Error,info:React.ErrorInfo){
    const stack=`${error.stack??error.message}\n\n组件栈：\n${info.componentStack??''}`;
    this.setState({stack});
    void reportCrash('react.render',stack).then(logPath=>this.setState({logPath}));
  }
  private copy=async()=>{
    const text=`${this.state.error?.message??''}\n${this.state.stack}`;
    try{ await navigator.clipboard.writeText(text); this.setState({copied:true}); setTimeout(()=>this.setState({copied:false}),1600); }catch{}
  };
  render(){
    if(!this.state.error) return this.props.children;
    const message=this.state.error.message||String(this.state.error);
    return <div className="crash-screen" role="alert">
      <div className="crash-card">
        <h1>界面出错了</h1>
        <p className="crash-lead">这不是你的操作问题。错误信息已写进日志，重新加载通常可以恢复；如果反复出现，把下面的日志发给开发者。</p>
        <p className="crash-message">{message}</p>
        <pre className="crash-stack">{this.state.stack.slice(0,1400)}</pre>
        <div className="crash-actions">
          <button type="button" className="crash-primary" onClick={()=>location.reload()}>重新加载</button>
          <button type="button" onClick={()=>void this.copy()}>{this.state.copied?'已复制':'复制错误信息'}</button>
        </div>
        {this.state.logPath?<p className="crash-path">日志：{this.state.logPath}</p>
          :<p className="crash-path">日志未能落盘（浏览器预览态不会写文件）。</p>}
      </div>
    </div>;
  }
}
