import assert from 'node:assert/strict';

/** 已在本实例中的焦点不重复设置；需切换时等新前端 focus 事件，不能匹配旧的 focused=true。 */
export async function ensureRenderFocus({ state, tool, args, wait }) {
  const before = await state();
  const native = (await tool('dev_window_state', args({}))).value;
  if (native.focused === true && !native.auxiliaryFocused && before.window.focused) return;
  await tool('dev_window_action', args({ kind: 'focusMain' }, true));
  const result = (
    await tool(
      'dev_window_wait',
      args({
        condition: { kind: 'focus', main: true, auxiliary: false },
        afterSequence: null,
        timeoutMs: 10000,
      }),
    )
  ).value;
  assert(result.satisfied, JSON.stringify(result));
  await wait({ kind: 'windowFocus', focused: true, eventsAfter: before.window.focusEvents });
}
