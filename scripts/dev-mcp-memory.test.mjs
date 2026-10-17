import assert from 'node:assert/strict';
import test from 'node:test';
import { recordMemorySample } from './dev-mcp-memory.mjs';

const sample = () => ({
  rootProcessId: 10,
  errors: [],
  observations: [],
  processes: [
    { processId: 10, role: 'application', privateBytes: 100, workingSetBytes: 200 },
    { processId: 12, role: 'webview', privateBytes: 300, workingSetBytes: 400 },
  ],
});
test('确认退出或排除候选单列，不混入已验证进程总和', () => {
  const metrics = sample();
  metrics.observations.push({ processId: 11, reason: 'exitedBeforeMeasurement' });
  const retained = [];
  assert.equal(recordMemorySample(retained, metrics, 10, { label: 'ready', cycle: 0 }), 400);
  assert.equal(retained[0].valid, true);
  assert.deepEqual(retained[0].observations, metrics.observations);
});
test('身份错误的原始采样保留，但不生成有效总和', () => {
  const metrics = sample();
  metrics.errors.push('12: 创建时间不可验证');
  const retained = [];
  assert.throws(() => recordMemorySample(retained, metrics, 10, { label: 'interval' }));
  assert.equal(retained[0].valid, false);
  assert.equal(retained[0].privateBytes, null);
  assert.deepEqual(retained[0].processes, metrics.processes);
  assert.deepEqual(retained[0].errors, metrics.errors);
});
test('错误根进程、缺少 WebView 和不可用内存均拒绝', () => {
  for (const change of [
    (m) => {
      m.rootProcessId = 20;
    },
    (m) => {
      m.processes.pop();
    },
    (m) => {
      m.processes[1].privateBytes = NaN;
    },
    (m) => {
      m.processes[1].workingSetBytes = 0;
    },
  ]) {
    const metrics = sample();
    change(metrics);
    const retained = [];
    assert.throws(() => recordMemorySample(retained, metrics, 10, {}));
    assert.equal(retained[0].privateBytes, null);
  }
});
