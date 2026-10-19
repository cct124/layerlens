// 真实大稿诊断：全部功能操作通过开发 MCP；完整视觉能力与子集交互成功分开。
import assert from 'node:assert/strict';
import { distribution, memoryCycleGate } from './render-statistics.mjs';
import { ensureRenderFocus } from './dev-mcp-render-focus.mjs';
import { recordMemorySample } from './dev-mcp-memory.mjs';

export async function exerciseRenderProject(context) {
  const {
    state,
    core,
    wait,
    waitCore,
    action,
    click,
    key,
    point,
    tool,
    args,
    workspace,
    report,
    screenshot,
    sample,
    workload,
    checkpoint,
    projectSpec: spec,
    display,
  } = context;
  const engine = 'konva';
  assert.equal(spec.sourceSha256, report.sourceHashes[0]);
  const a = (report.acceptance = {
    workload,
    engine,
    sourceSha256: spec.sourceSha256,
    complete: false,
    fullSceneAccepted: false,
    scope: 'isolatedSourceLayers',
    limitations: [
      '严格场景完整性与源图层诊断分别报告；源像素回退不能作为原生文字或完整 PSD 保真通过。',
      ...spec.limitations,
    ],
    stages: [],
    memory: [],
    gates: [],
    interactions: [],
  });
  report.phase = workload;
  const ready = (generation = '0', phase = 'ready') =>
    wait({ kind: 'renderer', engine, phase, generationAtLeast: generation });
  const mutate = async (target) => {
    const v = await state();
    await click(target);
    return ready(String(BigInt(v.renderLab.generation) + 1n));
  };
  const started = performance.now();
  const sampleMemory = async (label, cycle = null) => {
    const m = (await tool('dev_process_metrics', args({}))).value;
    return recordMemorySample(a.memory, m, report.processId, {
      label,
      cycle,
      elapsedMs: performance.now() - started,
    });
  };
  const fixed = (await core()).selection.selectionRevision;
  let view = await ready();
  assert.equal(view.renderLab.profile, 'large');
  assert.equal(view.renderLab.sourceLayers, spec.layerCount);
  a.stages.push({ stage: 'strict', render: view.renderLab });
  a.strictComplete =
    view.renderLab.unsupportedLayers === 0 && view.renderLab.drawnLayerIds.length > 0;
  await ensureRenderFocus({ state, tool, args, wait });
  if (workload === 'project') {
    await tool('dev_window_action', args({ kind: 'resize', size: 'renderLab' }, true));
    const sized = (
      await tool(
        'dev_window_wait',
        args({
          condition: { kind: 'size', size: 'renderLab' },
          afterSequence: null,
          timeoutMs: 10000,
        }),
      )
    ).value;
    assert(sized.satisfied);
    await mutate('render:large');
    await mutate('render:fit');
    await screenshot('project-' + engine + '-strict.png');
  }
  view = await mutate('render:source-layers');
  const baseline = view.renderLab;
  assert.equal(baseline.profile, 'largeLayers');
  assert.equal(baseline.sourceLayers, spec.layerCount);
  assert.deepEqual(baseline.drawnLayerIds, spec.diagnosticDrawnLayerIds);
  assert.deepEqual(baseline.unsupportedLayerIds, spec.diagnosticUnsupportedLayerIds);
  assert.equal(baseline.degradedLayerIds.length, baseline.drawnLayerIds.length);
  assert(baseline.encodedBytes > 0 && baseline.alphaBytes > 0);
  a.stages.push({ stage: 'source-layers', render: baseline });
  await display.sample('project-start');
  const sameCoverage = (v) => {
    for (const k of [
      'drawnLayerIds',
      'rasterLayerIds',
      'textRasterLayerIds',
      'unsupportedLayerIds',
      'degradedLayerIds',
      'fontSha256',
    ])
      assert.deepEqual(v.renderLab[k], baseline[k], k);
    assert.equal(v.renderLab.error, null);
    assert.equal(v.renderLab.sourceSha256, spec.sourceSha256);
  };
  const close = async () => {
    const document = (await core()).documents[0];
    await workspace({ kind: 'close', documentId: document.id });
    await waitCore({ kind: 'documentCount', count: 0 });
    const empty = await ready('0', 'released');
    assert.equal(empty.renderLab.resourceCount, 0);
    assert.equal(empty.renderLab.encodedBytes, 0);
    assert.equal(empty.renderLab.alphaBytes, 0);
    assert.equal(empty.renderLab.fontReady, false);
  };
  const reopen = async () => {
    await workspace({ kind: 'open', path: sample });
    const document = (await waitCore({ kind: 'documentCount', count: 1 })).documents[0];
    await wait({ kind: 'imageReady', documentId: document.id, revision: document.revision });
    const result = await ready();
    sameCoverage(result);
    return result;
  };
  if (workload === 'project-memory') {
    const opened = [],
      released = [];
    let pending = Promise.resolve(),
      sampling = false,
      samplingError = null,
      cycle = 0;
    const timer = setInterval(() => {
      if (sampling || samplingError) return;
      sampling = true;
      pending = sampleMemory('interval', cycle)
        .catch((error) => {
          samplingError = error;
        })
        .finally(() => {
          sampling = false;
        });
    }, 1000);
    try {
      for (cycle = 0; cycle < 20; cycle++) {
        await display.sample('cycle-start', cycle);
        if (cycle) await reopen();
        if (samplingError) throw samplingError;
        opened.push(await sampleMemory('ready', cycle));
        await display.sample('ready', cycle);
        await close();
        released.push(await sampleMemory('released', cycle));
        await display.sample('released', cycle);
        await checkpoint();
      }
    } finally {
      clearInterval(timer);
      await pending;
    }
    if (samplingError) throw samplingError;
    const gate = memoryCycleGate(opened, released, { maxPeakBytes: 3 * 1024 ** 3 });
    gate.peakPrivateBytes = Math.max(...a.memory.map((m) => m.privateBytes));
    gate.passed &&= gate.peakPrivateBytes <= 3 * 1024 ** 3;
    a.gates.push({
      ...gate,
      maxPeakBytes: 3 * 1024 ** 3,
      maxReleasedDeltaBytes: 64 * 1024 ** 2,
      maxSlopeBytesPerCycle: 2 * 1024 ** 2,
      note: '显式大稿实验采样门槛；不是进程硬限或长期无泄漏证明。',
    });
    // 固定测量窗口观察自然回收，非界面同步等待；不执行强制 GC。
    let previous = 0;
    for (const seconds of [0, 10, 30, 60]) {
      if (seconds > previous)
        await new Promise((resolve) => setTimeout(resolve, (seconds - previous) * 1000));
      await sampleMemory('idle-' + seconds);
      await display.sample('idle', seconds);
      previous = seconds;
      await checkpoint();
    }
    a.complete = true;
    assert(gate.passed, JSON.stringify(gate));
  } else {
    sameCoverage(await mutate('render:fit'));
    await screenshot('project-' + engine + '-fit.png');
    await sampleMemory('fit');
    for (const [index, detail] of spec.details.entries()) {
      await click('layer:' + detail.layerId + ':select');
      await wait({ kind: 'selectionIdle' });
      for (const zoom of [100, 200, 400]) {
        await display.sample('detail-before', index * 3 + [100, 200, 400].indexOf(zoom));
        await mutate('render:' + zoom);
        view = await mutate('render:focus');
        sameCoverage(view);
        await screenshot('project-' + engine + '-detail' + index + '-' + zoom + '.png');
        await display.sample('detail-after', index * 3 + [100, 200, 400].indexOf(zoom));
        a.stages.push({
          stage: 'detail',
          index,
          zoom,
          render: view.renderLab,
          canvas: view.canvas,
        });
      }
    }
    await click('render:clear-metrics');
    const draw = [];
    for (let repeat = 0; repeat < 3; repeat++)
      for (const zoom of [100, 200, 400]) {
        await display.sample('draw-before', repeat * 3 + [100, 200, 400].indexOf(zoom));
        view = await mutate('render:' + zoom);
        sameCoverage(view);
        draw.push(view.renderLab.samples.at(-1).readyMs);
        await display.sample('draw-after', repeat * 3 + [100, 200, 400].indexOf(zoom));
      }
    await mutate('render:fit');
    await ensureRenderFocus({ state, tool, args, wait });
    const before = await state(),
      p = point(before, spec.width / 2, spec.height / 2);
    const steps = [
      { kind: 'move', ...p },
      { kind: 'down', button: 'middle' },
    ];
    for (let i = 1; i <= 24; i++)
      steps.push({ kind: 'moveTimed', x: p.x + i * 2, y: p.y - i, durationMs: 16 });
    steps.push({ kind: 'up', button: 'middle' });
    const gesture = await action({ kind: 'canvasGesture', steps, hold: false });
    await wait({
      kind: 'canvasIdle',
      pointerEventsAtLeast: String(BigInt(gesture.value.before.input.pointerEvents) + 2n),
    });
    view = await ready(String(BigInt(before.renderLab.generation) + 1n));
    assert.equal(view.window.blurEvents, before.window.blurEvents, '真实失焦使本次手势无效');
    assert(Math.abs(view.canvas.panX - before.canvas.panX - 48) < 0.1);
    assert(Math.abs(view.canvas.panY - before.canvas.panY + 24) < 0.1);
    const frames = view.renderLab.samples.filter((s) => s.operation === 'pan');
    assert(frames.length >= 2);
    a.interactions.push({ kind: 'pan', frames });
    await mutate('render:fit');
    const hit = async (item) => {
      const v = await state(),
        location = point(v, item.x, item.y);
      const result = await action({
        kind: 'canvasGesture',
        steps: [
          { kind: 'move', ...location },
          { kind: 'down', button: 'left' },
          { kind: 'up', button: 'left' },
        ],
        hold: false,
      });
      await wait({
        kind: 'canvasIdle',
        pointerEventsAtLeast: String(BigInt(result.value.before.input.pointerEvents) + 2n),
      });
      await wait({ kind: 'selectionIdle' });
      const actual = await state(),
        selected = (await core()).documents[0].selectedLayerId;
      assert.equal(actual.renderLab.selectedLayerId, selected);
      if (actual.renderLab.autoSelect || actual.renderLab.ctrl)
        assert(
          actual.renderLab.candidateIds.includes(item.sourceLayerId),
          '独立参考确认的不透明点必须包含对应源图层',
        );
      a.interactions.push({
        kind: 'select',
        item,
        selected,
        candidates: actual.renderLab.candidateIds,
        hitMs: actual.renderLab.hitMs,
        selectionMs: actual.renderLab.selectionMs,
      });
      return actual;
    };
    const first = await hit(spec.hitPoints[0]);
    assert.notEqual(first.renderLab.selectedLayerId, null);
    assert.equal(first.renderLab.selectedLayerId, first.renderLab.candidateIds[0]);
    await click('render:auto');
    const disabled = await hit(spec.hitPoints[1]);
    assert.equal(disabled.renderLab.selectedLayerId, first.renderLab.selectedLayerId);
    await key('control', 'down');
    let temporary;
    try {
      temporary = await hit(spec.hitPoints[1]);
      assert(temporary.renderLab.ctrl);
      assert.notEqual(temporary.renderLab.selectedLayerId, first.renderLab.selectedLayerId);
      assert.equal(temporary.renderLab.selectedLayerId, temporary.renderLab.candidateIds[0]);
    } finally {
      await key('control', 'up');
    }
    assert.equal(
      (await hit(spec.hitPoints[0])).renderLab.selectedLayerId,
      temporary.renderLab.selectedLayerId,
    );
    assert.equal((await core()).selection.selectionRevision, fixed);
    const pan = distribution(frames.map((f) => f.inputToReadyMs).filter((n) => n !== null));
    a.gates.push({
      draw: distribution(draw),
      pan,
      passed: distribution(draw).p95 <= 50 && pan?.p95 <= 50,
    });
    await sampleMemory('after-interaction');
    await close();
    await sampleMemory('closed');
    await reopen();
    await screenshot('final-state.png');
    a.complete = true;
    assert(
      a.gates.every((g) => g.passed),
      JSON.stringify(a.gates),
    );
  }
  report.checks.push('大稿 MCP 诊断流程完成；整页保真未通过，源图层投影与文字像素回退均显式报告');
}
