// M2 扩展场景复用实际 MCP 输入／观测；固定几何预期来自 256×128 网格样本。
import assert from 'node:assert/strict';

/** 经滚轮、左键完整手势验证坐标、八方向调整及每次只提交一个版本。 */
export async function exerciseRegionControls({
  state,
  core,
  wait,
  action,
  click,
  key,
  point,
  selection,
  report,
  document,
  zoom,
}) {
  const current = await core();
  const selected = await selection({
    kind: 'region',
    documentId: document.id,
    documentRevision: document.revision,
    expectedRevision: current.selection.selectionRevision,
    bounds: { x: 17, y: 17, width: 30, height: 22 },
  });
  await selection({
    kind: 'createTask',
    snapshotId: selected.value.snapshotId,
    requestId: 'geometry-fixed',
    name: '几何回归固定任务',
  });
  await click('panel:tasks');
  await click('task:refresh');
  await wait({ kind: 'taskCount', count: 1 });
  await click('tool:region');
  const fixedTasks = (await state()).tasks;
  const assertBounds = (view, expected) => {
    // W3C 输入只能取整到 CSS 像素，两个端点误差最多一个 CSS 像素。
    const tolerance = 1 / view.canvas.zoom + 0.02;
    for (const [field, value] of Object.entries(expected))
      assert(
        Math.abs(view.region[field] - value) <= tolerance,
        `${field}: ${view.region[field]} != ${value}`,
      );
    assert.deepEqual(view.tasks, fixedTasks);
  };
  async function drag(from, to, expected, hold = false) {
    const before = await state();
    const start = point(before, ...from),
      end = point(before, ...to);
    await action({
      kind: 'drag',
      startX: start.x,
      startY: start.y,
      endX: end.x,
      endY: end.y,
      hold,
    });
    if (hold) return before;
    const after = await wait({
      kind: 'selection',
      after: before.selectionRevision,
      hasScope: true,
    });
    assert.equal(BigInt(after.selectionRevision), BigInt(before.selectionRevision) + 1n);
    assertBounds(after, expected);
    return after;
  }
  async function reset() {
    await click('canvas:reframe');
    return drag([32, 16], [224, 112], { x: 32, y: 16, width: 192, height: 96 });
  }
  await click('canvas:snap'); // 独立实例默认启用吸附；本组先单独验证几何变换。
  const geometries = [
    ['nw', [32, 16], [48, 32], { x: 48, y: 32, width: 176, height: 80 }],
    ['n', [128, 16], [128, 32], { x: 32, y: 32, width: 192, height: 80 }],
    ['ne', [224, 16], [208, 32], { x: 32, y: 32, width: 176, height: 80 }],
    ['e', [224, 64], [208, 64], { x: 32, y: 16, width: 176, height: 96 }],
    ['se', [224, 112], [208, 96], { x: 32, y: 16, width: 176, height: 80 }],
    ['s', [128, 112], [128, 96], { x: 32, y: 16, width: 192, height: 80 }],
    ['sw', [32, 112], [48, 96], { x: 48, y: 16, width: 176, height: 80 }],
    ['w', [32, 64], [48, 64], { x: 48, y: 16, width: 176, height: 96 }],
  ];
  {
    report.phase = `m2-geometry-${zoom}`;
    await click('menu:view');
    await click('command:actual');
    for (let step = 0; step < 8; step++) {
      const before = await state();
      if (Math.abs(before.canvas.zoom - zoom) < 0.002) break;
      const deltaY = Math.max(
        -200,
        Math.min(200, Math.round(-Math.log(zoom / before.canvas.zoom) / 0.002)),
      );
      const center = point(before, 128, 64);
      const input = await action({ kind: 'scroll', ...center, deltaY });
      await wait({
        kind: 'canvasPaint',
        documentId: document.id,
        revision: document.revision,
        sceneAtLeast: (BigInt(input.value.before.canvas.sceneRevision) + 1n).toString(),
      });
    }
    const observed = await state();
    assert(Math.abs(observed.canvas.zoom - zoom) < 0.002, '滚轮应达到目标缩放附近');
    report.geometry ??= [];
    report.geometry.push({ requestedZoom: zoom, actualZoom: observed.canvas.zoom });
    for (const [handle, from, to, expected] of geometries) {
      await reset();
      await drag(from, to, expected);
      report.checks.push(`缩放 ${zoom} 的 ${handle} 调整保持对边及固定任务`);
    }
    await reset();
    await drag([128, 64], [144, 64], { x: 48, y: 16, width: 192, height: 96 });
    const before = await drag([144, 64], [128, 64], null, true);
    await wait({ kind: 'draft', active: true });
    assert.equal((await core()).selection.selectionRevision, before.selectionRevision);
    await key('escape');
    await wait({ kind: 'draft', active: false });
    await action({ kind: 'releaseInputs' });
    assert.equal((await core()).selection.selectionRevision, before.selectionRevision);
    assertBounds(await state(), before.region);
    report.checks.push(`缩放 ${zoom} 的移动保持尺寸、Esc 取消不提交`);
  }
  await click('menu:view');
  await click('command:actual');
  await click('canvas:snap');
  const beforePanel = await state();
  await click('panel:layers');
  await key('arrowRight');
  await wait({ kind: 'panel', panel: 'tasks' });
  await key('arrowLeft');
  await wait({ kind: 'panel', panel: 'layers' });
  await click('panel:tasks');
  const afterPanel = await wait({ kind: 'panel', panel: 'tasks' });
  assert.equal(afterPanel.selectionRevision, beforePanel.selectionRevision);
  assert.equal(afterPanel.snapshotId, beforePanel.snapshotId);
  assert.equal((await core()).selection.selectionRevision, beforePanel.selectionRevision);
  assertBounds(afterPanel, beforePanel.region);
  report.checks.push('面板方向键切换不改变选区与固定任务');
}

