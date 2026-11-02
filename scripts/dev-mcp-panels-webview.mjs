// 面板功能只通过开发 MCP 的真实输入、同步等待与只读观测执行。
import assert from 'node:assert/strict';
import { placeTestWindow } from './dev-mcp-preflight.mjs';
import { focusTestWindow } from './dev-mcp-render-focus.mjs';
import { exercisePanelBoundaries } from './dev-mcp-panel-boundaries.mjs';

export async function exercisePanels(context) {
  const {
    scenario,
    state,
    core,
    wait,
    waitCore,
    action,
    click,
    key,
    selection,
    workspace,
    tool,
    args,
    report,
    screenshot,
    document,
    previousPanelReport,
  } = context;
  report.phase = scenario;
  const native = async (request) => (await tool('dev_window_action', args(request, true))).value;
  const nativeWait = async (condition) => {
    const r = (
      await tool('dev_window_wait', args({ condition, afterSequence: null, timeoutMs: 10000 }))
    ).value;
    assert(r.satisfied, JSON.stringify(r));
    return r.lastState;
  };
  const layout = (extra = {}) =>
    wait({
      kind: 'panelLayout',
      sidebarWidth: null,
      inspectorPercent: null,
      dragging: false,
      treeCount: null,
      afterRevision: null,
      ...extra,
    });
  const near = (actual, expected, label, tolerance = 2) =>
    assert(Math.abs(actual - expected) <= tolerance, label + ': ' + actual + ' != ' + expected);
  const reset = async () => {
    await click('menu:window');
    await wait({ kind: 'menu', menu: 'window' });
    await click('command:resetLayout');
    await wait({ kind: 'menu', menu: null });
    return layout({ sidebarWidth: 320, inspectorPercent: 36 });
  };
  await layout({ treeCount: scenario === 'panels-list' ? 4096 : 512 });
  if (scenario === 'panels-restore') {
    assert(
      previousPanelReport?.passed && previousPanelReport.panelRestart,
      '须提供已正常退出的保存场景证据',
    );
    assert.notEqual(report.instanceId, previousPanelReport.instanceId);
    const restored = await layout();
    const saved = previousPanelReport.panelRestart;
    report.panelRestart = {
      instanceId: report.instanceId,
      sessionId: restored.sessionId,
      panels: restored.panels,
    };
    assert.notEqual(restored.sessionId, saved.sessionId);
    near(restored.panels.sidebarWidth, saved.panels.sidebarWidth, '重启宽度');
    near(restored.panels.inspectorPercent, saved.panels.inspectorPercent, '重启比例', 0.5);
    assert.equal(restored.panels.savedSidebarWidth, saved.panels.savedSidebarWidth);
    assert.equal(restored.panels.savedInspectorPercent, saved.panels.savedInspectorPercent);
    const tasks = await selection({ kind: 'tasks', after: null, limit: 1 }, false);
    assert.equal(tasks.value.page.total, 0);
    assert.deepEqual(tasks.value.page.tasks, []);
    await screenshot('panels-restored.png');
    await reset();
    report.checks.push(
      '新进程／新会话从同一隔离 profile 恢复实际面板偏好，任务不跨进程存活，菜单重置正常',
    );
    return;
  }
  const initialCore = await core();
  const selected = await selection({
    kind: 'region',
    documentId: document.id,
    documentRevision: document.revision,
    expectedRevision: initialCore.selection.selectionRevision,
    bounds: { x: 8, y: 8, width: 24, height: 24 },
  });
  await selection({
    kind: 'createTask',
    snapshotId: selected.value.snapshotId,
    requestId: 'panel-fixed',
    name: '面板回归固定任务',
  });
  await wait({ kind: 'applied', atLeast: (await core()).sequence });
  await click('panel:tasks');
  await click('task:refresh');
  await wait({ kind: 'taskCount', count: 1 });
  await action({ kind: 'fillTaskName', text: '保留面板任务草稿' });
  const fixed = await state();
  const unchanged = async (expectedRevision = fixed.selectionRevision) => {
    const now = await state();
    const authoritative = await core();
    assert.equal(now.snapshotId, fixed.snapshotId);
    assert.equal(now.selectionRevision, expectedRevision);
    assert.equal(authoritative.selection.scope.snapshotId, fixed.snapshotId);
    assert.equal(authoritative.selection.selectionRevision, expectedRevision);
    assert.deepEqual(now.region, fixed.region);
    assert.deepEqual(authoritative.selection.region, fixed.region);
    assert.deepEqual(now.tasks, fixed.tasks);
    assert.equal(now.panels.taskName, '保留面板任务草稿');
  };
  async function drag(axis, delta, hold = false) {
    const before = await state();
    await action({ kind: 'panelDrag', axis, delta, hold });
    return layout({ dragging: hold, afterRevision: before.viewRevision });
  }
  report.panelSamples = [];
  if (
    ['panels-boundaries', 'panels-maximized', 'panels-cancel', 'panels-keyboard-cancel'].includes(
      scenario,
    )
  ) {
    return exercisePanelBoundaries(context, { layout, reset, unchanged, drag });
  }
  if (scenario === 'panels-save') {
    await reset();
    await drag('width', -100);
    const saved = await drag('height', 70);
    assert(saved.panels.savedSidebarWidth > 400);
    assert(saved.panels.savedInspectorPercent > 36);
    await unchanged();
    report.panelRestart = {
      instanceId: report.instanceId,
      sessionId: saved.sessionId,
      panels: saved.panels,
    };
    await screenshot('panels-saved.png');
    report.checks.push('真实拖动产生持久化布局，正常退出由运行器确认，未写入测试伪造偏好');
    return;
  }
  if (scenario === 'panels-list') {
    await click('panel:layers');
    await wait({ kind: 'panel', panel: 'layers' });
    let view = await layout({ treeCount: 4096 });
    assert.equal(view.panels.tree.firstLayerId, 0);
    assert(view.panels.tree.renderedRows <= Math.ceil(view.panels.tree.clientHeight / 28) + 6);
    await action({ kind: 'panelScroll', target: 'layers', edge: 'end' });
    view = await wait({
      kind: 'panelScroll',
      target: 'layers',
      edge: 'end',
      afterRevision: view.viewRevision,
    });
    assert.equal(view.panels.tree.lastLayerId, 4095);
    assert(view.panels.tree.scrollTop > 100000);
    assert(view.panels.tree.renderedRows <= Math.ceil(view.panels.tree.clientHeight / 28) + 6);
    const end = view.panels.tree.scrollTop;
    await screenshot('panels-list-end.png');
    await click('layer:4095:select');
    await wait({
      kind: 'inspection',
      documentId: document.id,
      revision: document.revision,
      selectedLayerId: 4095,
      autoSelect: false,
      ctrl: false,
    });
    await click('panel:tasks');
    await wait({ kind: 'panel', panel: 'tasks' });
    await unchanged();
    await click('panel:layers');
    await wait({ kind: 'panel', panel: 'layers' });
    view = await layout({ treeCount: 4096 });
    near(view.panels.tree.scrollTop, end, '切换面板恢复滚动');
    await action({ kind: 'panelScroll', target: 'layers', edge: 'start' });
    await wait({ kind: 'panelScroll', target: 'layers', edge: 'start', afterRevision: null });
    await action({ kind: 'fillLayerSearch', text: 'row-0000' });
    await wait({ kind: 'layerFilter', active: true });
    await click('layer:4095:select');
    await unchanged();
    await workspace({ kind: 'open', path: 'viewer-text.psd' });
    const loaded = await waitCore({ kind: 'documentCount', count: 2 });
    const textDocument = loaded.documents.find((d) => d.id !== document.id);
    assert(textDocument);
    await wait({
      kind: 'imageReady',
      documentId: textDocument.id,
      revision: textDocument.revision,
    });
    await layout({ treeCount: 3 });
    await click('layer:1:select');
    await wait({
      kind: 'inspection',
      documentId: textDocument.id,
      revision: textDocument.revision,
      selectedLayerId: 1,
      autoSelect: false,
      ctrl: false,
    });
    view = await state();
    assert(view.panels.properties.textLength > 1000);
    await action({ kind: 'panelScroll', target: 'properties', edge: 'end' });
    view = await wait({
      kind: 'panelScroll',
      target: 'properties',
      edge: 'end',
      afterRevision: view.viewRevision,
    });
    assert(view.panels.properties.scrollTop > 0);
    await screenshot('panels-properties-end.png');
    await click('document:' + document.id);
    await wait({ kind: 'imageReady', documentId: document.id, revision: document.revision });
    view = await layout({ treeCount: 4096 });
    assert.equal(view.panels.search, 'row-0000');
    // 活动标签切换两次，全局选择版本递增两次，原文档快照与固定任务仍保持。
    await unchanged((BigInt(fixed.selectionRevision) + 2n).toString());
    report.panelSamples.push({ operation: 'long-list-and-properties', view });
    report.checks.push(
      '4096 层真实分页与有界虚拟行、首尾滚动、检查选择、隐藏及跨标签恢复、长文字属性末端可达',
    );
    return;
  }
  for (const size of ['default', 'minimum']) {
    report.phase = 'panels-drag-' + size;
    await native({ kind: 'restore' });
    await native({ kind: 'resize', size });
    await nativeWait({ kind: 'size', size });
    await placeTestWindow(context);
    await layout();
    await reset();
    await click('menu:view');
    await wait({ kind: 'menu', menu: 'view' });
    await click('command:actual');
    await wait({ kind: 'menu', menu: null });
    const initial = await layout();
    const zoom = initial.canvas.zoom,
      panX = initial.canvas.panX,
      panY = initial.canvas.panY;
    const changed = await drag('width', -70);
    assert(changed.panels.sidebarWidth > initial.panels.sidebarWidth + 50);
    await drag('height', 40);
    const committed = await state();
    near(committed.canvas.zoom, zoom, '手动缩放', 1e-6);
    near(committed.canvas.panX, panX, '手动平移X', 1e-6);
    near(committed.canvas.panY, panY, '手动平移Y', 1e-6);
    for (const cancel of ['escape', 'blur']) {
      await drag('width', 40, true);
      if (cancel === 'escape') await key('escape');
      else await focusTestWindow(context, 'auxiliary');
      await layout({
        sidebarWidth: committed.panels.sidebarWidth,
        inspectorPercent: committed.panels.inspectorPercent,
      });
      await action({ kind: 'releaseInputs' });
      if (cancel === 'blur') await focusTestWindow(context, 'main');
      const after = await layout({
        sidebarWidth: committed.panels.sidebarWidth,
        inspectorPercent: committed.panels.inspectorPercent,
      });
      assert.equal(after.panels.savedSidebarWidth, committed.panels.savedSidebarWidth);
      await unchanged();
    }
    await action({ kind: 'panelReset', axis: 'width' });
    await layout({ sidebarWidth: initial.panels.sidebarWidth });
    await action({ kind: 'panelReset', axis: 'height' });
    await layout({ inspectorPercent: initial.panels.inspectorPercent });
    const minimumWidth = await drag('width', 240);
    near(minimumWidth.panels.sidebarWidth, 260, '右侧最小宽度');
    const handle = minimumWidth.panels.widthHandle;
    const maximumWidth = await drag('width', 40 - Math.floor(handle.x + handle.width / 2));
    assert(maximumWidth.panels.sidebarWidth <= 562);
    assert(maximumWidth.canvas.viewport.width >= 398);
    assert(
      Math.abs(maximumWidth.panels.sidebarWidth - 560) <= 2 ||
        Math.abs(maximumWidth.canvas.viewport.width - 400) <= 2,
      '面板上限须保留 400 px 画布或限制为 560 px',
    );
    await reset();
    await click('menu:view');
    await wait({ kind: 'menu', menu: 'view' });
    await click('command:fit');
    await wait({ kind: 'menu', menu: null });
    await drag('width', -70);
    const fitted = await wait({
      kind: 'imageReady',
      documentId: document.id,
      revision: document.revision,
    });
    assert.equal(fitted.canvas.panX, 0);
    assert.equal(fitted.canvas.panY, 0);
    assert(fitted.canvas.image.width <= fitted.canvas.viewport.width);
    assert(fitted.canvas.image.height <= fitted.canvas.viewport.height);
    await unchanged();
    await screenshot('panels-' + size + '.png');
    report.panelSamples.push({
      size,
      initial: initial.panels,
      committed: committed.panels,
      after: (await state()).panels,
    });
  }
  report.checks.push(
    '两档窗口真实面板拖动、宽度边界、Esc／失焦取消、迟到释放、双击重置与适应视口；已提交范围、固定任务及输入草稿保持',
  );
}
