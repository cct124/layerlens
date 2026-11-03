// 验证真实窗口边框拖动，不以 set_size 的结果代替系统命中和手势。
import assert from 'node:assert/strict';

export async function exerciseWindowResize(context) {
  const { native, nativeWait, nativeState, layout, state, report, click, wait, tool, args } =
    context;
  await click('menu:view');
  await wait({ kind: 'menu', menu: 'view' });
  await click('command:actual');
  await wait({ kind: 'menu', menu: null });
  const edges = [
    ['left', -1, 0],
    ['right', 1, 0],
    ['top', 0, -1],
    ['bottom', 0, 1],
    ['topLeft', -1, -1],
    ['topRight', 1, -1],
    ['bottomLeft', -1, 1],
    ['bottomRight', 1, 1],
  ];
  report.edgeResizes = [];
  for (const size of ['minimum', 'default']) {
    await native({ kind: 'restore' });
    await native({ kind: 'resize', size });
    await nativeWait({ kind: 'size', size });
    await native({ kind: 'center' });
    await nativeWait({ kind: 'withinWorkArea' });
    await nativeWait({ kind: 'foreground', target: 'main' });
    const initial = await layout(await nativeState(), 'edge-' + size + '-before');
    for (const [edge, dx, dy] of edges) {
      const directions =
        size === 'minimum' ? ['inward', 'outward', 'inward'] : ['inward', 'outward'];
      for (const direction of directions) {
        const before = await nativeState();
        report.phase = 'window-resize-' + size + '-' + edge + '-' + direction;
        report.edgeResizeAttempt = { size, edge, direction, before };
        const amount = Math.round(48 * before.scaleFactor) * (direction === 'outward' ? 1 : -1);
        const width = Math.max(
          Math.round(900 * before.scaleFactor),
          before.physicalWidth + Math.abs(dx) * amount,
        );
        const height = Math.max(
          Math.round(600 * before.scaleFactor),
          before.physicalHeight + Math.abs(dy) * amount,
        );
        await native({ kind: 'resizeEdge', edge, direction });
        await nativeWait(
          { kind: 'clientSize', width, height },
          width !== before.physicalWidth || height !== before.physicalHeight
            ? before.eventSequence
            : null,
        );
        await nativeWait({ kind: 'foreground', target: 'main' });
        const after = await nativeState();
        const probe = after.nativeResize;
        assert.equal(probe?.status, 'available', '边框动作必须保留原生命中观测');
        assert.equal(probe.probe?.complete, true);
        assert.equal(probe.probe.edge, edge);
        assert(probe.probe.candidates.length >= 1 && probe.probe.candidates.length <= 6);
        const chosen = probe.probe.candidates.at(-1);
        assert.deepEqual(chosen.point, probe.probe.matched);
        assert.equal(chosen.hit, probe.probe.expectedHit);
        assert(['main', 'mainChild'].includes(chosen.target), '仅自有实际鼠标目标可通过边框命中');
        assert.equal(after.scaleFactor, before.scaleFactor);
        assert.equal(
          after.monitors.find((m) => m.current)?.index,
          before.monitors.find((m) => m.current)?.index,
        );
        assert(
          Math.abs(
            after.x - (before.x - (dx < 0 ? after.physicalWidth - before.physicalWidth : 0)),
          ) <= 2,
          '边框拖动须保持水平对边',
        );
        assert(
          Math.abs(
            after.y - (before.y - (dy < 0 ? after.physicalHeight - before.physicalHeight : 0)),
          ) <= 2,
          '边框拖动须保持垂直对边',
        );
        const view = await layout(after, 'edge-' + size, false);
        assert.equal(view.canvas.zoom, initial.canvas.zoom);
        assert.equal(view.canvas.panX, initial.canvas.panX);
        assert.equal(view.canvas.panY, initial.canvas.panY);
        report.edgeResizes.push({
          size,
          edge,
          direction,
          before,
          after,
          window: view.window,
          canvas: view.canvas,
        });
      }
    }
    await layout(await nativeState(), 'edge-' + size + '-after');
    assert.equal((await state()).window.focused, true);
  }
  report.phase = 'window-resize-maximized-rejections';
  await native({ kind: 'maximize' });
  const maximized = await nativeWait({ kind: 'state', minimized: false, maximized: true });
  report.edgeResizeRejections = [];
  for (const [edge] of edges) {
    const result = await tool(
      'dev_window_action',
      args({ kind: 'resizeEdge', edge, direction: 'inward' }, true),
      true,
    );
    assert.equal(result.error.code, 'INVALID_INPUT');
    const after = await nativeState();
    assert.equal(after.maximized, true);
    for (const key of ['x', 'y', 'physicalWidth', 'physicalHeight'])
      assert.equal(after[key], maximized[key]);
    report.edgeResizeRejections.push({ edge, result, after });
  }
  await layout(maximized, 'maximized');
  report.checks.push(
    '真实四边／四角拖动保持对边、最小尺寸约束、Canvas 布局／DPR、视图与固定选区／任务，最大化时拒绝边缘手势且保持几何',
  );
}
