import { test } from 'node:test';
import assert from 'node:assert/strict';
import { distribution, summarizeRenderReports, memoryCycleGate } from './render-statistics.mjs';
test('统计保留缺测与失败，偶数中位数和最近秩 P95 正确', () => {
  assert.equal(distribution([]), null);
  assert.deepEqual(distribution([40, 10, 30, 20]), { count: 4, median: 25, p95: 40, max: 40 });
  assert.throws(() => distribution([1, NaN]));
  assert.throws(() => distribution([-1]));
  assert.deepEqual(summarizeRenderReports([{ passed: false }]), {
    successfulReports: 0,
    failedReports: 1,
    rows: [],
  });
});
test('Konva 汇总按样本和排版聚合独立运行，失败不污染读数且旧引擎不能混入', () => {
  const report = (ms) => ({
    passed: true,
    benchmark: {
      engine: 'konva',
      sample: 'fixture.psd',
      textMode: 'native',
      stages: [{ engine: 'konva', stage: 'warm-draw', sample: { readyMs: ms, submitMs: ms / 2 } }],
      interactions: [],
      memory: [],
    },
  });
  const failed = { ...report(1000), passed: false };
  const result = summarizeRenderReports([report(10), failed, report(30)]);
  assert.equal(result.successfulReports, 2);
  assert.equal(result.failedReports, 1);
  assert.equal(result.rows.length, 1);
  assert.equal(result.rows[0].runs, 2);
  assert.deepEqual(result.rows[0].syncDraw, { count: 2, median: 20, p95: 30, max: 30 });
  assert.equal(result.rows[0].privateReady, null);
  const obsolete = report(10);
  obsolete.benchmark.engine = 'leafer';
  assert.throws(() => summarizeRenderReports([obsolete]), /Konva/);
});
test('固定资源门槛拒绝持续增长、过高峰值和缺失循环，保留稳定缓存平台', () => {
  const mib = 1024 ** 2;
  const stable = Array.from({ length: 20 }, (_, i) => (i < 5 ? 300 + i * 10 : 360 + (i % 2)) * mib);
  assert(
    memoryCycleGate(
      stable.map((v) => v + 100 * mib),
      stable,
    ).passed,
  );
  const growing = Array.from({ length: 20 }, (_, i) => (300 + i * 3) * mib);
  assert.equal(memoryCycleGate(growing, growing).passed, false);
  assert.equal(memoryCycleGate(Array(20).fill(1300 * mib), stable).passed, false);
  assert.throws(() => memoryCycleGate(stable.slice(1), stable));
  assert(memoryCycleGate(Array(20).fill(1300 * mib), stable, { maxPeakBytes: 3072 * mib }).passed);
  assert.equal(
    memoryCycleGate(growing, growing, { maxPeakBytes: 3072 * mib }).passed,
    false,
    '大稿峰值额度不能放宽增长门槛',
  );
  assert.throws(() => memoryCycleGate(stable, stable, { maxPeakBytes: NaN }));
});
