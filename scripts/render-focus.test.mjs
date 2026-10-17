import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ensureRenderFocus } from './dev-mcp-render-focus.mjs';
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
              ? { focused, auxiliaryFocused: !focused }
              : { satisfied: true },
        };
      },
      wait: async (condition) => {
        called.push(condition);
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
