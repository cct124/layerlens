// 加载样本前核验真实输入桌面、前台及显示条件；只调用受限开发 MCP。
import assert from 'node:assert/strict';
import { captureDisplayEnvironment, displayIssues } from './dev-mcp-render-environment.mjs';
import { ensureRenderFocus } from './dev-mcp-render-focus.mjs';

/** 显式条件是断言，不通过设备模拟或修改前端状态实现。 */
export function preflightOptions(options) {
  const size = options['window-size'] ?? null;
  const dpr = options['expect-dpr'] === undefined ? null : Number(options['expect-dpr']);
  assert(size === null || ['minimum', 'default', 'renderLab'].includes(size));
  assert(dpr === null || (Number.isFinite(dpr) && dpr > 0 && dpr <= 8));
  return { size, dpr };
}

/** 每项 GUI 实例仅执行一次；错误携带阶段和实际末态，不能改判成业务失败。 */
export async function prepareTestWindow(context, expected) {
  const { report, state, tool, args, wait } = context;
  const record = (report.preflight = { expected, passed: false, stage: 'input-desktop' });
  report.phase = 'preflight';
  try {
    record.before = (await tool('dev_window_state', args({}))).value;
    const desktop = record.before.nativeFocus?.snapshot?.desktop;
    assert(
      desktop?.status === 'available' &&
        desktop.callingThreadReceivesInput &&
        desktop.windowThreadReceivesInput,
      '输入桌面未就绪或查询不可用。',
    );
    record.stage = 'foreground';
    await ensureRenderFocus(context);
    if (expected.size) {
      record.stage = 'window-size';
      record.resize = (
        await tool('dev_window_action', args({ kind: 'resize', size: expected.size }, true))
      ).value;
      const resized = (
        await tool(
          'dev_window_wait',
          args({
            condition: { kind: 'size', size: expected.size },
            afterSequence: null,
            timeoutMs: 10000,
          }),
        )
      ).value;
      record.resizeWait = resized;
      assert(resized.satisfied, '窗口未达到请求的尺寸预设。');
      await wait({
        kind: 'display',
        width: Math.round(resized.lastState.logicalWidth),
        height: Math.round(resized.lastState.logicalHeight),
        devicePixelRatio: resized.lastState.scaleFactor,
        afterRevision: null,
      });
    }
    record.stage = 'placement';
    record.center = (await tool('dev_window_action', args({ kind: 'center' }, true))).value;
    record.placementWait = (
      await tool(
        'dev_window_wait',
        args({
          condition: { kind: 'withinWorkArea' },
          afterSequence: null,
          timeoutMs: 10000,
        }),
      )
    ).value;
    assert(record.placementWait.satisfied, '客户区无法完整放入当前屏幕工作区。');
    record.stage = 'display';
    record.display = await captureDisplayEnvironment(context);
    record.issues = displayIssues(record.display);
    assert.deepEqual(record.issues, [], '原生／前端显示条件不一致。');
    const actualDpr = record.display.ui.window.devicePixelRatio;
    assert(
      expected.dpr === null || Math.abs(actualDpr - expected.dpr) < 0.000001,
      '实际 DPR 与预定值不符。',
    );
    record.passed = true;
    record.stage = 'complete';
    report.checks.push('加载样本前输入桌面、系统前台、原生／前端尺寸及预定 DPR 通过');
    return record;
  } catch (error) {
    record.error = String(error);
    // 失败时保留观测，不再次激活或改变环境来掩盖首次结果。
    record.terminal = await captureDisplayEnvironment({ state, tool, args });
    throw error;
  }
}
