import test from 'node:test';
import assert from 'node:assert/strict';
import {createWaveformScheduler,waveformKey,envelopePath,fractionAt,waveformPosition,withWaveformRetry,isTransientWaveformError} from '../src/waveform-data.ts';
const asset=(id,modified='1')=>({id,size:100,modified});
const wave={peaks:[0,.5,1,0],duration:4,analyzed_duration:4,truncated:false};
const tick=()=>new Promise(resolve=>setImmediate(resolve));
function harness(queue=24,cache=64){
 const started=[],pending=[];
 const scheduler=createWaveformScheduler(id=>{started.push(id);return new Promise((resolve,reject)=>pending.push({resolve,reject}));},queue,cache);
 return {get:scheduler.get,started,pending};
}
test('same-file requests share one decode; all jobs globally serialize and priority promotes selection',async()=>{
 const h=harness();const a=h.get(asset('a')),duplicate=h.get(asset('a'));
 const low=h.get(asset('low')),high=h.get(asset('high'),10);
 await tick();assert.deepEqual(h.started,['a']);
 h.pending.shift().resolve(wave);assert.equal(await a,wave);assert.equal(await duplicate,wave);
 await tick();assert.deepEqual(h.started,['a','high']);h.pending.shift().resolve(wave);await high;
 await tick();assert.deepEqual(h.started,['a','high','low']);h.pending.shift().resolve(wave);await low;
});
test('bounded queue rejects overflow, promotes explicit selection without disrupting active job',async()=>{
 const h=harness(1);const active=h.get(asset('active'));const low=h.get(asset('low'));
 const lowRejected=assert.rejects(low,/让位/);
 await assert.rejects(h.get(asset('extra')),/繁忙/);
 const chosen=h.get(asset('chosen'),10);await lowRejected;
 await tick();h.pending.shift().resolve(wave);await active;
 await tick();assert.deepEqual(h.started,['active','chosen']);h.pending.shift().resolve(wave);await chosen;
});
test('aborting invisible row removes queued work; aborting one subscriber leaves other subscriber intact',async()=>{
 const h=harness();const active=h.get(asset('active'));
 const queuedAbort=new AbortController();const queued=h.get(asset('invisible'),0,queuedAbort.signal);
 const queuedRejected=assert.rejects(queued,/取消/);queuedAbort.abort();await queuedRejected;
 const subscriberAbort=new AbortController();const subscriber=h.get(asset('active'),0,subscriberAbort.signal);
 const subscriberRejected=assert.rejects(subscriber,/取消/);subscriberAbort.abort();await subscriberRejected;
 await tick();h.pending.shift().resolve(wave);await active;await tick();assert.deepEqual(h.started,['active']);
});
test('failed decode is retryable, LRU bounded and metadata changes invalidate cached waveform',async()=>{
 const calls=[];let fail=true;
 const h=createWaveformScheduler(async id=>{calls.push(id);if(fail){fail=false;throw new Error('decode');}return wave;},3,1);
 await assert.rejects(h.get(asset('a')),/decode/);
 await h.get(asset('a'));await h.get(asset('a'));assert.deepEqual(calls,['a','a']);
 await h.get(asset('a','2'));assert.deepEqual(calls,['a','a','a']);
 await h.get(asset('b'));await h.get(asset('a','2'));assert.deepEqual(calls,['a','a','a','b','a']);
 assert.notEqual(waveformKey(asset('a','1')),waveformKey(asset('a','2')));
});
test('simple mini waveform is a separate request and cache entry from the full waveform',async()=>{
 const calls=[];
 const scheduler=createWaveformScheduler(async(id,simple)=>{calls.push([id,simple]);return wave;});
 await scheduler.get(asset('a'));
 await scheduler.get(asset('a')); // 完整模式命中缓存
 await scheduler.get(asset('a'),0,undefined,{simple:true});
 await scheduler.get(asset('a'),0,undefined,{simple:true}); // 简化模式命中自己的缓存
 assert.deepEqual(calls,[['a',false],['a',true]]);
});
test('continuous envelope uses real amplitudes with no synthetic floor; pointer maps only analyzed duration',()=>{
 const path=envelopePath([0,.5,1]);
 assert.match(path,/M0\.000,50\.000 L500\.000,26\.000 L1000\.000,2\.000/);
 assert.match(path,/L1000\.000,98\.000 L500\.000,74\.000 L0\.000,50\.000 Z$/);
 assert.equal(envelopePath([]),'');assert.equal(envelopePath([0,0]).includes('50.000'),true);
 assert.equal(fractionAt(50,100,200),0);assert.equal(fractionAt(200,100,200),.5);assert.equal(fractionAt(400,100,200),1);
 assert.equal(fractionAt(1,0,0),0);assert.equal(waveformPosition(.25,60),15);assert.equal(waveformPosition(2,60),60);
});
test('transient waveform contention is retried with backoff instead of showing unavailable',async()=>{
 let calls=0;
 const base=async()=>{calls++;if(calls<3)throw new Error('波形队列繁忙，请稍后重试');return wave;};
 const get=withWaveformRetry(base,[0,0,0]);
 assert.equal(await get(asset('a')),wave);assert.equal(calls,3);
});
test('permanent decode failure is never retried; exhaustion preserves the last transient error',async()=>{
 let permanent=0;
 await assert.rejects(withWaveformRetry(async()=>{permanent++;throw new Error('decode failed');},[0,0])(asset('a')),/decode failed/);
 assert.equal(permanent,1,'永久错误必须立即上抛');
 let busy=0;
 await assert.rejects(withWaveformRetry(async()=>{busy++;throw new Error('波形分析进行中，请稍后重试');},[0,0])(asset('b')),/稍后重试/);
 assert.equal(busy,3,'2 次退避共 3 次尝试');
});
test('waveform retry honors abort before and during backoff',async()=>{
 let calls=0;const get=withWaveformRetry(async()=>{calls++;return wave;},[50]);
 const already=new AbortController();already.abort();
 await assert.rejects(get(asset('a'),0,already.signal),/取消/);assert.equal(calls,0,'已取消的请求不得发起');
 const controller=new AbortController();
 const pending=withWaveformRetry(async()=>{throw new Error('波形请求已让位于当前选中素材，请重试');},[50])(asset('b'),0,controller.signal);
 const rejected=assert.rejects(pending,/取消/);await tick();controller.abort();await rejected;
});
test('only contention messages are classified as transient',()=>{
 assert.equal(isTransientWaveformError(new Error('波形队列繁忙，请稍后重试')),true);
 assert.equal(isTransientWaveformError(new Error('波形请求已让位于当前选中素材，请重试')),true);
 assert.equal(isTransientWaveformError(new Error('波形分析锁不可用')),true);
 assert.equal(isTransientWaveformError(new Error('decode failed')),false);
 assert.equal(isTransientWaveformError('波形队列繁忙'),true);
 assert.equal(isTransientWaveformError(null),false);
});
