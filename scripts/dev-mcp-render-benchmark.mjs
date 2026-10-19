// 所有输入、等待、截图及 OS 采样均通过开发 MCP；本模块不执行页面脚本。
import assert from 'node:assert/strict';
import { ensureRenderFocus } from './dev-mcp-render-focus.mjs';
import { recordMemorySample } from './dev-mcp-memory.mjs';
export async function exerciseRenderBenchmark({
  state,
  core,
  wait,
  waitCore,
  action,
  click,
  point,
  tool,
  args,
  workspace,
  report,
  screenshot,
  document,
  sample,
  textMode,
  display,
}) {
  const engine = 'konva';
  report.phase = 'render-benchmark';
  report.benchmark = {
    sample,
    textMode,
    engine,
    stages: [],
    memory: [],
    interactions: [],
    limits: [
      '新进程首次使用与同进程热重建分别报告；OS 文件缓存未清空。',
      '同步绘制采用同一 Canvas 像素读回屏障；平移不读回或编码 PNG，ready 不代表物理显示。',
      '文字单位沿用实验解释；生成样本不作为 Photoshop 真值。原生模式的共享富文本回退单独计数。',
      '帧间隔来自应用实际绘制启动间隔，输入请求 16ms W3C 移步，实际调度由驱动与浏览器决定；不是硬件 FPS。',
    ],
  };
  const b = report.benchmark;
  const renderer = (engine, generation = '0', phase = 'ready') =>
    wait({ kind: 'renderer', engine, phase, generationAtLeast: generation });
  const focus = () => ensureRenderFocus({ state, tool, args, wait });
  const mutate = async (target, engine) => {
    const before = await state();
    await click(target);
    return renderer(engine, String(BigInt(before.renderLab.generation) + 1n));
  };
  const memory = async (label, engine) => {
    const data = (await tool('dev_process_metrics', args({}))).value;
    recordMemorySample(b.memory, data, report.processId, { label, engine });
  };
  const fixed = (await core()).selection.selectionRevision;
  let v = await renderer(engine);
  await focus();
  await display.sample('benchmark-start');
  assert.equal(v.renderLab.textMode, textMode);
  assert.equal(v.renderLab.sourceSha256, report.sourceHashes[0]);
  assert.equal(v.renderLab.error, null);
  b.fontSha256 = v.renderLab.fontSha256;
  if (sample === 'typography.psd') {
    assert.equal(v.renderLab.nativeTextLayers, textMode === 'native' ? 3 : 0);
    assert.equal(v.renderLab.sharedTextLayers, textMode === 'native' ? 1 : 4);
  }
  b.stages.push({
    engine,
    stage: 'process-first-use',
    render: v.renderLab,
  });
  await memory('initial', engine);
  if (!sample.startsWith('public-'))
    assert.equal(v.renderLab.unsupportedLayers, 0, JSON.stringify(v.renderLab.limitations));
  // 固定三轮重建，保留所有读数；不自动重试挑选最好结果。
  for (let repeat = 0; repeat < 3; repeat++) {
    await display.sample('rebuild-before', repeat);
    v = await mutate('render:rebuild', engine);
    b.stages.push({ engine, stage: 'warm-rebuild', repeat, render: v.renderLab });
    await memory('warm-rebuild-' + repeat, engine);
    await display.sample('rebuild-after', repeat);
  }
  await click('render:clear-metrics');
  for (let repeat = 0; repeat < 3; repeat++)
    for (const zoom of [100, 200, 400]) {
      await display.sample('draw-before', repeat * 3 + [100, 200, 400].indexOf(zoom));
      v = await mutate('render:' + zoom, engine);
      assert.equal(v.canvas.zoom, zoom / 100);
      b.stages.push({
        engine,
        stage: 'warm-draw',
        repeat,
        zoom,
        sample: v.renderLab.samples.at(-1),
      });
      if (repeat === 0) await screenshot('render-' + engine + '-' + zoom + '.png');
      await display.sample('draw-after', repeat * 3 + [100, 200, 400].indexOf(zoom));
    }
  await mutate('render:reset', engine);
  await click('render:clear-metrics');
  const before = await state(),
    p = point(before, 180, 170);
  await focus();
  const focusBefore = (await state()).window.blurEvents;
  const steps = [
    { kind: 'move', ...p },
    { kind: 'down', button: 'middle' },
  ];
  for (let i = 1; i <= 24; i++)
    steps.push({ kind: 'moveTimed', x: p.x + Math.round(i * 1.5), y: p.y - i, durationMs: 16 });
  steps.push({ kind: 'up', button: 'middle' });
  const gesture = await action({ kind: 'canvasGesture', steps, hold: false });
  await wait({
    kind: 'canvasIdle',
    pointerEventsAtLeast: String(BigInt(gesture.value.before.input.pointerEvents) + 2n),
  });
  v = await renderer(engine, String(BigInt(before.renderLab.generation) + 1n));
  assert.equal(
    v.window.blurEvents,
    focusBefore,
    '手势期间真实失焦，不能把取消后的位移记为性能结果',
  );
  assert(Math.abs(v.canvas.panX - before.canvas.panX - 36) < 0.1);
  assert(Math.abs(v.canvas.panY - before.canvas.panY + 24) < 0.1);
  const frames = v.renderLab.samples.filter((s) => s.operation === 'pan');
  assert(frames.length >= 2 && frames.every((s) => !s.synchronized && s.readbackMs === 0));
  b.interactions.push({ engine, kind: 'pan', frames, pointerEvents: v.input.pointerEvents });
  await mutate('render:reset', engine);
  const target = point(
    await state(),
    sample.startsWith('text-') ? 5 : 6,
    sample.startsWith('text-') ? 10 : 6,
  );
  const clicked = await action({
    kind: 'canvasGesture',
    steps: [
      { kind: 'move', ...target },
      { kind: 'down', button: 'left' },
      { kind: 'up', button: 'left' },
    ],
    hold: false,
  });
  await wait({
    kind: 'canvasIdle',
    pointerEventsAtLeast: String(BigInt(clicked.value.before.input.pointerEvents) + 2n),
  });
  await wait({ kind: 'selectionIdle' });
  v = await state();
  const actual = (await core()).documents.find((d) => d.id === document.id);
  assert.equal(v.renderLab.selectedLayerId, actual.selectedLayerId);
  if (!sample.startsWith('public-')) {
    assert.notEqual(actual.selectedLayerId, null);
    assert.equal(actual.selectedLayerId, v.renderLab.candidateIds[0]);
  }
  b.interactions.push({
    engine,
    kind: 'selection',
    selectedLayerId: actual.selectedLayerId,
    selectionMs: v.renderLab.selectionMs,
    hitMs: v.renderLab.hitMs,
    candidates: v.renderLab.candidateIds,
  });
  assert.equal((await core()).selection.selectionRevision, fixed);
  await memory('after-interaction', engine);
  await click('render:release');
  const released = await renderer(engine, '0', 'released');
  assert.equal(released.renderLab.resourceCount, 0);
  assert.equal(released.renderLab.fontReady, false);
  await memory('released', engine);
  await mutate('render:rebuild', engine);
  await workspace({ kind: 'close', documentId: document.id });
  await waitCore({ kind: 'documentCount', count: 0 });
  await renderer(engine, '0', 'released');
  await memory('document-closed', engine);
  await workspace({ kind: 'open', path: sample });
  const reopened = await waitCore({ kind: 'documentCount', count: 1 });
  assert.notEqual(reopened.documents[0].id, document.id);
  await wait({
    kind: 'imageReady',
    documentId: reopened.documents[0].id,
    revision: reopened.documents[0].revision,
  });
  await renderer(engine);
  await memory('reopened', engine);
  await screenshot('final-state.png');
  report.checks.push(
    'Konva 冷／热标签、固定重建和倍率循环、真实平移与选层同步、MCP OS 内存、释放／关闭／重开',
  );
}
