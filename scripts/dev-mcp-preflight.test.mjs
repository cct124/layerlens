import test from 'node:test';
import assert from 'node:assert/strict';
import { preflightOptions, prepareTestWindow } from './dev-mcp-preflight.mjs';

function fixture() {
  const native = {
    visible: true,
    minimized: false,
    focused: true,
    auxiliaryFocused: false,
    foregroundReady: 'main',
    clientWithinWorkArea: true,
    eventSequence: '1',
    focusEvents: '1',
    blurEvents: '0',
    resizeEvents: '0',
    moveEvents: '0',
    physicalWidth: 1280,
    physicalHeight: 800,
    logicalWidth: 1280,
    logicalHeight: 800,
    scaleFactor: 1,
    nativeFocus: {
      snapshot: {
        desktop: {
          status: 'available',
          callingThreadReceivesInput: true,
          windowThreadReceivesInput: true,
        },
      },
    },
  };
  const view = {
    window: {
      visible: true,
      focused: true,
      displayRevision: '1',
      focusEvents: '1',
      blurEvents: '0',
      width: 1280,
      height: 800,
      devicePixelRatio: 1,
      canvasViewport: null,
    },
    canvas: null,
    activeDocument: null,
  };
  const calls = [];
  const context = {
    report: { checks: [] },
    state: async () => structuredClone(view),
    args: (r) => r,
    tool: async (name, request) => {
      calls.push({ name, request });
      if (name === 'dev_window_action')
        return { value: { before: structuredClone(native), dispatched: true } };
      if (name === 'dev_window_wait')
        return { value: { satisfied: true, lastState: structuredClone(native) } };
      assert.equal(name, 'dev_window_state');
      return { value: structuredClone(native) };
    },
    wait: async (condition) => {
      calls.push({ condition });
      return structuredClone(view);
    },
  };
  return { native, view, calls, context };
}

test('预定显示条件拒绝无效尺寸、非有限及超限 DPR，不提供设备模拟', () => {
  assert.deepEqual(preflightOptions({}), { size: null, dpr: null });
  assert.deepEqual(preflightOptions({ 'window-size': 'minimum', 'expect-dpr': '1.5' }), {
    size: 'minimum',
    dpr: 1.5,
  });
  for (const value of ['', '0', '-1', 'Infinity', 'NaN', '9'])
    assert.throws(() => preflightOptions({ 'expect-dpr': value }));
  assert.throws(() => preflightOptions({ 'window-size': 'arbitrary' }));
});

test('输入桌面不接收输入或查询不可用时，在任何窗口动作前失败并保存末态', async () => {
  for (const field of ['callingThreadReceivesInput', 'windowThreadReceivesInput', 'status']) {
    const { context, native, calls } = fixture();
    native.nativeFocus.snapshot.desktop[field] = field === 'status' ? 'unavailable' : false;
    await assert.rejects(prepareTestWindow(context, { size: 'minimum', dpr: 1 }), /输入桌面/);
    assert.equal(context.report.preflight.stage, 'input-desktop');
    assert.equal(context.report.preflight.passed, false);
    assert(context.report.preflight.terminal.nativeAfter);
    assert(calls.every((c) => c.name === 'dev_window_state'));
  }
});

test('实际 DPR 与预定值不同不接受；原生和前端不一致也不能当作通过', async () => {
  for (const mismatch of ['expected', 'projection']) {
    const { context, view } = fixture();
    if (mismatch === 'projection') view.window.devicePixelRatio = 1.5;
    await assert.rejects(prepareTestWindow(context, { size: null, dpr: 2 }));
    const r = context.report.preflight;
    assert.equal(r.passed, false);
    assert.equal(r.stage, 'display');
    assert(r.terminal.ui);
    assert.equal(r.display.ui.window.devicePixelRatio, view.window.devicePixelRatio);
  }
});

test('尺寸动作必须等原生完成及前端同步，超时保留回执和末态', async () => {
  for (const satisfied of [false, true]) {
    const { context, native, calls } = fixture();
    const original = context.tool;
    context.tool = async (name, request) =>
      name === 'dev_window_wait'
        ? { value: { satisfied, timedOut: !satisfied, lastState: structuredClone(native) } }
        : original(name, request);
    const result = prepareTestWindow(context, { size: 'default', dpr: 1 });
    if (!satisfied) {
      await assert.rejects(result, /尺寸/);
      assert.equal(context.report.preflight.resizeWait.timedOut, true);
      assert(!calls.some((c) => c.condition));
    } else {
      assert.equal((await result).passed, true);
      assert(calls.some((c) => c.condition?.kind === 'display'));
      assert.deepEqual(context.report.preflight.expected, { size: 'default', dpr: 1 });
    }
  }
});