/** 通过真实弹窗确认当前打开文档；外部 MCP 绑定任务使预检过期于影响变化。 */
export async function exerciseOpenCleanup({
  core,
  state,
  wait,
  waitCore,
  click,
  key,
  selection,
  report,
  document,
  inspectDialog,
  screenshot,
}) {
  report.phase = 'm2-open-cleanup';
  assert.equal((await state()).tasks.length, 0, '清理场景从独立空实例开始');
  const before = await core();
  const selected = await selection({
    kind: 'region',
    documentId: document.id,
    documentRevision: document.revision,
    expectedRevision: before.selection.selectionRevision,
    bounds: { x: 0, y: 0, width: 4, height: 3 },
  });
  const snapshotId = selected.value.snapshotId;
  const ids = [];
  async function create() {
    const task = await selection({
      kind: 'createTask',
      snapshotId,
      requestId: `m2-open-${ids.length}`,
      name: `清理影响任务 ${ids.length}`,
    });
    ids.push(task.value.task.id);
  }
  for (let i = 0; i < 40; i++) await create();
  await click('panel:tasks');
  await click('task:refresh');
  await wait({ kind: 'taskCount', count: 32 });
  await click('task:more');
  const loaded = await wait({ kind: 'taskCount', count: 40 });
  const loadedIds = loaded.tasks.map((task) => task.id).sort();
  assert.deepEqual(loadedIds, [...ids].sort());
  await click('menu:file');
  await click('command:cleanup');
  let view = await wait({ kind: 'cleanup', phase: 'ready' });
  assert.deepEqual([...view.cleanup.taskIds].sort(), [...ids].sort());
  await key('tab');
  assert.deepEqual(await inspectDialog(), { focusInside: true, taskCount: 40, scrollable: true });
  await screenshot('cleanup-confirmation.png');
  await key('escape');
  await wait({ kind: 'cleanup', phase: null });
  assert.equal((await core()).selection.scope.snapshotId, snapshotId);
  await click('menu:file');
  await click('command:cleanup');
  view = await wait({ kind: 'cleanup', phase: 'ready' });
  const oldPlan = view.cleanup.planId;
  await create();
  await click('cleanup:confirm');
  await wait({ kind: 'cleanup', phase: 'stale' });
  assert.equal((await core()).documents[0].id, document.id);
  await click('cleanup:recheck');
  view = await wait({ kind: 'cleanup', phase: 'ready' });
  assert.notEqual(view.cleanup.planId, oldPlan);
  assert.deepEqual([...view.cleanup.taskIds].sort(), [...ids].sort());
  await click('cleanup:confirm');
  await wait({ kind: 'cleanup', phase: 'complete' });
  const final = await waitCore({ kind: 'documentCount', count: 0 });
  const finalView = await wait({ kind: 'applied', atLeast: final.sequence });
  assert.equal(final.selection.scope, null);
  assert.deepEqual([...final.cleanup.invalidatedTaskIds].sort(), [...ids].sort());
  assert.equal(finalView.activeDocument, null);
  assert.equal(finalView.snapshotId, null);
  assert.equal(finalView.region, null);
  assert.deepEqual(finalView.tasks.map((task) => task.id).sort(), loadedIds);
  assert(finalView.tasks.every((task) => task.status === 'invalidated'));
  await click('cleanup:cancel');
  await wait({ kind: 'cleanup', phase: null });
  report.checks.push(
    '打开文档清理、40 项完整影响、Tab/Esc、外部新增任务使预检失效、重新确认及跨页终态同步',
  );
}
