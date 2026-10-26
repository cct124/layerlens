// 通过原生窗口 MCP 制造可恢复的真实失焦／尺寸变化，验证观测和门禁，不写页面状态。
import assert from 'node:assert/strict';
import { captureDisplayEnvironment, displayIssues } from './dev-mcp-render-environment.mjs';
import { ensureRenderFocus } from './dev-mcp-render-focus.mjs';

export async function exerciseDisplayEnvironment(context) {
  const {
    state,
    tool,
    args,
    wait,
    core,
    waitCore,
    workspace,
    report,
    display,
    sample,
    screenshot,
    checkpoint,
    click,
  } = context;
  report.phase = 'display-environment';
  const evidence = (report.environmentChecks = []);
  const resize = async (size) => {
    const before = await state();
    await tool('dev_window_action', args({ kind: 'resize', size }, true));
    const result = (
      await tool(
        'dev_window_wait',
        args({
          condition: { kind: 'size', size },
          afterSequence: null,
          timeoutMs: 10000,
        }),
      )
    ).value;
    assert(result.satisfied);
    return wait({
      kind: 'display',
      width: Math.round(result.lastState.logicalWidth),
      height: Math.round(result.lastState.logicalHeight),
      devicePixelRatio: result.lastState.scaleFactor,
      afterRevision:
        before.window.width === Math.round(result.lastState.logicalWidth) &&
        before.window.height === Math.round(result.lastState.logicalHeight)
          ? null
          : before.window.displayRevision,
    });
  };
  await resize('renderLab');
  // 先经真实输入建立可交互主窗口，再测原生焦点转移；不靠启动时的缓存 focus 推断。
  const beforeInput = await state();
  await click('render:clear-metrics');
  await wait({
    kind: 'canvasIdle',
    pointerEventsAtLeast: String(BigInt(beforeInput.input.pointerEvents) + 2n),
  });
  await ensureRenderFocus(context);
  const baseline = await captureDisplayEnvironment(context);
  assert.deepEqual(displayIssues(baseline), []);
  const dispatch = (await tool('dev_window_action', args({ kind: 'focusAuxiliary' }, true))).value;
  evidence.push({ stage: 'blur-dispatch', dispatch });
  const nativeBlur = (
    await tool(
      'dev_window_wait',
      args({
        condition: { kind: 'foreground', target: 'auxiliary' },
        afterSequence: dispatch.before.eventSequence,
        timeoutMs: 10000,
      }),
    )
  ).value;
  evidence.push({ stage: 'native-blur', result: nativeBlur });
  assert(nativeBlur.satisfied, JSON.stringify(nativeBlur));
  await wait({ kind: 'windowFocus', focused: false, eventsAfter: baseline.ui.window.blurEvents });
  const blurred = await captureDisplayEnvironment(context);
  evidence.push({ stage: 'blur', snapshot: blurred, issues: displayIssues(blurred, baseline) });
  assert(evidence.at(-1).issues.includes('window-not-focused'));
  await ensureRenderFocus(context);
  const restored = await captureDisplayEnvironment(context);
  evidence.push({
    stage: 'focus-restored',
    snapshot: restored,
    issues: displayIssues(restored, baseline),
  });
  assert(evidence.at(-1).issues.includes('focus-history-changed'));
  await resize('minimum');
  const resized = await captureDisplayEnvironment(context);
  evidence.push({ stage: 'resize', snapshot: resized, issues: displayIssues(resized, restored) });
  assert(evidence.at(-1).issues.includes('display-conditions-changed'));
  await resize('renderLab');
  const recovered = await captureDisplayEnvironment(context);
  evidence.push({
    stage: 'size-restored',
    snapshot: recovered,
    issues: displayIssues(recovered, restored),
  });
  assert(evidence.at(-1).issues.includes('display-history-changed'));
  assert.deepEqual(displayIssues(recovered), []);
  // 上述为明确的负向测试；固定循环从恢复后的环境开始，不将中途漂移重置为成功。
  await display.sample('measurement-start');
  for (let cycle = 0; cycle < 3; cycle++) {
    await display.sample('cycle-start', cycle);
    if (cycle) {
      await workspace({ kind: 'open', path: sample });
      await waitCore({ kind: 'documentCount', count: 1 });
    }
    const doc = (await core()).documents[0];
    await wait({ kind: 'imageReady', documentId: doc.id, revision: doc.revision });
    await wait({ kind: 'renderer', engine: 'konva', phase: 'ready', generationAtLeast: '0' });
    await display.sample('ready', cycle);
    if (cycle === 2) await screenshot('final-state.png', true);
    await workspace({ kind: 'close', documentId: doc.id });
    await waitCore({ kind: 'documentCount', count: 0 });
    await wait({ kind: 'renderer', engine: 'konva', phase: 'released', generationAtLeast: '0' });
    await display.sample('released', cycle);
    await checkpoint();
  }
  report.checks.push(
    '真实失焦／恢复及窗口尺寸变化／恢复均被识别，三轮加载／释放保留显示条件与 Canvas 像素证据',
  );
}
