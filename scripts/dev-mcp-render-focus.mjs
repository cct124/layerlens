import assert from 'node:assert/strict';

/** 只通过测试 MCP 转移自有窗口；系统前台和可信 DOM 状态必须共同就绪。 */
export async function focusTestWindow(
  { state, tool, args, wait, report },
  target,
  { requireEvent = false } = {},
) {
  assert(['main', 'auxiliary'].includes(target));
  const focused = target === 'main';
  const before = await state();
  const native = (await tool('dev_window_state', args({}))).value;
  const record = { target, before: native, beforeWindow: before.window };
  if (report) {
    const records = (report.focusTransitions ??= []);
    assert(records.length < 256);
    records.push(record);
  }
  if (
    !requireEvent &&
    native.foregroundReady === target &&
    native.visible &&
    !native.minimized &&
    native.focused === focused &&
    native.auxiliaryFocused === !focused &&
    before.window.focused === focused
  ) {
    record.alreadyReady = true;
    return { view: before, window: native };
  }
  const dispatch = (
    await tool('dev_window_action', args({ kind: focused ? 'focusMain' : 'focusAuxiliary' }, true))
  ).value;
  record.dispatch = dispatch;
  const result = (
    await tool(
      'dev_window_wait',
      args({
        condition: { kind: 'foreground', target },
        afterSequence: requireEvent ? dispatch.before.eventSequence : null,
        timeoutMs: 10000,
      }),
    )
  ).value;
  record.wait = result;
  assert(result.satisfied, JSON.stringify(result));
  const view = await wait({
    kind: 'windowFocus',
    focused,
    eventsAfter:
      requireEvent || before.window.focused !== focused
        ? focused
          ? before.window.focusEvents
          : before.window.blurEvents
        : null,
  });
  record.afterWindow = view.window;
  return { view, window: result.lastState, dispatch };
}

/** 已就绪时不重新激活；事件层显示 focused=true 不能替代系统前台。 */
export async function ensureRenderFocus(context) {
  return focusTestWindow(context, 'main');
}
