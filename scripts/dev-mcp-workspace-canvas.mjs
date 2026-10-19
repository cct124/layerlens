// 正式 Konva 工作区：所有应用操作通过开发 MCP；预期来自人工样本定义。
import assert from 'node:assert/strict';
import { captureDisplayEnvironment, displayIssues } from './dev-mcp-render-environment.mjs';
import { ensureRenderFocus } from './dev-mcp-render-focus.mjs';
import { recordMemorySample } from './dev-mcp-memory.mjs';
import { memoryCycleGate } from './render-statistics.mjs';

export async function recordWorkspaceEnvironment(context, label, cycle = null, terminal = false) {
  const { report } = context;
  const record = (report.workspaceEnvironment ??= {
    samples: [],
    baseline: null,
    terminal: null,
    valid: true,
  });
  const sample = await captureDisplayEnvironment(context);
  const issues = displayIssues(sample);
  // 文档关闭会主动撤销 Canvas，属于生命周期变化；原生窗口和焦点仍须保持。
  if (report.scenario === 'canvas-memory' && record.baseline) {
    const signature = (s) => [
      s.nativeAfter?.physicalWidth,
      s.nativeAfter?.physicalHeight,
      s.nativeAfter?.scaleFactor,
      s.nativeAfter?.resizeEvents,
      s.nativeAfter?.moveEvents,
      s.nativeAfter?.focusEvents,
      s.nativeAfter?.blurEvents,
      s.ui?.window.devicePixelRatio,
      s.ui?.window.width,
      s.ui?.window.height,
      s.ui?.window.focusEvents,
      s.ui?.window.blurEvents,
    ];
    if (JSON.stringify(signature(sample)) !== JSON.stringify(signature(record.baseline)))
      issues.push('workspace-window-history-changed');
    if (
      sample.ui?.render?.phase === 'ready' &&
      JSON.stringify(sample.ui.window.canvasViewport) !==
        JSON.stringify(record.baseline.ui.window.canvasViewport)
    )
      issues.push('workspace-viewport-changed');
  }
  const entry = { label, cycle, ...sample, issues, valid: !issues.length };
  assert(record.samples.length < 256);
  record.samples.push(entry);
  record.valid &&= entry.valid;
  if (terminal) record.terminal = entry;
  if (!record.baseline && entry.valid) record.baseline = sample;
  assert(entry.valid, JSON.stringify(issues));
  return entry;
}

