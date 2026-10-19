import test from 'node:test';
import assert from 'node:assert/strict';
import {
  captureDisplayEnvironment,
  createDisplayGuard,
  displayIssues,
  displaySignature,
} from './dev-mcp-render-environment.mjs';

function fixture() {
  const native = {
    eventSequence: '3',
    focusEvents: '1',
    blurEvents: '0',
    resizeEvents: '0',
    moveEvents: '0',
    focused: true,
    auxiliaryFocused: false,
    visible: true,
    minimized: false,
    x: 20,
    y: 40,
    physicalWidth: 1920,
    physicalHeight: 1350,
    scaleFactor: 1.5,
  };
  return {
    errors: [],
    nativeBefore: { ...native },
    nativeAfter: { ...native },
    ui: {
      instanceId: 'instance',
      frontendId: 'page',
      documentEpoch: '1',
      window: {
        displayRevision: '1',
        visible: true,
        focused: true,
        focusEvents: '1',
        blurEvents: '0',
        width: 1280,
        height: 900,
        devicePixelRatio: 1.5,
        canvasViewport: { x: 10, y: 30, width: 640, height: 420 },
      },
      render: {
        phase: 'ready',
        surface: { width: 960, height: 630, cssWidth: 640, cssHeight: 420, configuredDpr: 1.5 },
      },
    },
  };
}
test('稳定显示及释放后的空 surface 可接受，跨进程签名排除身份和计数', () => {
  const base = fixture(),
    next = fixture();
  next.ui.render = { phase: 'released', surface: null };
  assert.deepEqual(displayIssues(next, base), []);
  next.ui.instanceId = 'another';
  assert.deepEqual(displaySignature(next), displaySignature(base));
  assert(displayIssues(next, base).includes('frontend-identity-changed'));
});
test('尺寸／DPR／焦点即使恢复，历史变化仍拒绝', () => {
  for (const kind of ['display', 'focus', 'resize']) {
    const base = fixture(),
      next = fixture();
    if (kind === 'display') next.ui.window.displayRevision = '3';
    if (kind === 'focus') next.ui.window.blurEvents = '1';
    if (kind === 'resize') next.nativeBefore.resizeEvents = next.nativeAfter.resizeEvents = '2';
    assert(displayIssues(next, base).length > 0, kind);
  }
});
test('当前 DPR 漂移、旧 backing store、缺失／未知和不一致原生观测均拒绝', () => {
  const changed = fixture();
  changed.ui.window.devicePixelRatio = 1;
  assert(displayIssues(changed, fixture()).includes('surface-dpr-mismatch'));
  const mismatched = fixture();
  mismatched.ui.render.surface.width = 640;
  assert(displayIssues(mismatched).includes('surface-dpr-mismatch'));
  const missing = fixture();
  missing.ui.render.surface = null;
  assert(displayIssues(missing).includes('surface-missing'));
  const unknown = fixture();
  unknown.nativeAfter.focused = null;
  assert(displayIssues(unknown).includes('window-not-focused'));
  const moving = fixture();
  moving.nativeAfter.eventSequence = '4';
  assert(displayIssues(moving).includes('native-changed-during-capture'));
});
test('采样失败仍保留其他可用末态，不能伪造为空环境成功', async () => {
  const current = fixture();
  const result = await captureDisplayEnvironment({
    args: (x) => x,
    state: async () => {
      throw new Error('disconnected');
    },
    tool: async () => ({ value: current.nativeAfter }),
  });
  assert.equal(result.errors.length, 1);
  assert.equal(result.ui, null);
  assert.equal(result.nativeAfter.scaleFactor, 1.5);
  assert.deepEqual(displayIssues(result), ['environment-unavailable']);
});
test('固定循环保留全部样本，失败终态不覆盖成功历史且不自动重试', async () => {
  const current = fixture(),
    report = {};
  let calls = 0;
  const guard = createDisplayGuard(
    {
      args: (x) => x,
      state: async () => {
        calls++;
        return { ...current.ui, renderLab: current.ui.render };
      },
      tool: async () => ({ value: { ...current.nativeAfter } }),
    },
    report,
  );
  await guard.sample('ready', 0);
  await guard.sample('released', 0);
  current.ui.window.displayRevision = '2';
  await assert.rejects(guard.finish('failure'), /display-history-changed/);
  assert.equal(calls, 3);
  assert.equal(report.displayEnvironment.samples.length, 3);
  assert.equal(report.displayEnvironment.valid, false);
  assert.equal(report.displayEnvironment.terminal.label, 'terminal-failure');
  assert.equal(report.displayEnvironment.terminal.ui.window.displayRevision, '2');
});
