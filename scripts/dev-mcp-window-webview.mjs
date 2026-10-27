// 窗口行为经开发 MCP 调用真实系统 API；UI 行为经 WebDriver。每次失焦都核对两层事件。
import assert from 'node:assert/strict';
import { focusTestWindow } from './dev-mcp-render-focus.mjs';

export async function exerciseWindows(context) {
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
  } = context;
  report.phase = scenario;
  report.windowSamples = [];
  const native = async (request) => (await tool('dev_window_action', args(request, true))).value;
  const nativeState = async () => (await tool('dev_window_state', args({}))).value;
  if (scenario === 'window-topology') {
    report.displayTopology = await nativeState();
    report.checks.push('真实显示拓扑、工作区及输入桌面已观测');
    return { closeWindow: false };
  }
  async function nativeWait(condition, afterSequence = null) {
    const result = (
      await tool('dev_window_wait', args({ condition, afterSequence, timeoutMs: 10000 }))
    ).value;
    assert.equal(result.satisfied, true, JSON.stringify(result));
    return result.lastState;
  }
  async function focus(main, requireEvent = true) {
    const {
      view,
      window,
      dispatch: input,
    } = await focusTestWindow(context, main ? 'main' : 'auxiliary', { requireEvent });
    if (requireEvent)
      assert(
        BigInt(main ? window.focusEvents : window.blurEvents) >
          BigInt(main ? input.before.focusEvents : input.before.blurEvents),
      );
    report.windowSamples.push({ operation: main ? 'focusMain' : 'focusAuxiliary', window, view });
    return view;
  }
  // 先做一次自有窗口之间的转移，建立可观测的原生焦点事件基线。
  await focus(false, false);
  await focus(true);
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
    requestId: 'window-fixed',
    name: '窗口失焦固定任务',
  });
  await wait({ kind: 'applied', atLeast: (await core()).sequence });
  await click('panel:tasks');
  await click('task:refresh');
  await wait({ kind: 'taskCount', count: 1 });
  await click('tool:region');
  const fixed = await state();
  async function unchanged(view) {
    view ??= await state();
    assert.equal(view.selectionRevision, fixed.selectionRevision);
    assert.equal(view.snapshotId, fixed.snapshotId);
    assert.deepEqual(view.region, fixed.region);
    assert.deepEqual(view.tasks, fixed.tasks);
    const current = await core();
    assert.equal(current.selection.selectionRevision, fixed.selectionRevision);
    assert.equal(current.selection.scope.snapshotId, fixed.snapshotId);
  }
  async function layout(window, name) {
    let view = await wait({
      kind: 'windowLayout',
      width: Math.round(window.logicalWidth),
      height: Math.round(window.logicalHeight),
      maximized: window.maximized,
    });
    view = await wait({
      kind: 'canvasPaint',
      documentId: document.id,
      revision: document.revision,
      sceneAtLeast: view.canvas.sceneRevision,
    });
    assert.equal(view.window.devicePixelRatio, window.scaleFactor);
    const title = view.window.titlebar;
    assert(title && title.height > 0 && title.height <= 40);
    assert(Math.abs(title.y) <= 1);
    assert.equal(view.window.controls.length, 3);
    assert.deepEqual(
      view.window.controls.map((c) => c.target),
      ['window:minimize', 'window:maximize', 'window:close'],
    );
    let right = -1;
    for (const control of view.window.controls) {
      const r = control.rect;
      assert(!control.disabled && r.width >= 30 && r.height > 0);
      assert(r.x >= right && r.x + r.width <= view.window.width + 1);
      assert(r.y >= title.y && r.y + r.height <= title.y + title.height + 1);
      right = r.x + r.width;
    }
    const viewport = view.canvas.viewport;
    assert(viewport.width > 0 && viewport.height > 0 && viewport.y >= title.y + title.height);
    assert(viewport.x + viewport.width <= view.window.width + 1);
    assert(viewport.y + viewport.height <= view.window.height + 1);
    await unchanged(view);
    report.windowSamples.push({ operation: name, window, view });
    await screenshot('window-' + name + '.png');
    return view;
  }
  if (scenario === 'window-controls') {
    for (const size of ['minimum', 'default']) {
      await native({ kind: 'resize', size });
      await layout(await nativeWait({ kind: 'size', size }), size);
    }
    await click('window:maximize');
    await layout(
      await nativeWait({ kind: 'state', minimized: false, maximized: true }),
      'maximized',
    );
    await click('window:maximize');
    await layout(
      await nativeWait({ kind: 'state', minimized: false, maximized: false }),
      'restored',
    );
    await action({ kind: 'titlebarDoubleClick' });
    await nativeWait({ kind: 'state', minimized: false, maximized: true });
    await action({ kind: 'titlebarDoubleClick' });
    await nativeWait({ kind: 'state', minimized: false, maximized: false });
    // 最小化时 WebView 可停止绘制；先等原生状态，再从受限原生工具恢复。
    await click('window:minimize');
    report.windowSamples.push({
      operation: 'minimized',
      window: await nativeWait({ kind: 'state', minimized: true, maximized: false }),
    });
    await native({ kind: 'restore' });
    await focus(true, false);
    await layout(
      await nativeWait({ kind: 'state', minimized: false, maximized: false }),
      'recovered',
    );
    // 同一请求 ID 重放绝不重复切换系统状态。
    const replay = args({ kind: 'resize', size: 'default' }, true);
    const receipt = await tool('dev_window_action', replay);
    assert.deepEqual(await tool('dev_window_action', replay), receipt);
    const timeout = (
      await tool(
        'dev_window_wait',
        args({
          condition: { kind: 'state', minimized: true, maximized: true },
          afterSequence: null,
          timeoutMs: 1,
        }),
      )
    ).value;
    assert.equal(timeout.timedOut, true);
    assert.equal(timeout.lastState.minimized, false);
    report.checks.push(
      '最小／默认／最大化／还原布局与三窗口按钮可达，真实标题栏双击和最小化恢复保持已提交范围及固定任务',
      '原生动作回执重放和有界等待超时保留实际末态',
    );
  } else if (scenario === 'window-blur') {
    await click('menu:view');
    await wait({ kind: 'menu', menu: 'view' });
    await click('command:actual');
    await wait({ kind: 'menu', menu: null });
    for (const [name, x, y, button] of [
      ['create', 160, 64, 'left'],
      ['move', 64, 40, 'left'],
      ['resize', 96, 64, 'left'],
      ['pan', 160, 96, 'middle'],
    ]) {
      report.phase = 'window-blur-' + name;
      await focus(true, false);
      const before = await state();
      const start = point(before, x, y);
      const input = await action({
        kind: 'canvasGesture',
        hold: true,
        steps: [
          { kind: 'move', ...start },
          { kind: 'down', button },
          { kind: 'move', x: start.x + 19, y: start.y + 13 },
        ],
      });
      const held = await wait({
        kind: 'canvasInput',
        documentId: document.id,
        revision: document.revision,
        ctrl: false,
        alt: false,
        pointerCaptured: true,
        keyEventsAtLeast: input.value.before.input.keyEvents,
        pointerEventsAtLeast: String(BigInt(input.value.before.input.pointerEvents) + 2n),
      });
      if (button === 'left') await wait({ kind: 'draft', active: true });
      if (button === 'middle') assert.equal(held.canvas.panning, true);
      await focus(false);
      const cancelled = await wait({
        kind: 'canvasIdle',
        pointerEventsAtLeast: held.input.pointerEvents,
      });
      await unchanged(cancelled);
      await focus(true);
      // 真正失焦已取消输入；此后才派发迟到松键和无按键移动，不能先用 Esc 掩盖问题。
      await action({ kind: 'releaseInputs' });
      const move = await action({ kind: 'movePointer', x: start.x + 39, y: start.y + 29 });
      const late = await wait({
        kind: 'canvasIdle',
        pointerEventsAtLeast: (BigInt(move.value.before.input.pointerEvents) + 1n).toString(),
      });
      await unchanged(late);
      assert.equal(late.canvas.panX, cancelled.canvas.panX);
      assert.equal(late.canvas.panY, cancelled.canvas.panY);
      report.windowSamples.push({ operation: name, held, cancelled, late });
    }
    await screenshot('window-blur.png');
    report.checks.push(
      '新建／移动／调整草稿及中键平移在系统真实失焦后取消捕获，迟到松键和移动不继续操作，已提交范围与固定任务保留',
    );
  } else if (scenario === 'window-menu') {
    await click('menu:file');
    await wait({ kind: 'menu', menu: 'file' });
    await focus(false);
    await wait({ kind: 'menu', menu: null });
    await focus(true);
    await key('alt', 'down');
    await key('v');
    await key('alt', 'up');
    await wait({ kind: 'menu', menu: 'view' });
    await key('arrowRight');
    await wait({ kind: 'menu', menu: 'window' });
    await key('escape');
    await wait({ kind: 'menu', menu: null });
    await unchanged();
    await action({ kind: 'fillTaskName', text: '焦点恢复' });
    await key('v');
    await key('m');
    const inputEscape = await key('escape');
    await unchanged(
      await wait({
        kind: 'tool',
        tool: 'region',
        keyEventsAtLeast: (BigInt(inputEscape.value.before.input.keyEvents) + 2n).toString(),
      }),
    );
    await click('canvas');
    await key('v');
    let view = await wait({ kind: 'selection', after: fixed.selectionRevision, hasScope: false });
    await wait({ kind: 'selectionIdle' });
    const toolKey = await key('m');
    const current = await wait({
      kind: 'tool',
      tool: 'region',
      keyEventsAtLeast: (BigInt(toolKey.value.before.input.keyEvents) + 2n).toString(),
    });
    assert.equal(current.tool, 'region');
    assert.deepEqual(current.tasks, fixed.tasks);
    assert.equal(view.snapshotId, null);
    await screenshot('window-menu.png');
    report.checks.push(
      '失焦关闭菜单，恢复后 Alt 访问键、方向键与 Esc 正常；输入框隔离 V／M／Esc，画布快捷键恢复且固定任务保留',
    );
  } else if (scenario === 'window-system' || scenario === 'window-cross-monitor') {
    await native({ kind: 'restore' });
    await native({ kind: 'resize', size: 'minimum' });
    await nativeWait({ kind: 'size', size: 'minimum' });
    await native({ kind: 'center' });
    await nativeWait({ kind: 'withinWorkArea' });
    await focus(true, false);
    const before = await nativeState();
    if (scenario === 'window-system') {
      await native({ kind: 'dragTitlebar', destination: { kind: 'offset' } });
      await nativeWait(
        {
          kind: 'position',
          x: Math.round(before.x + 48 * before.scaleFactor),
          y: Math.round(before.y + 32 * before.scaleFactor),
          tolerance: 3,
        },
        before.eventSequence,
      );
      await layout(await nativeState(), 'dragged');
      for (const [shortcut, side] of [
        ['snapLeft', 'left'],
        ['snapRight', 'right'],
      ]) {
        await native({ kind: 'systemShortcut', shortcut });
        await nativeWait({ kind: 'snapped', side });
        await layout(await nativeState(), 'snap-' + side);
        await native({ kind: 'restore' });
        await native({ kind: 'resize', size: 'minimum' });
        await native({ kind: 'center' });
        await focus(true, false);
      }
      await native({ kind: 'dragTitlebar', destination: { kind: 'topEdge' } });
      await nativeWait({ kind: 'state', minimized: false, maximized: true });
      await layout(await nativeState(), 'drag-maximized');
      await native({ kind: 'restore' });
      await native({ kind: 'resize', size: 'minimum' });
      await native({ kind: 'center' });
      report.checks.push('真实系统标题栏拖动、Win+方向键贴靠及顶部拖动最大化，固定选区／任务不变');
    } else {
      const first = before.monitors.find((m) => m.current),
        other = before.monitors.find((m) => !m.current);
      assert(first && other, '真实跨屏测试需要至少两个可用显示器');
      report.monitorTransitions = [];
      for (const target of [other, first]) {
        const prior = await nativeState();
        await native({
          kind: 'dragTitlebar',
          destination: { kind: 'monitor', index: target.index },
        });
        const arrived = await nativeWait(
          { kind: 'monitor', index: target.index },
          prior.eventSequence,
        );
        await nativeWait({ kind: 'scaleFactor', value: target.scaleFactor });
        await native({ kind: 'center' });
        await nativeWait({ kind: 'withinWorkArea' });
        const view = await layout(await nativeState(), 'monitor-' + target.index);
        assert(
          Math.abs(view.window.width - before.logicalWidth) <= 1 &&
            Math.abs(view.window.height - before.logicalHeight) <= 1,
          '跨屏后的逻辑尺寸只允许原生像素取整误差',
        );
        report.monitorTransitions.push({ target, arrived, view });
      }
      report.checks.push('真实跨屏来回拖动、原生／前端 DPI 和布局同步，固定选区／任务不变');
    }
  } else if (scenario === 'window-picker') {
    report.filePicker = [];
    const opening = await core();
    await click('menu:file');
    await wait({ kind: 'menu', menu: 'file' });
    await click('command:open');
    report.filePicker.push({ opened: await nativeWait({ kind: 'filePicker', opened: true }) });
    const invalid = await tool(
      'dev_window_action',
      args({ kind: 'chooseSample', sample: '../outside.psd' }, true),
      true,
    );
    assert(invalid.error);
    assert.equal((await nativeState()).filePicker.opened, true);
    await native({ kind: 'cancelFilePicker' });
    await nativeWait({ kind: 'filePicker', opened: false });
    await nativeWait({ kind: 'foreground', target: 'main' });
    await wait({ kind: 'filePickerIdle' });
    assert.deepEqual((await core()).documents, opening.documents);
    await unchanged();
    await click('menu:file');
    await wait({ kind: 'menu', menu: 'file' });
    await click('command:open');
    await nativeWait({ kind: 'filePicker', opened: true });
    await native({ kind: 'chooseSample', sample: 'bitmap-rle.psd' });
    await nativeWait({ kind: 'filePicker', opened: false });
    await nativeWait({ kind: 'foreground', target: 'main' });
    await wait({ kind: 'filePickerIdle' });
    const opened = await context.waitCore({ kind: 'documentCount', count: 2 });
    const added = opened.documents.find((d) => d.id !== document.id);
    assert(added);
    const shown = await wait({
      kind: 'imageReady',
      documentId: added.id,
      revision: added.revision,
    });
    assert.equal(shown.canvas.documentWidth, 4);
    assert.equal(shown.canvas.documentHeight, 3);
    assert.deepEqual(shown.tasks, fixed.tasks);
    report.filePicker.push({ opened, shown });
    await screenshot('window-picker-opened.png');
    report.checks.push(
      '真实系统文件选择器打开／取消保持状态，越界样本拒绝，再次选取样本完成真实桌面加载',
    );
  } else {
    assert(['window-exit', 'window-alt-f4'].includes(scenario));
    const arm = async (checkpoint) =>
      (
        await tool(
          'dev_test_control',
          args({ kind: 'arm', checkpoint, mode: 'pause', timeoutMs: 30000 }, true),
        )
      ).value.gate.id;
    const reached = async (id) => {
      const result = (
        await tool(
          'dev_test_control',
          args({ kind: 'wait', id, phase: 'reached', timeoutMs: 10000 }),
        )
      ).value;
      assert.equal(result.satisfied, true, JSON.stringify(result));
      report.checkpoints ??= [];
      report.checkpoints.push(result.gate);
    };
    const content = await arm('desktopContent');
    await click('task:' + fixed.tasks[0].id + ':view');
    await reached(content);
    const preview = await arm('previewPublication');
    await workspace({ kind: 'reload', documentId: document.id });
    await waitCore({ kind: 'idle' });
    await reached(preview);
    report.finalWindow = await nativeState();
    await screenshot('window-exit.png');
    report.checks.push('固定任务内容交付及预览交付检查点均已到达，正常关闭前有真实在途操作');
    // 外部运行器收集最后状态后点击真实关闭按钮，再独立等待退出。
    return { closeWindow: true, systemClose: scenario === 'window-alt-f4' };
  }
  return { closeWindow: false };
}
