import test from 'node:test';
import assert from 'node:assert/strict';
import { classifyTopInput } from './dev-mcp-top-input.mjs';
import { assertResizeInput } from './dev-mcp-window-resize.mjs';

test('顶部客户区输入与原生命中分开验收，拒绝未完成、外部目标及其他边缘的回退', () => {
  const point = { x: 900, y: 120 };
  const candidate = { point, target: 'mainChild', hit: 1, parentHit: 12 };
  const probe = {
    complete: true,
    inputCompleted: true,
    edge: 'top',
    candidates: [candidate],
    clientTop: candidate,
    matched: null,
    inputRoute: 'clientTop',
    expectedHit: 12,
  };
  const validate = (changes = {}, edge = 'top') =>
    assertResizeInput({ status: 'available', probe: { ...probe, ...changes } }, edge);
  validate();
  assert.throws(() => validate({ inputCompleted: false }));
  assert.throws(() => validate({ edge: 'left' }, 'left'));
  assert.throws(() => validate({ clientTop: { ...candidate, target: 'other' } }));
  assert.throws(() => validate({ matched: point }));
  assert.throws(() => validate({ inputRoute: 'nativeBorder', clientTop: null, matched: point }));
  validate({
    inputRoute: 'nativeBorder',
    clientTop: null,
    matched: point,
    candidates: [{ ...candidate, hit: 12 }],
  });
});

test('顶部输入分类要求客户区真实增长且对边固定，不把窗口移动当作缩放', () => {
  const before = {
    x: -1000,
    y: 120,
    width: 1200,
    height: 800,
    clientWidth: 1184,
    clientHeight: 792,
  };
  assert.equal(
    classifyTopInput(before, { ...before, y: 72, height: 848, clientHeight: 840 }, 48),
    'resized',
  );
  assert.equal(classifyTopInput(before, { ...before, y: 72 }, 48), 'moved');
  assert.equal(classifyTopInput(before, before, 48), 'unchanged');
  assert.equal(
    classifyTopInput(before, { ...before, y: 72, height: 848 }, 48),
    'otherGeometryChange',
  );
  assert.equal(
    classifyTopInput(before, { ...before, height: 848, clientHeight: 840 }, 48),
    'otherGeometryChange',
  );
});
