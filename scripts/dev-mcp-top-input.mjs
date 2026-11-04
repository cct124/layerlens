// 顶部真实输入诊断。输入完成与缩放功能通过分别记录，不把客户区命中判作产品结论。
import assert from 'node:assert/strict';

export function classifyTopInput(before, after, distance) {
  const near = (actual, expected) => Math.abs(actual - expected) <= 2;
  if (
    near(after.x, before.x) &&
    near(after.width, before.width) &&
    near(after.y, before.y - distance) &&
    near(after.height, before.height + distance) &&
    near(after.clientWidth, before.clientWidth) &&
    near(after.clientHeight, before.clientHeight + distance)
  )
    return 'resized';
  if (
    near(after.width, before.width) &&
    near(after.height, before.height) &&
    (!near(after.x, before.x) || !near(after.y, before.y))
  )
    return 'moved';
  if (
    ['x', 'y', 'width', 'height', 'clientWidth', 'clientHeight'].every((k) =>
      near(after[k], before[k]),
    )
  )
    return 'unchanged';
  return 'otherGeometryChange';
}

export async function diagnoseTopResize(context) {
  const { native, nativeWait, nativeState, layout, report } = context;
  await native({ kind: 'restore' });
  await native({ kind: 'resize', size: 'default' });
  await nativeWait({ kind: 'size', size: 'default' });
  await native({ kind: 'center' });
  await nativeWait({ kind: 'withinWorkArea' });
  await nativeWait({ kind: 'foreground', target: 'main' });
  const before = await nativeState();
  await layout(before, 'top-input-before');
  report.phase = 'window-top-input-dispatch';
  await native({ kind: 'diagnoseTopResize' });
  const after = await nativeState();
  report.topInputDiagnostic = after.nativeTopResize;
  assert.equal(after.nativeTopResize?.status, 'available');
  const diagnosis = after.nativeTopResize.diagnosis;
  assert.equal(diagnosis?.complete, true, '诊断必须实际完成并释放输入');
  assert.equal(diagnosis.error, null);
  assert.deepEqual(
    diagnosis.samples.map((s) => s.stage),
    ['prepared', 'pointerReady', 'pressed', 'systemMode', 'moved', 'released'],
  );
  const ready = diagnosis.samples[1];
  assert.deepEqual(ready.pointer, diagnosis.start);
  assert(['main', 'mainChild'].includes(ready.hit?.target));
  assert.equal(diagnosis.samples[2].leftButtonDown, true);
  const released = diagnosis.samples.at(-1);
  assert.equal(released.leftButtonDown, false);
  assert.equal(released.inMoveSize, false);
  assert.equal(released.mouseCapture, 'none');
  assert.equal(released.foreground, true);
  const distance = Math.round(48 * before.scaleFactor);
  report.topInputOutcome = classifyTopInput(
    diagnosis.samples[0].geometry,
    released.geometry,
    distance,
  );
  report.topInputAcceptance = 'diagnostic-only';
  await layout(after, 'top-input-after');
  report.checks.push(
    '顶部真实鼠标诊断完成，命中／系统模式／实际几何分别保留；不据诊断完成宣称缩放验收通过',
  );
}
