// 显示环境经 MCP 采样；观测窗口之外不宣称连续屏幕监控，也不自动恢复／重试失败。
import assert from 'node:assert/strict';

const equal = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const counter = (v) => typeof v === 'string' && /^(0|[1-9][0-9]*)$/.test(v);
const positive = (v) => Number.isFinite(v) && v > 0;

/** 两次原生观测夹住前端投影；原生变化时保留全部样本并拒绝当作稳定同一环境。 */
export async function captureDisplayEnvironment({ state, tool, args }) {
  const result = {
    capturedAt: new Date().toISOString(),
    nativeBefore: null,
    ui: null,
    nativeAfter: null,
    errors: [],
  };
  for (const [name, read] of [
    ['nativeBefore', async () => (await tool('dev_window_state', args({}))).value],
    [
      'ui',
      async () => {
        const view = await state();
        return structuredClone({
          instanceId: view.instanceId,
          frontendId: view.frontendId,
          documentEpoch: view.documentEpoch,
          viewRevision: view.viewRevision,
          coreSequence: view.coreSequence,
          activeDocument: view.activeDocument,
          window: view.window,
          render: view.renderLab
            ? {
                generation: view.renderLab.generation,
                phase: view.renderLab.phase,
                sourceSha256: view.renderLab.sourceSha256,
                fontSha256: view.renderLab.fontSha256,
                textMode: view.renderLab.textMode,
                surface: view.renderLab.surface,
              }
            : {
                generation: view.canvas?.sceneRevision ?? '0',
                paintedRevision: view.canvas?.paintedRevision ?? '0',
                phase: view.canvas ? (view.canvas.paintPrepared ? 'ready' : 'loading') : 'released',
                sourceSha256: null,
                fontSha256: null,
                textMode: 'savedComposite',
                surface: view.canvas?.surface ?? null,
              },
        });
      },
    ],
    ['nativeAfter', async () => (await tool('dev_window_state', args({}))).value],
  ]) {
    try {
      result[name] = await read();
    } catch (error) {
      result.errors.push({ source: name, error: String(error) });
    }
  }
  return result;
}

/** 跨独立进程只比较显示条件，不比较进程身份、位置或单进程事件计数。 */
export function displaySignature(snapshot) {
  const w = snapshot.ui.window,
    n = snapshot.nativeAfter;
  return {
    width: w.width,
    height: w.height,
    dpr: w.devicePixelRatio,
    viewport: w.canvasViewport
      ? { width: w.canvasViewport.width, height: w.canvasViewport.height }
      : null,
    physicalWidth: n.physicalWidth,
    physicalHeight: n.physicalHeight,
    scaleFactor: n.scaleFactor,
    foreground: n.foregroundReady ?? 'unavailable',
  };
}

export function displayIssues(current, baseline = null, foregroundTarget = 'main') {
  assert(['main', 'auxiliary'].includes(foregroundTarget));
  const issues = [];
  const ui = current.ui,
    w = ui?.window,
    n = current.nativeAfter,
    before = current.nativeBefore;
  if (current.errors.length || !w || !n || !before) return ['environment-unavailable'];
  if (!equal(before, n)) issues.push('native-changed-during-capture');
  if (!w.visible || !n.visible || n.minimized) issues.push('window-not-visible');
  if (before.clientWithinWorkArea !== true || n.clientWithinWorkArea !== true)
    issues.push('client-outside-work-area');
  const focused = foregroundTarget === 'main';
  if (w.focused !== focused || n.focused !== focused || n.auxiliaryFocused !== !focused)
    issues.push('window-not-focused');
  if (n.foregroundReady !== foregroundTarget || before.foregroundReady !== foregroundTarget)
    issues.push('native-foreground-not-ready');
  if (
    ![
      w.width,
      w.height,
      w.devicePixelRatio,
      n.scaleFactor,
      n.physicalWidth,
      n.physicalHeight,
    ].every(positive) ||
    (ui.render?.phase !== 'released' &&
      (!w.canvasViewport || ![w.canvasViewport.width, w.canvasViewport.height].every(positive))) ||
    ![
      w.displayRevision,
      w.blurEvents,
      w.focusEvents,
      n.eventSequence,
      n.blurEvents,
      n.focusEvents,
      n.resizeEvents,
      n.moveEvents,
    ].every(counter)
  )
    issues.push('invalid-display-observation');
  if (
    Math.abs(w.devicePixelRatio - n.scaleFactor) > 0.000001 ||
    Math.abs(w.width * w.devicePixelRatio - n.physicalWidth) > 1 ||
    Math.abs(w.height * w.devicePixelRatio - n.physicalHeight) > 1
  )
    issues.push('frontend-native-mismatch');
  const render = ui.render,
    surface = render?.surface;
  if (render?.phase === 'ready' && !surface) issues.push('surface-missing');
  if (
    surface &&
    (![
      surface.width,
      surface.height,
      surface.cssWidth,
      surface.cssHeight,
      surface.configuredDpr,
    ].every(positive) ||
      Math.abs(surface.configuredDpr - w.devicePixelRatio) > 0.000001 ||
      Math.abs(surface.width - surface.cssWidth * w.devicePixelRatio) > 1 ||
      Math.abs(surface.height - surface.cssHeight * w.devicePixelRatio) > 1)
  )
    issues.push('surface-dpr-mismatch');
  if (baseline?.ui && baseline.nativeAfter) {
    if (
      !equal(
        [ui.instanceId, ui.frontendId, ui.documentEpoch],
        [baseline.ui.instanceId, baseline.ui.frontendId, baseline.ui.documentEpoch],
      )
    )
      issues.push('frontend-identity-changed');
    if (!equal(displaySignature(current), displaySignature(baseline)))
      issues.push('display-conditions-changed');
    if (w.displayRevision !== baseline.ui.window.displayRevision)
      issues.push('display-history-changed');
    if (
      !equal(
        [w.focusEvents, w.blurEvents, n.focusEvents, n.blurEvents],
        [
          baseline.ui.window.focusEvents,
          baseline.ui.window.blurEvents,
          baseline.nativeAfter.focusEvents,
          baseline.nativeAfter.blurEvents,
        ],
      )
    )
      issues.push('focus-history-changed');
    if (
      !equal(
        [n.resizeEvents, n.moveEvents, n.x, n.y],
        [
          baseline.nativeAfter.resizeEvents,
          baseline.nativeAfter.moveEvents,
          baseline.nativeAfter.x,
          baseline.nativeAfter.y,
        ],
      )
    )
      issues.push('native-layout-history-changed');
  }
  return issues;
}

export function createDisplayGuard(context, report) {
  const record = (report.displayEnvironment = {
    formatVersion: 1,
    baseline: null,
    samples: [],
    terminal: null,
    valid: true,
    limits: '环境观察在测量区间外；事件计数可检出观测到的中途变化，不代表连续物理屏幕采样。',
  });
  async function sample(label, cycle = null, { terminal = false } = {}) {
    assert(record.samples.length < 512, '显示环境记录超过固定 512 项');
    const snapshot = await captureDisplayEnvironment(context);
    const issues = displayIssues(snapshot, record.baseline);
    const entry = { label, cycle, ...snapshot, issues, valid: issues.length === 0 };
    record.samples.push(entry);
    if (terminal) record.terminal = entry;
    record.valid &&= entry.valid;
    if (!record.baseline && entry.valid) record.baseline = structuredClone(snapshot);
    assert(entry.valid, '显示环境门禁失败：' + issues.join(', '));
    return entry;
  }
  return { sample, finish: (outcome) => sample('terminal-' + outcome, null, { terminal: true }) };
}
