import test from 'node:test';
import assert from 'node:assert/strict';
import {fullWaveMatches} from '../src/full-wave-client.ts';
test('完整波形结果同时绑定素材与任务，拒绝旧任务串到新选中素材',()=>{const status={job_id:'job-a',asset_id:'asset-a'};assert.equal(fullWaveMatches(status,'asset-a','job-a'),true);assert.equal(fullWaveMatches(status,'asset-b','job-a'),false);assert.equal(fullWaveMatches(status,'asset-a','job-b'),false);});
