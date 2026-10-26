import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ensureRenderFocus, focusTestWindow } from './dev-mcp-render-focus.mjs';
test('已有双层焦点不重复触发原生 set_focus；需要切换时等待新事件', async () => {
  for (const focused of [true, false]) {
    const called = [];
    await ensureRenderFocus({
      state: async () => ({ window: { focused, focusEvents: '7' } }),
      args: (r) => r,
      tool: async (name) => {
        called.push(name);
        return {
          value:
            name === 'dev_window_state'
              ? {
                  focused,
                  auxiliaryFocused: !focused,
                  foregroundReady: focused ? 'main' : 'auxiliary',
                  visible: true,
                  minimized: false,
                }
              : { satisfied: true },
        };
      },
      wait: async (condition) => {
        called.push(condition);
        return { window: { focused: true } };
      },
    });
    if (focused) assert.deepEqual(called, ['dev_window_state']);
    else
      assert.deepEqual(called, [
        'dev_window_state',
        'dev_window_action',
        'dev_window_wait',
        { kind: 'windowFocus', focused: true, eventsAfter: '7' },
      ]);
  }
});

test('DOM 和 Tauri 旧焦点不能跳过系统前台等待，失败保留末态', async () => {
  const calls = [],
    report = {};
  const context = {
    report,
    args: (r) => r,
    state: async () => ({ window: { focused: true, focusEvents: '1' } }),
    tool: async (name, request) => {
      calls.push({ name, request });
      return {
        value:
          name === 'dev_window_state'
            ? { focused: true, auxiliaryFocused: false, foregroundReady: null, visible: true }
            : name === 'dev_window_action'
              ? { before: { eventSequence: '1' } }
              : { satisfied: false, lastState: { foregroundReady: null } },
      };
    },
    wait: async () => assert.fail('系统前台失败后不得按 DOM 状态放行'),
  };
  await assert.rejects(ensureRenderFocus(context), /foregroundReady/);
  assert.equal(calls.length, 3);
  assert.deepEqual(calls[2].request.condition, { kind: 'foreground', target: 'main' });
  assert.equal(report.focusTransitions[0].wait.satisfied, false);
});

test('切到辅助窗口必须等待新原生序号和可信 blur，拒绝外部目标', async () => {
  const conditions = [];
  const context = {
    args: (r) => r,
    state: async () => ({ window: { focused: true, blurEvents: '2' } }),
    tool: async (name, request) => {
      if (name === 'dev_window_state') return { value: { foregroundReady: 'main', focused: true } };
      if (name === 'dev_window_action') return { value: { before: { eventSequence: '9' } } };
      conditions.push(request);
      return { value: { satisfied: true, lastState: { foregroundReady: 'auxiliary' } } };
    },
    wait: async (condition) => {
      conditions.push(condition);
      return { window: { focused: false, blurEvents: '3' } };
    },
  };
  await focusTestWindow(context, 'auxiliary', { requireEvent: true });
  assert.deepEqual(conditions[0], {
    condition: { kind: 'foreground', target: 'auxiliary' },
    afterSequence: '9',
    timeoutMs: 10000,
  });
  assert.deepEqual(conditions[1], { kind: 'windowFocus', focused: false, eventsAfter: '2' });
  await assert.rejects(focusTestWindow(context, 'other'));
});
