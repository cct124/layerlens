import test from 'node:test';
import assert from 'node:assert/strict';
import { classifyTopInput } from './dev-mcp-top-input.mjs';

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
