// Mock IPC only: no native playback or real drag is exercised.
import {test} from "node:test";import assert from "node:assert/strict";
import {executeProbe,normalizeTestPath} from "../src/probe-session.ts";
test("absolute path normalization and malformed input rejection",()=>{
 assert.equal(normalizeTestPath(' "D:\\测试\\样本.wav" '),"D:\\测试\\样本.wav");
 assert.equal(normalizeTestPath("\\\\server\\share\\test.wav"),"\\\\server\\share\\test.wav");
 for(const s of ["","file.wav","D:relative.wav","D:/a.wav\nD:/b.wav"])assert.throws(()=>normalizeTestPath(s));
});
test("synchronous guard rejects double invocation before rerender",async()=>{
 const lock={busy:false},events=[],busy=[];let release;let calls=0;
 const mock=()=>{calls++;return new Promise(resolve=>{release=resolve;});};
 const first=executeProbe(lock,"test_tone",undefined,mock,e=>events.push(e),v=>busy.push(v),1000);
 assert.equal(await executeProbe(lock,"test_tone",undefined,mock,()=>{},()=>{}),false);
 assert.equal(calls,1);assert.equal(lock.busy,true);release("mock success");await first;
 assert.equal(lock.busy,false);assert.deepEqual(busy,[true,false]);assert.equal(events.at(-1).phase,"success");
});
test("timeout keeps lock and late response releases it",async()=>{
 const lock={busy:false},events=[];let release;
 const first=executeProbe(lock,"play_sample",{path:"D:/test.wav"},()=>new Promise(resolve=>{release=resolve;}),e=>events.push(e),()=>{},5);
 await new Promise(resolve=>setTimeout(resolve,25));
 assert.equal(events.at(-1).phase,"waiting");assert.equal(lock.busy,true);
 assert.equal(await executeProbe(lock,"test_tone",undefined,async()=>{throw Error("must not invoke");},()=>{},()=>{}),false);
 release("mock late response");await first;assert.equal(events.at(-1).phase,"success");assert.match(events.at(-1).message,/等待较久/);assert.equal(lock.busy,false);
});
test("rejection reports error and path, clears timeout and unlocks",async()=>{
 const lock={busy:false},events=[];
 await executeProbe(lock,"decode_sample",{path:"D:/missing.wav"},async()=>{throw Error("file missing");},e=>events.push(e),()=>{},10);
 assert.equal(events.at(-1).phase,"error");assert.match(events.at(-1).message,/file missing/);assert.equal(events.at(-1).path,"D:/missing.wav");assert.equal(lock.busy,false);
 await new Promise(resolve=>setTimeout(resolve,20));assert.equal(events.at(-1).phase,"error");
});
