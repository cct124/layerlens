// 保留采样数；缺测与失败不会转成零或悄悄并入性能优胜结论。
import { displaySignature } from './dev-mcp-render-environment.mjs';
export function distribution(values) {
  if (!values.length) return null;
  if (values.some((v) => !Number.isFinite(v) || v < 0)) throw new Error('无效性能读数');
  const sorted = [...values].sort((a, b) => a - b),
    n = sorted.length;
  return {
    count: n,
    median: n % 2 ? sorted[(n - 1) / 2] : (sorted[n / 2 - 1] + sorted[n / 2]) / 2,
    p95: sorted[Math.ceil(n * 0.95) - 1],
    max: sorted[n - 1],
  };
}
/** 预先固定的短循环验收；预热五轮，比较第 6–10 与 16–20 轮，并保留线性趋势。 */
export function memoryCycleGate(ready, released, { maxPeakBytes = 1280 * 1024 ** 2 } = {}) {
  if (!Number.isFinite(maxPeakBytes) || maxPeakBytes <= 0) throw new Error('无效峰值门槛');
  if (ready.length !== 20 || released.length !== 20) throw new Error('内存循环必须完整采样 20 轮');
  const first = distribution(released.slice(5, 10)).median;
  const last = distribution(released.slice(15, 20)).median;
  const values = released.slice(5),
    center = (values.length - 1) / 2;
  const mean = values.reduce((a, b) => a + b, 0) / values.length;
  const slope =
    values.reduce((sum, v, i) => sum + (i - center) * (v - mean), 0) /
    values.reduce((sum, _, i) => sum + (i - center) ** 2, 0);
  const peak = distribution([...ready, ...released]).max;
  return {
    passed: peak <= maxPeakBytes && last - first <= 64 * 1024 ** 2 && slope <= 2 * 1024 ** 2,
    peakPrivateBytes: peak,
    releasedDeltaBytes: last - first,
    releasedSlopeBytesPerCycle: slope,
    firstWindowMedian: first,
    lastWindowMedian: last,
  };
}
export function summarizeRenderReports(reports) {
  const groups = new Map();
  const accepted = (report) =>
    report.passed &&
    (!report.displayEnvironment ||
      (report.displayEnvironment.valid &&
        report.displayEnvironment.baseline &&
        report.displayEnvironment.terminal?.valid));
  for (const report of reports) {
    if (!accepted(report) || !report.benchmark) continue;
    const b = report.benchmark;
    const engine = b.engine;
    if (engine !== 'konva') throw new Error('只汇总当前 Konva 回归报告');
    const displayConditions = report.displayEnvironment?.baseline
      ? displaySignature(report.displayEnvironment.baseline)
      : null;
    const key =
      b.sample + '/' + b.textMode + '/' + engine + '/' + JSON.stringify(displayConditions);
    let g = groups.get(key);
    if (!g) {
      g = {
        sample: b.sample,
        textMode: b.textMode,
        engine,
        displayConditions,
        runs: 0,
        cold: [],
        warmInit: [],
        syncDraw: [],
        submit: [],
        panInterval: [],
        hit: [],
        selection: [],
        privateReady: [],
        privateReleased: [],
        unsupported: [],
        nativeText: [],
        sharedText: [],
      };
      groups.set(key, g);
    }
    g.runs++;
    for (const s of b.stages.filter((s) => s.engine === engine)) {
      if (s.stage === 'process-first-use') g.cold.push(s.render.initMs);
      if (s.stage === 'warm-rebuild') g.warmInit.push(s.render.initMs);
      if (s.stage === 'warm-draw') {
        g.syncDraw.push(s.sample.readyMs);
        g.submit.push(s.sample.submitMs);
      }
      if (s.render) {
        g.unsupported.push(s.render.unsupportedLayers);
        g.nativeText.push(s.render.nativeTextLayers);
        g.sharedText.push(s.render.sharedTextLayers);
      }
    }
    for (const i of b.interactions.filter((i) => i.engine === engine)) {
      if (i.kind === 'pan')
        g.panInterval.push(...i.frames.map((f) => f.frameIntervalMs).filter((v) => v !== null));
      else {
        if (i.hitMs !== null) g.hit.push(i.hitMs);
        if (i.selectionMs !== null) g.selection.push(i.selectionMs);
      }
    }
    for (const m of b.memory.filter(
      (m) => m.engine === engine && ['after-interaction', 'released'].includes(m.label),
    )) {
      const sum = m.processes.reduce((n, p) => n + p.privateBytes, 0);
      (m.label === 'released' ? g.privateReleased : g.privateReady).push(sum);
    }
  }
  return {
    successfulReports: reports.filter(accepted).length,
    failedReports: reports.filter((r) => !accepted(r)).length,
    rows: [...groups.values()].map((g) =>
      Object.fromEntries(
        Object.entries(g).map(([k, v]) => [k, Array.isArray(v) ? distribution(v) : v]),
      ),
    ),
  };
}
