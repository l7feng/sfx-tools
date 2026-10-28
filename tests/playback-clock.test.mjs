import test from 'node:test';
import assert from 'node:assert/strict';
import {playbackPositionAt,PLAYBACK_CLOCK_MAX_AHEAD_MS} from '../src/playback-clock.ts';
const playing={id:'source-a',state:'playing',position:2,duration:10};
test('显示时钟平滑前进但旧快照最多领先0.5秒',()=>{
 assert.equal(playbackPositionAt(playing,0),2);assert.equal(playbackPositionAt(playing,125),2.125);
 assert.equal(playbackPositionAt(playing,250),2.25);assert.equal(playbackPositionAt(playing,500),2.5);
 assert.equal(playbackPositionAt(playing,5000),2.5);assert.equal(PLAYBACK_CLOCK_MAX_AHEAD_MS,500);
});
test('暂停停止和无来源不插值，真实倒退定位立即成为新锚点',()=>{
 for(const state of ['paused','stopped','ended','idle','error'])assert.equal(playbackPositionAt({...playing,state},400),2);
 assert.equal(playbackPositionAt({...playing,id:null},400),2);assert.equal(playbackPositionAt({...playing,id:''},400),2);
 assert.equal(playbackPositionAt({...playing,position:.25},0),.25);assert.equal(playbackPositionAt({...playing,id:'source-b',position:0},100),.1);
});
test('显示值受真实时长约束，非有限值不扩散为NaN或无穷',()=>{
 assert.equal(playbackPositionAt({...playing,position:9.9},300),10);
 assert.equal(playbackPositionAt({...playing,position:20},0),10);
 assert.equal(playbackPositionAt({...playing,duration:0},200),0);
 assert.equal(playbackPositionAt({...playing,duration:null},300),2.3);
 for(const value of [NaN,Infinity,-Infinity,-20])assert.equal(playbackPositionAt(playing,value),2);
 assert.equal(playbackPositionAt({...playing,position:NaN},200),.2);
 assert.equal(playbackPositionAt({...playing,position:-3,duration:Infinity},100),.1);
});
