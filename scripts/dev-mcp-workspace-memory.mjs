// 应用功能经 MCP，固定测量时间仅用于自然回收诊断，不替代原 20 轮门槛。
import assert from 'node:assert/strict';
import { recordMemorySample } from './dev-mcp-memory.mjs';
import { memoryCycleGate } from './render-statistics.mjs';

export async function exerciseWorkspaceMemory(context, paint, environment) {
  const { report, tool, args, workspace, waitCore, wait, checkpoint } = context;
  assert.equal(report.buildProfile, 'release', '资源验收必须使用 release 开发测试程序');
  report.memory = [];
  report.memoryIdle = [];
  report.memoryResources = [];
  report.memoryCycleTimings = [];
  const ready = [],
    released = [];
  const started = performance.now();
  const sample = async (destination, label, cycle = null) =>
    recordMemorySample(
      destination,
      (await tool('dev_process_metrics', args({}))).value,
      report.processId,
      { label, cycle, elapsedMs: performance.now() - started },
    );
  for (let cycle = 0; cycle < 20; cycle++) {
    const cycleStarted = performance.now();
    if (cycle) await workspace({ kind: 'open', path: 'render-simple.psd' });
    const current = (await waitCore({ kind: 'documentCount', count: 1 })).documents[0];
    const painted = await paint(current);
    const readyAt = performance.now();
    report.memoryResources.push({ cycle, label: 'ready', resources: painted.previewResources });
    await environment(context, 'ready', cycle);
    ready.push(await sample(report.memory, 'ready', cycle));
    const closeAt = performance.now();
    const result = await workspace({ kind: 'close', documentId: current.id });
    const emptyCore = await waitCore({ kind: 'documentCount', count: 0 });
    assert.deepEqual(emptyCore.resources, {
      sourceBytes: '0',
      decodedBytes: '0',
      outputBytes: '0',
      cacheBytes: '0',
    });
    const empty = await wait({ kind: 'applied', atLeast: result.sequence });
    assert.equal(empty.canvas, null);
    await environment(context, 'released', cycle);
    // 保留原门槛采样时机，额外的资源释放等待不能把释放态读数悄悄后移。
    released.push(await sample(report.memory, 'released', cycle));
    const sampledAt = performance.now();
    report.memoryResources.push({
      cycle,
      label: 'released',
      resources: empty.previewResources,
      core: emptyCore.resources,
    });
    const confirmed = await wait({ kind: 'previewReleased', atLeast: result.sequence });
    report.memoryResources.push({
      cycle,
      label: 'release-confirmed',
      resources: confirmed.previewResources,
    });
    report.memoryCycleTimings.push({
      cycle,
      elapsedMs: cycleStarted - started,
      openToPaintMs: readyAt - cycleStarted,
      closeToSampleMs: sampledAt - closeAt,
      releaseWaitMs: performance.now() - sampledAt,
      cycleMs: performance.now() - cycleStarted,
      initialOpenMeasured: cycle !== 0,
    });
    await checkpoint();
  }
  report.memoryGate = memoryCycleGate(ready, released);
  await checkpoint();
  const idleStarted = performance.now();
  for (const seconds of [0, 10, 30, 60]) {
    const remaining = seconds * 1000 - (performance.now() - idleStarted);
    if (remaining > 0) await new Promise((resolve) => setTimeout(resolve, remaining));
    await sample(report.memoryIdle, 'idle-' + seconds);
    await environment(context, 'idle', seconds);
    const view = await context.state();
    report.memoryResources.push({
      cycle: null,
      label: 'idle-' + seconds,
      resources: view.previewResources,
    });
    await checkpoint();
  }
  report.memoryComplete = true;
  // 空闲回落只辅助诊断；原循环失败仍使场景失败。
  assert(report.memoryGate.passed, JSON.stringify(report.memoryGate));
  report.checks.push(
    'release 固定 20 轮和 60 秒自然空闲；角色分解、资源所有权、逐轮耗时与原门槛分别记录',
  );
}