export async function exerciseWorkspaceCanvas(context) {
  const {
    scenario,
    state,
    core,
    wait,
    waitCore,
    action,
    click,
    key,
    point,
    selection,
    workspace,
    tool,
    args,
    report,
    screenshot,
    document,
    verifyComposite,
    checkpoint,
  } = context;
  report.phase = scenario;
  const paint = (doc = document, sceneAtLeast = '0') =>
    wait({ kind: 'canvasPaint', documentId: doc.id, revision: doc.revision, sceneAtLeast });
  const inspect = (selectedLayerId, autoSelect = false, ctrl = false, doc = document) =>
    wait({
      kind: 'inspection',
      documentId: doc.id,
      revision: doc.revision,
      selectedLayerId,
      autoSelect,
      ctrl,
    });
  await click('menu:view');
  await click('command:actual');
  await paint();
  await ensureRenderFocus(context);
  await recordWorkspaceEnvironment(context, 'start');
  const move = (p) => ({ kind: 'move', ...p });
  const down = (button = 'left') => ({ kind: 'down', button });
  const up = (button = 'left') => ({ kind: 'up', button });
  async function gesture(steps, hold = false) {
    const result = await action({ kind: 'canvasGesture', steps, hold });
    if (!hold)
      await wait({
        kind: 'canvasIdle',
        pointerEventsAtLeast: String(BigInt(result.value.before.input.pointerEvents) + 2n),
      });
  }
  async function pick(x, y) {
    const p = point(await paint(), x, y);
    await gesture([move(p), down(), up()]);
  }
  if (scenario === 'canvas-inspection') {
    const page = (
      await tool(
        'dev_layers',
        args({
          protocolVersion: 1,
          documentId: document.id,
          revision: document.revision,
          query: { kind: 'list', offset: 0, limit: 128 },
        }),
      )
    ).value.result.page;
    assert.equal(page.total, 11);
    const id = (name) => {
      const layer = page.layers.find((l) => l.name === name);
      assert(layer, name);
      return layer.id;
    };
    await inspect(null);
    await pick(140, 230);
    await inspect(null);
    await click('canvas:auto');
    await inspect(null, true);
    await pick(140, 230);
    let view = await inspect(id('overlap-card'), true);
    assert.equal(view.canvas.inspection.candidateCount, 3);
    await click('canvas:next');
    await inspect(id('blue-card'), true);
    await click('canvas:next');
    await inspect(id('background'), true);
    // 透明孔仍以 bounds 命中，精度声明必须保留；不能声称 alpha 命中。
    await pick(85, 220);
    view = await inspect(id('alpha-window'), true);
    assert.equal(view.canvas.inspection.precision, 'bounds');
    await pick(10, 10);
    await inspect(id('background'), true);
    await click('layer:' + id('hidden-group') + ':toggle');
    const invalidSearch = await action({ kind: 'fillLayerSearch', text: 'x'.repeat(257) }, true);
    assert.equal(invalidSearch.error.code, 'INVALID_INPUT');
    await action({ kind: 'fillLayerSearch', text: 'no-matching-layer' });
    await wait({ kind: 'layerFilter', active: true });
    await pick(140, 230);
    await inspect(id('overlap-card'), true);
    await wait({ kind: 'layerFilter', active: false });
    assert.equal(page.layers.find((l) => l.name === 'hidden-child').effectiveVisible, false);
    const fixedBefore = await core();
    const selected = await selection({
      kind: 'region',
      documentId: document.id,
      documentRevision: document.revision,
      expectedRevision: fixedBefore.selection.selectionRevision,
      bounds: { x: 24, y: 170, width: 150, height: 105 },
    });
    await selection({
      kind: 'createTask',
      snapshotId: selected.value.snapshotId,
      requestId: 'workspace-fixed',
      name: '画布检查不改变固定任务',
    });
    await wait({ kind: 'applied', atLeast: (await core()).sequence });
    await wait({ kind: 'selectionIdle' });
    await click('panel:tasks');
    await wait({ kind: 'panel', panel: 'tasks' });
    await click('task:refresh');
    await wait({ kind: 'taskCount', count: 1 });
    await click('tool:region');
    const fixed = await core();
    await key('control', 'down');
    await inspect(id('overlap-card'), true, true);
    await pick(140, 230);
    await inspect(id('overlap-card'), true, true);
    await key('control', 'up');
    await inspect(id('overlap-card'), true);
    const p = point(await paint(), 85, 220);
    await key('control', 'down');
    await gesture([move(p), down(), move({ x: p.x + 20, y: p.y + 15 }), move(p), up()]);
    await key('control', 'up');
    await inspect(id('overlap-card'), true);
    await key('control', 'down');
    await gesture([move(p), down()], true);
    const beforeBlur = await state();
    const dispatch = (await tool('dev_window_action', args({ kind: 'focusAuxiliary' }, true)))
      .value;
    const blurred = (
      await tool(
        'dev_window_wait',
        args({
          condition: { kind: 'focus', main: false, auxiliary: true },
          afterSequence: dispatch.before.eventSequence,
          timeoutMs: 10000,
        }),
      )
    ).value;
    assert(blurred.satisfied);
    await wait({ kind: 'windowFocus', focused: false, eventsAfter: beforeBlur.window.blurEvents });
    await ensureRenderFocus(context);
    await action({ kind: 'releaseInputs' });
    view = await inspect(id('overlap-card'), true);
    assert.equal(view.canvas.pointerCaptured, false);
    assert.deepEqual((await core()).selection, fixed.selection);
    assert.equal(view.tasks[0].snapshotId, selected.value.snapshotId);
    // 空白位于文档外、视口内；Ctrl 不切换工具或清空任务范围。
    const blank = {
      x: Math.round(view.canvas.viewport.x + 8),
      y: Math.round(view.canvas.viewport.y + 8),
    };
    await key('control', 'down');
    await gesture([move(blank), down(), up()]);
    await key('control', 'up');
    await inspect(null, true);
    assert.deepEqual((await core()).selection, fixed.selection);
    await screenshot('workspace-inspection.png');
    report.checks.push(
      '默认关闭／自动选择／重叠循环／隐藏祖先／透明 bounds／Ctrl 区域检查及拖动与失焦取消；核心、树、属性同步且任务不变',
    );
  } else if (scenario === 'canvas-display') {
    for (const zoom of [0.25, 1, 2]) {
      for (let step = 0; step < 8; step++) {
        const view = await paint();
        if (Math.abs(view.canvas.zoom - zoom) < 0.002) break;
        const r = view.canvas.viewport;
        await action({
          kind: 'scroll',
          x: Math.round(r.x + r.width / 2),
          y: Math.round(r.y + r.height / 2),
          deltaY: Math.max(
            -200,
            Math.min(200, Math.round(-Math.log(zoom / view.canvas.zoom) / 0.002)),
          ),
        });
        await paint(document, String(BigInt(view.canvas.sceneRevision) + 1n));
      }
      const view = await paint();
      assert(Math.abs(view.canvas.zoom - zoom) < 0.002);
      await recordWorkspaceEnvironment(context, 'zoom', zoom);
      const name = `workspace-${zoom * 100}.png`;
      await screenshot(name);
      await verifyComposite(name, view);
    }
    const before = await paint();
    for (const size of ['minimum', 'default']) {
      await tool('dev_window_action', args({ kind: 'resize', size }, true));
      const result = (
        await tool(
          'dev_window_wait',
          args({ condition: { kind: 'size', size }, afterSequence: null, timeoutMs: 10000 }),
        )
      ).value;
      assert(result.satisfied);
      await wait({
        kind: 'display',
        width: Math.round(result.lastState.logicalWidth),
        height: Math.round(result.lastState.logicalHeight),
        devicePixelRatio: result.lastState.scaleFactor,
        afterRevision: null,
      });
      const view = await paint();
      assert.equal(view.canvas.zoom, before.canvas.zoom);
      assert.equal(view.canvas.panX, before.canvas.panX);
      await recordWorkspaceEnvironment(context, size);
    }
    await workspace({ kind: 'reload', documentId: document.id });
    const reloaded = await waitCore({ kind: 'idle' });
    const next = reloaded.documents.find((d) => d.id === document.id);
    assert.notEqual(next.revision, document.revision);
    await paint(next);
    await workspace({ kind: 'open', path: 'bitmap-rle.psd' });
    const two = await waitCore({ kind: 'documentCount', count: 2 });
    const other = two.documents.find((d) => d.id !== document.id);
    await paint(other);
    await click('document:' + document.id);
    const restored = await paint(next);
    assert.equal(restored.canvas.zoom, before.canvas.zoom);
    report.checks.push('25%／100%／200% 实际绘制、独立合成色块截图、原生缩放、重载与标签视图恢复');
  } else {
    assert.equal(scenario, 'canvas-memory');
    report.memory = [];
    const ready = [],
      released = [];
    for (let cycle = 0; cycle < 20; cycle++) {
      if (cycle) await workspace({ kind: 'open', path: 'render-simple.psd' });
      const current = (await waitCore({ kind: 'documentCount', count: 1 })).documents[0];
      await paint(current);
      await recordWorkspaceEnvironment(context, 'ready', cycle);
      ready.push(
        recordMemorySample(
          report.memory,
          (await tool('dev_process_metrics', args({}))).value,
          report.processId,
          { label: 'ready', cycle },
        ),
      );
      const result = await workspace({ kind: 'close', documentId: current.id });
      await waitCore({ kind: 'documentCount', count: 0 });
      const empty = await wait({ kind: 'applied', atLeast: result.sequence });
      assert.equal(empty.canvas, null);
      await recordWorkspaceEnvironment(context, 'released', cycle);
      released.push(
        recordMemorySample(
          report.memory,
          (await tool('dev_process_metrics', args({}))).value,
          report.processId,
          { label: 'released', cycle },
        ),
      );
      await checkpoint();
    }
    report.memoryGate = memoryCycleGate(ready, released);
    await checkpoint();
    assert(report.memoryGate.passed, JSON.stringify(report.memoryGate));
    report.checks.push(
      '正式合成图 20 轮打开／关闭，40 次进程树内存及逐轮实际显示环境；不替代历史实验门槛',
    );
  }
}
