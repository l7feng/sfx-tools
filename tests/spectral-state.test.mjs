import test from 'node:test';
import assert from 'node:assert/strict';
import {safeWaveMode,visualMatches,heatColor,frequencyColor} from '../src/spectral-state.ts';
test('模式只接受既有五项，状态必须匹配任务与素材',()=>{for(const mode of ['peaks','rms','spectrum','combined','frequency'])assert.equal(safeWaveMode(mode),mode);assert.equal(safeWaveMode('fake'),'peaks');assert.equal(visualMatches({job_id:'j',asset_id:'a'},'j','a'),true);assert.equal(visualMatches({job_id:'old',asset_id:'a'},'j','a'),false);assert.equal(visualMatches({job_id:'j',asset_id:'old'},'j','a'),false);});
test('频谱颜色有界，静音和频率位置来自数值而非随机',()=>{assert.deepEqual(heatColor(-100),[5,9,15]);assert.deepEqual(heatColor(0),[255,231,157]);assert.deepEqual(heatColor(Infinity),heatColor(-100));for(const value of [-100,-80,-20,0,100])assert(heatColor(value).every(channel=>channel>=0&&channel<=255));assert.notEqual(frequencyColor(100,48000),frequencyColor(10000,48000));assert.equal(frequencyColor(1000,48000),frequencyColor(1000,48000));});
