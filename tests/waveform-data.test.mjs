import test from 'node:test';
import assert from 'node:assert/strict';
import {createWaveformScheduler,waveformKey,envelopePath,fractionAt,waveformPosition} from '../src/waveform-data.ts';
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
test('continuous envelope uses real amplitudes with no synthetic floor; pointer maps only analyzed duration',()=>{
 const path=envelopePath([0,.5,1]);
 assert.match(path,/M0\.000,50\.000 L500\.000,26\.000 L1000\.000,2\.000/);
 assert.match(path,/L1000\.000,98\.000 L500\.000,74\.000 L0\.000,50\.000 Z$/);
 assert.equal(envelopePath([]),'');assert.equal(envelopePath([0,0]).includes('50.000'),true);
 assert.equal(fractionAt(50,100,200),0);assert.equal(fractionAt(200,100,200),.5);assert.equal(fractionAt(400,100,200),1);
 assert.equal(fractionAt(1,0,0),0);assert.equal(waveformPosition(.25,60),15);assert.equal(waveformPosition(2,60),60);
});
