// 中键、组合按键与吸附边界全部经开发 MCP 触发真实输入；预期来自网格和明确手势增量。
import assert from 'node:assert/strict';

export async function exerciseCanvasInput({
  scenario,
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
}) {
  report.phase = scenario;
  const initial = await core();
  const selected = await selection({
    kind: 'region',
    documentId: document.id,
    documentRevision: document.revision,
    expectedRevision: initial.selection.selectionRevision,
    bounds: { x: 32, y: 16, width: 64, height: 48 },
  });
  await selection({
    kind: 'createTask',
    snapshotId: selected.value.snapshotId,
    requestId: 'input-fixed',
    name: '输入边界固定任务',
  });
  await click('panel:tasks');
  await click('task:refresh');
  await wait({ kind: 'taskCount', count: 1 });
  await click('tool:region');
  await click('menu:view');
  await click('command:actual');
  const fixed = await state();
  report.samplesOfInput = [];
  const close = (actual, expected, label) =>
    assert(Math.abs(actual - expected) < 0.03, `${label}: ${actual} != ${expected}`);
  async function unchanged(view) {
    assert.equal(view.selectionRevision, fixed.selectionRevision);
    assert.equal(view.snapshotId, fixed.snapshotId);
    assert.deepEqual(view.region, fixed.region);
    assert.deepEqual(view.tasks, fixed.tasks);
    const backend = await core();
    assert.equal(backend.selection.selectionRevision, fixed.selectionRevision);
    assert.equal(backend.selection.scope.snapshotId, fixed.snapshotId);
  }
  const down = (button) => ({ kind: 'down', button });
  const up = (button) => ({ kind: 'up', button });
  const move = (position) => ({ kind: 'move', ...position });
  const offset = (position, x, y) => ({ x: position.x + x, y: position.y + y });
  async function gesture(steps, hold = false) {
    const result = await action({ kind: 'canvasGesture', steps, hold });
    if (hold) return result;
    const view = await wait({
      kind: 'canvasIdle',
      pointerEventsAtLeast: (BigInt(result.value.before.input.pointerEvents) + 2n).toString(),
    });
    report.samplesOfInput.push(view);
    await unchanged(view);
    return view;
  }
  async function resetView() {
    await click('menu:view');
    await wait({ kind: 'menu', menu: 'view' });
    await click('command:actual');
    return wait({ kind: 'menu', menu: null });
  }
  if (scenario === 'input-pan') {
    const start = point(fixed, 160, 96);
    const invalid = await action(
      {
        kind: 'canvasGesture',
        hold: false,
        steps: [move(start), down('middle'), move({ x: -1, y: start.y }), up('middle')],
      },
      true,
    );
    assert.equal(invalid.error.code, 'INVALID_INPUT');
    const rejected = await state();
    assert.equal(rejected.input.pointerEvents, fixed.input.pointerEvents);
    await unchanged(rejected);
    report.checks.push('尾部越界步骤在全部输入前拒绝，没有先按下中键或改变状态');
    for (const [name, x, y] of [
      ['canvas', 160, 96],
      ['inside', 64, 40],
      ['handle', 32, 16],
    ]) {
      const before = await resetView();
      const start = point(before, x, y);
      const end = offset(start, 37, -19);
      const after = await gesture([
        move(start),
        down('middle'),
        move(end),
        up('middle'),
        move(offset(end, 23, 11)),
      ]);
      close(after.canvas.panX - before.canvas.panX, 37, '平移 x');
      close(after.canvas.panY - before.canvas.panY, -19, '平移 y');
      report.checks.push(`从 ${name} 中键平移保持文档范围，松键后的移动不继续平移`);
    }
    const before = await resetView();
    const heldStart = point(before, 160, 96);
    await gesture([move(heldStart), down('middle'), move(offset(heldStart, 31, 17))], true);
    const held = await state();
    assert.equal(held.canvas.panning, true);
    assert.equal(held.canvas.pointerCaptured, true);
    await unchanged(held);
    const busy = await action(
      {
        kind: 'canvasGesture',
        hold: false,
        steps: [move(heldStart), down('middle'), up('middle')],
      },
      true,
    );
    assert.equal(busy.error.code, 'BUSY');
    await key('escape');
    await action({ kind: 'releaseInputs' });
    const cancelled = await wait({
      kind: 'canvasIdle',
      pointerEventsAtLeast: (BigInt(held.input.pointerEvents) + 1n).toString(),
    });
    await unchanged(cancelled);
    close(cancelled.canvas.panX, held.canvas.panX, '取消保留平移 x');
    report.checks.push('中键保留捕获、Esc 取消及显式释放不提交选区');
  } else if (scenario === 'input-chord') {
    for (const [name, x, y] of [
      ['create', 160, 64],
      ['move', 64, 40],
      ['resize', 96, 64],
    ]) {
      for (const firstRelease of ['middle', 'left']) {
        const before = await resetView();
        if (name === 'create') await click('canvas:reframe');
        const start = point(before, x, y),
          draft = offset(start, 17, 13),
          pan = offset(draft, 29, -11);
        const after = await gesture([
          move(start),
          down('left'),
          move(draft),
          down('middle'),
          move(pan),
          up(firstRelease),
          move(offset(pan, 21, 8)),
          up(firstRelease === 'middle' ? 'left' : 'middle'),
          move(offset(pan, 33, 12)),
        ]);
        close(
          after.canvas.panX - before.canvas.panX,
          firstRelease === 'middle' ? 29 : 50,
          '组合平移 x',
        );
        close(
          after.canvas.panY - before.canvas.panY,
          firstRelease === 'middle' ? -11 : -3,
          '组合平移 y',
        );
        report.checks.push(
          `${name} 草稿追加中键后取消，先松 ${firstRelease} 后范围不变，最后松键后停止平移`,
        );
      }
    }
    const before = await resetView();
    const start = point(before, 64, 40);
    await gesture([
      move(start),
      down('left'),
      move(offset(start, 15, 10)),
      { kind: 'escape' },
      up('left'),
    ]);
    report.checks.push('同批 W3C 键盘 Esc 取消草稿，后续左键松开不提交');
  } else {
    // 画布 x=0 与首列图层左边重合，任何输入下距离都相同，画布应优先。
    // W3C 整数坐标不能在带小数的视口中保证 x=4 与 0/8 精确等距。
    for (let round = 0; round < 3; round++) {
      const before = await resetView();
      await click('canvas:reframe');
      await gesture([move(point(before, 2, 32)), down('left'), move(point(before, 2, 80))], true);
      const draft = await wait({ kind: 'draft', active: true });
      assert.deepEqual(draft.canvas.snapX, { position: 0, layerId: null });
      close(draft.canvas.draft.x, 0, '同距画布优先');
      await unchanged(draft);
      report.samplesOfInput.push(draft);
      await key('escape');
      await action({ kind: 'releaseInputs' });
      await wait({
        kind: 'canvasIdle',
        pointerEventsAtLeast: (BigInt(draft.input.pointerEvents) + 1n).toString(),
      });
    }
    report.checks.push('连续三次同距目标稳定选择画布，草稿始终未提交');
    const before = await resetView();
    await click('canvas:reframe');
    // 先靠近右侧图层边 x=248，再向画布外移动；不能粘在 248。
    await gesture(
      [
        move(point(before, 32, 32)),
        down('left'),
        move(point(before, 247, 80)),
        move(point(before, 280, 80)),
      ],
      true,
    );
    const edge = await wait({ kind: 'draft', active: true });
    close(edge.canvas.draft.x + edge.canvas.draft.width, 256, '向外脱离旧吸附');
    assert.deepEqual(edge.canvas.snapX, { position: 256, layerId: null });
    await unchanged(edge);
    report.samplesOfInput.push(edge);
    await key('escape');
    await action({ kind: 'releaseInputs' });
    await wait({
      kind: 'canvasIdle',
      pointerEventsAtLeast: (BigInt(edge.input.pointerEvents) + 1n).toString(),
    });
    // 平移之后重复同距场景，文档坐标和吸附目标不变。
    const center = point(await state(), 160, 96);
    await gesture([move(center), down('middle'), move(offset(center, 43, 27)), up('middle')]);
    const panned = await state();
    await click('canvas:reframe');
    await gesture([move(point(panned, 2, 32)), down('left'), move(point(panned, 2, 80))], true);
    const afterPan = await wait({ kind: 'draft', active: true });
    assert.deepEqual(afterPan.canvas.snapX, { position: 0, layerId: null });
    await unchanged(afterPan);
    await key('escape');
    await action({ kind: 'releaseInputs' });
    await wait({
      kind: 'canvasIdle',
      pointerEventsAtLeast: (BigInt(afterPan.input.pointerEvents) + 1n).toString(),
    });
    report.checks.push('靠边向外移动脱离旧目标，平移后吸附仍使用原文档坐标');
    for (const zoom of [1, 2]) {
      await resetView();
      for (let step = 0; step < 8; step++) {
        const beforeZoom = await state();
        if (Math.abs(beforeZoom.canvas.zoom - zoom) < 0.002) break;
        const deltaY = Math.max(
          -200,
          Math.min(200, Math.round(-Math.log(zoom / beforeZoom.canvas.zoom) / 0.002)),
        );
        const input = await action({ kind: 'scroll', ...point(beforeZoom, 128, 64), deltaY });
        await wait({
          kind: 'canvasPaint',
          documentId: document.id,
          revision: document.revision,
          sceneAtLeast: (BigInt(input.value.before.canvas.sceneRevision) + 1n).toString(),
        });
      }
      const scaled = await state();
      assert(Math.abs(scaled.canvas.zoom - zoom) < 0.002);
      await click('canvas:reframe');
      await gesture(
        [
          move(point(scaled, 32, 32)),
          down('left'),
          move(point(scaled, 256 - 2 / scaled.canvas.zoom, 80)),
        ],
        true,
      );
      const snapped = await wait({ kind: 'draft', active: true });
      assert.deepEqual(snapped.canvas.snapX, { position: 256, layerId: null });
      close(snapped.canvas.draft.x + snapped.canvas.draft.width, 256, '缩放下的画布吸附坐标');
      await unchanged(snapped);
      report.samplesOfInput.push(snapped);
      await key('escape');
      await action({ kind: 'releaseInputs' });
      await wait({
        kind: 'canvasIdle',
        pointerEventsAtLeast: (BigInt(snapped.input.pointerEvents) + 1n).toString(),
      });
      report.checks.push(`缩放 ${zoom} 时距边缘 2 CSS px 的输入吸附至文档 x=256，取消不提交`);
    }
  }
  await unchanged(await state());
}
