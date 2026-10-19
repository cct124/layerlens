// Konva 验收：全部应用操作经测试 MCP；独立进程、真实输入与固定循环，保留失败和原始读数。
import assert from 'node:assert/strict';
import { distribution, memoryCycleGate } from './render-statistics.mjs';
import { exerciseRenderBenchmark } from './dev-mcp-render-benchmark.mjs';
import { ensureRenderFocus } from './dev-mcp-render-focus.mjs';
import { recordMemorySample } from './dev-mcp-memory.mjs';

export async function exerciseRenderAcceptance(context) {
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
    textMode,
    workload,
    checkpoint,
    display,
  } = context;
  const engine = 'konva';
  const a = (report.acceptance = {
    workload,
    sample,
    engine,
    textMode,
    stages: [],
    memory: [],
    gates: [],
    complete: false,
    bitmapSampling: 'nearest; fixed pixel-inspection policy',
  });
  report.phase = 'render-' + workload;
  const renderer = (engine, generation = '0', phase = 'ready') =>
    wait({ kind: 'renderer', engine, phase, generationAtLeast: generation });
  const mutate = async (target, engine) => {
    const before = await state();
    await click(target);
    return renderer(engine, String(BigInt(before.renderLab.generation) + 1n));
  };
  const memory = async (label, engine, cycle) => {
    const metrics = (await tool('dev_process_metrics', args({}))).value;
    return recordMemorySample(a.memory, metrics, report.processId, { label, engine, cycle });
  };
  const completeScene = (view) => {
    assert.equal(view.renderLab.error, null);
    assert.equal(view.renderLab.sourceSha256, report.sourceHashes[0]);
    assert.equal(view.renderLab.unsupportedLayers, 0, JSON.stringify(view.renderLab.limitations));
    assert(view.renderLab.nodeCount > 0);
  };
  if (workload === 'memory') {
    await ensureRenderFocus({ state, tool, args, wait });
    await display.sample('memory-start');
    const ready = [],
      released = [];
    for (let cycle = 0; cycle < 20; cycle++) {
      await display.sample('cycle-start', cycle);
      if (cycle) {
        await workspace({ kind: 'open', path: sample });
        await waitCore({ kind: 'documentCount', count: 1 });
      }
      const doc = (await core()).documents[0];
      await wait({ kind: 'imageReady', documentId: doc.id, revision: doc.revision });
      const view = await renderer(engine);
      completeScene(view);
      await display.sample('ready', cycle);
      assert.equal(view.renderLab.nodeCount, 2000);
      ready.push(await memory('ready', engine, cycle));
      await workspace({ kind: 'close', documentId: doc.id });
      await waitCore({ kind: 'documentCount', count: 0 });
      const empty = await renderer(engine, '0', 'released');
      assert.equal(empty.renderLab.resourceCount, 0);
      assert.equal(empty.renderLab.fontReady, false);
      released.push(await memory('released', engine, cycle));
      await display.sample('released', cycle);
      await checkpoint();
    }
    a.gates.push({ engine, ...memoryCycleGate(ready, released) });
    assert(a.gates[0].passed, JSON.stringify(a.gates[0]));
  } else {
    await tool('dev_window_action', args({ kind: 'resize', size: 'renderLab' }, true));
    const resized = (
      await tool(
        'dev_window_wait',
        args({
          condition: { kind: 'size', size: 'renderLab' },
          afterSequence: null,
          timeoutMs: 10000,
        }),
      )
    ).value;
    assert(resized.satisfied, JSON.stringify(resized));
    let view = await mutate('render:large', engine);
    // 实验页没有正式窗口的自绘最大化按钮；原生尺寸等待后核对新绘制代次的前端尺寸。
    assert.equal(view.window.width, 1280);
    assert.equal(view.window.height, 900);
    assert.equal(view.canvas.viewport.width, 960);
    assert.equal(view.canvas.viewport.height, 600);
    await ensureRenderFocus({ state, tool, args, wait });
    await display.sample('measurement-start');
    if (workload === 'visual') {
      view = await renderer(engine);
      completeScene(view);
      a.stages.push({ engine, render: view.renderLab });
      for (const zoom of [100, 200, 400]) {
        await display.sample('zoom-before', zoom);
        view = await mutate('render:' + zoom, engine);
        completeScene(view);
        await screenshot('render-' + engine + '-' + zoom + '.png');
        await display.sample('zoom-after', zoom);
      }
      await mutate('render:reset', engine);
    } else {
      assert.equal(workload, 'workspace');
      await exerciseRenderBenchmark(context);
      report.phase = 'render-workspace-selection';
      await mutate('render:reset', engine);
      await ensureRenderFocus({ state, tool, args, wait });
      const blur = (await state()).window.blurEvents;
      const clickCanvas = async (x, y) => {
        const p = point(await state(), x, y);
        const result = await action({
          kind: 'canvasGesture',
          steps: [
            { kind: 'move', ...p },
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
        return state();
      };
      view = await clickCanvas(6, 6);
      const selected = view.renderLab.selectedLayerId;
      assert.notEqual(selected, null);
      await click('render:auto');
      await wait({
        kind: 'renderInteraction',
        selectedLayerId: selected,
        autoSelect: false,
        ctrl: false,
      });
      view = await clickCanvas(50, 25);
      assert.equal(view.renderLab.selectedLayerId, selected);
      await key('control', 'down');
      await wait({
        kind: 'renderInteraction',
        selectedLayerId: selected,
        autoSelect: false,
        ctrl: true,
      });
      view = await clickCanvas(50, 25);
      assert.notEqual(view.renderLab.selectedLayerId, selected);
      assert.equal(view.renderLab.selectedLayerId, (await core()).documents[0].selectedLayerId);
      await key('control', 'up');
      view = await wait({
        kind: 'renderInteraction',
        selectedLayerId: view.renderLab.selectedLayerId,
        autoSelect: false,
        ctrl: false,
      });
      assert.equal(view.window.blurEvents, blur, '选择回归期间真实失焦');
      const draws = report.benchmark.stages
        .filter((s) => s.stage === 'warm-draw')
        .map((s) => s.sample.readyMs);
      const inputs = report.benchmark.interactions
        .filter((i) => i.kind === 'pan')
        .flatMap((i) => i.frames.map((f) => f.inputToReadyMs));
      assert(inputs.every((v) => v !== null));
      const draw = distribution(draws),
        input = distribution(inputs);
      const passed = draw !== null && input !== null && draw.p95 <= 50 && input.p95 <= 50;
      a.gates.push({ engine, draw, input, passed });
      assert(passed, JSON.stringify(a.gates[0]));
    }
  }
  a.complete = true;
  await checkpoint();
  report.checks.push('Konva ' + workload + ' 完成，真实场景／固定预算及循环证据已保留');
}
