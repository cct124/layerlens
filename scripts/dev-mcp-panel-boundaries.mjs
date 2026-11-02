// 面板边界与取消只使用开发 MCP 的真实输入和实际几何，不修改组件或存储。
import assert from 'node:assert/strict';
import { placeTestWindow } from './dev-mcp-preflight.mjs';
import { focusTestWindow } from './dev-mcp-render-focus.mjs';

export async function exercisePanelBoundaries(context, helpers) {
  const {
    scenario,
    state,
    wait,
    action,
    click,
    key,
    tool,
    args,
    report,
    screenshot,
    document,
    point,
  } = context;
  const { layout, reset, unchanged, drag } = helpers;
  const native = async (request) => (await tool('dev_window_action', args(request, true))).value;
  const nativeWait = async (condition) => {
    const r = (
      await tool('dev_window_wait', args({ condition, afterSequence: null, timeoutMs: 10000 }))
    ).value;
    assert(r.satisfied, JSON.stringify(r));
    return r.lastState;
  };
  const near = (a, b, label) => assert(Math.abs(a - b) <= 2, label + ': ' + a + ' != ' + b);
  const sync = async (n) =>
    wait({
      kind: 'windowLayout',
      width: Math.round(n.logicalWidth),
      height: Math.round(n.logicalHeight),
      maximized: n.maximized,
    });
  const setSize = async (size) => {
    await native({ kind: 'restore' });
    await nativeWait({ kind: 'state', minimized: false, maximized: false });
    if (size === 'maximized') {
      await native({ kind: 'maximize' });
      await sync(await nativeWait({ kind: 'state', minimized: false, maximized: true }));
    } else {
      await native({ kind: 'resize', size });
      await sync(await nativeWait({ kind: 'size', size }));
      await placeTestWindow(context);
    }
    return layout();
  };
  const keyOnHandle = async (axis, name) => {
    const before = await state();
    const result = await key(name);
    await wait({
      kind: 'tool',
      tool: before.tool,
      keyEventsAtLeast: String(BigInt(result.value.before.input.keyEvents) + 2n),
    });
    const after = await wait({
      kind: 'panelKeyboard',
      axis,
      eventsAfter: before.panels.keyboardEvents,
    });
    assert.equal(after.panels.focusedHandle, axis, '键盘必须落到实际分隔条');
    assert(after.panels.keyboardEvents > before.panels.keyboardEvents, '组件键盘处理必须实际完成');
    assert.equal(after.tool, before.tool);
    return after;
  };
  if (scenario === 'panels-keyboard-cancel') {
    await setSize('default');
    report.panelKeyboardCancellations = [];
    for (const axis of ['width', 'height'])
      for (const mode of ['element', 'escape', 'window']) {
        await reset();
        await click('panel:' + axis);
        const before = await state();
        const input = await key('end', 'down');
        await wait({
          kind: 'tool',
          tool: before.tool,
          keyEventsAtLeast: String(BigInt(input.value.before.input.keyEvents) + 1n),
        });
        const held = await layout({
          sidebarWidth: axis === 'width' ? 260 : null,
          inspectorPercent:
            axis === 'height'
              ? (1 - 180 / (before.panels.inspectorHeight + before.panels.resourcesHeight)) * 100
              : null,
          afterRevision: before.viewRevision,
        });
        assert.equal(held.panels.keyboardEvents, before.panels.keyboardEvents);
        if (axis === 'width')
          assert(Math.abs(held.panels.sidebarWidth - before.panels.sidebarWidth) > 5);
        else assert(Math.abs(held.panels.inspectorPercent - before.panels.inspectorPercent) > 5);
        if (mode === 'element') await click('task:name');
        else if (mode === 'escape') await key('escape');
        else await focusTestWindow(context, 'auxiliary');
        await layout({
          sidebarWidth: before.panels.sidebarWidth,
          inspectorPercent: before.panels.inspectorPercent,
          afterRevision: held.viewRevision,
        });
        if (mode === 'window') await focusTestWindow(context, 'main');
        await action({ kind: 'releaseInputs' });
        const after = await layout({
          sidebarWidth: before.panels.sidebarWidth,
          inspectorPercent: before.panels.inspectorPercent,
        });
        assert.equal(after.panels.savedSidebarWidth, before.panels.savedSidebarWidth);
        assert.equal(after.panels.savedInspectorPercent, before.panels.savedInspectorPercent);
        assert.equal(after.panels.keyboardEvents, before.panels.keyboardEvents);
        await unchanged();
        await click('panel:' + axis);
        const recovered = await keyOnHandle(axis, 'end');
        assert(recovered.panels.keyboardEvents > before.panels.keyboardEvents);
        await unchanged();
        report.panelKeyboardCancellations.push({
          axis,
          mode,
          before: before.panels,
          held: held.panels,
          after: after.panels,
          recovered: recovered.panels,
        });
      }
    await screenshot('panels-keyboard-cancel.png');
    report.checks.push(
      '两轴按住调整键后控件失焦／Esc／真实窗口失焦撤销，迟到松键不保存，后续正常键盘调整可恢复',
    );
    return;
  }
  if (scenario === 'panels-cancel') {
    report.panelCancellations = [];
    for (const axis of ['width', 'height']) {
      await setSize('default');
      await reset();
      const before = await state();
      await drag(axis, 30, true);
      await native({ kind: 'resize', size: 'minimum' });
      await sync(await nativeWait({ kind: 'size', size: 'minimum' }));
      await layout();
      await action({ kind: 'releaseInputs' });
      const after = await layout();
      assert.equal(after.panels.savedSidebarWidth, before.panels.savedSidebarWidth);
      assert.equal(after.panels.savedInspectorPercent, before.panels.savedInspectorPercent);
      await unchanged();
      report.panelCancellations.push({ axis, before: before.panels, after: after.panels });
    }
    await click('tool:region');
    for (const [name, x, y] of [
      ['create', 100, 70],
      ['move', 20, 20],
      ['resize', 32, 32],
    ]) {
      await setSize('default');
      await reset();
      await click('panel:width');
      await keyOnHandle('width', 'home');
      await click('menu:view');
      await wait({ kind: 'menu', menu: 'view' });
      await click('command:actual');
      await wait({ kind: 'menu', menu: null });
      const before = await state(),
        start = point(before, x, y);
      const input = await action({
        kind: 'canvasGesture',
        hold: true,
        steps: [
          { kind: 'move', ...start },
          { kind: 'down', button: 'left' },
          { kind: 'move', x: start.x + 19, y: start.y + 13 },
        ],
      });
      await wait({
        kind: 'canvasInput',
        documentId: document.id,
        revision: document.revision,
        ctrl: false,
        alt: false,
        pointerCaptured: true,
        keyEventsAtLeast: input.value.before.input.keyEvents,
        pointerEventsAtLeast: String(BigInt(input.value.before.input.pointerEvents) + 2n),
      });
      const held = await wait({ kind: 'draft', active: true });
      await native({ kind: 'resize', size: 'minimum' });
      await sync(await nativeWait({ kind: 'size', size: 'minimum' }));
      const cancelled = await wait({
        kind: 'canvasIdle',
        pointerEventsAtLeast: held.input.pointerEvents,
      });
      assert(
        cancelled.panels.sidebarWidth < before.panels.sidebarWidth - 5,
        '缩小窗口必须实际改变面板宽度约束',
      );
      assert.equal(cancelled.panels.savedSidebarWidth, before.panels.savedSidebarWidth);
      await action({ kind: 'releaseInputs' });
      const late = await wait({
        kind: 'canvasIdle',
        pointerEventsAtLeast: held.input.pointerEvents,
      });
      await unchanged();
      report.panelCancellations.push({ name, held, cancelled, late });
    }
    await screenshot('panels-cancel.png');
    report.checks.push(
      '窗口变化撤销两轴面板拖动；实际面板约束和视口变化取消新建／移动／调整画布草稿，迟到释放不提交',
    );
    return;
  }
  report.panelBoundaries = [];
  for (const size of scenario === 'panels-maximized'
    ? ['maximized']
    : ['default', 'minimum', 'maximized']) {
    report.phase = scenario + '-' + size;
    await setSize(size);
    await reset();
    await click('panel:width');
    const wide = await keyOnHandle('width', 'home');
    assert(wide.panels.sidebarWidth <= 562 && wide.canvas.viewport.width >= 398);
    assert(
      Math.abs(wide.panels.sidebarWidth - 560) <= 2 ||
        Math.abs(wide.canvas.viewport.width - 400) <= 2,
    );
    const narrow = await keyOnHandle('width', 'end');
    near(narrow.panels.sidebarWidth, 260, '最小侧栏');
    const left = await keyOnHandle('width', 'arrowLeft');
    assert(left.panels.sidebarWidth > narrow.panels.sidebarWidth);
    const right = await keyOnHandle('width', 'arrowRight');
    near(right.panels.sidebarWidth, narrow.panels.sidebarWidth, '方向键往返');
    await click('panel:height');
    const top = await keyOnHandle('height', 'home');
    near(top.panels.inspectorHeight, 140, '上方面板最小高度');
    const bottom = await keyOnHandle('height', 'end');
    near(bottom.panels.resourcesHeight, 180, '下方面板最小高度');
    const up = await keyOnHandle('height', 'arrowUp');
    assert(up.panels.inspectorHeight < bottom.panels.inspectorHeight);
    const down = await keyOnHandle('height', 'arrowDown');
    near(down.panels.inspectorHeight, bottom.panels.inspectorHeight, '上下键往返');
    near(down.panels.savedInspectorPercent, down.panels.inspectorPercent, '键盘高度持久化');
    await unchanged();
    await screenshot('panels-boundaries-' + size + '.png');
    report.panelBoundaries.push({
      size,
      dpr: down.window.devicePixelRatio,
      wide: wide.panels,
      narrow: narrow.panels,
      top: top.panels,
      bottom: bottom.panels,
    });
  }
  await setSize('default');
  await reset();
  await drag('width', -180);
  const preferred = await state();
  await setSize('minimum');
  const clamped = await state();
  assert(clamped.panels.sidebarWidth < preferred.panels.sidebarWidth);
  assert.equal(clamped.panels.savedSidebarWidth, preferred.panels.savedSidebarWidth);
  await setSize('default');
  const restored = await layout({ sidebarWidth: preferred.panels.sidebarWidth });
  assert.equal(restored.panels.savedSidebarWidth, preferred.panels.savedSidebarWidth);
  await unchanged();
  report.panelPreferenceResize = {
    before: preferred.panels,
    clamped: clamped.panels,
    restored: restored.panels,
  };
  report.checks.push(
    '实际 DPI 下默认／最小／最大化面板高度与宽度极值、真实键盘焦点和方向键、布局缩放后的保存偏好恢复',
  );
}
