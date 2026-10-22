import assert from 'node:assert/strict';
import test from 'node:test';
import { setImmediate } from 'node:timers/promises';
import { exerciseWorkspaceMemory } from './dev-mcp-workspace-memory.mjs';

test('循环门槛失败仍完成固定空闲观测，辅助读数不能覆盖失败或触发重试', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  let now = 0,
    samples = 0;
  t.mock.method(performance, 'now', () => now);
  const changes = [],
    conditions = [];
  const report = { buildProfile: 'release', processId: 10, checks: [] };
  const context = {
    report,
    args: (value) => value,
    workspace: async (action) => {
      changes.push(action);
      return { sequence: '2' };
    },
    waitCore: async () => ({
      documents: [{ id: '1' }],
      resources: { sourceBytes: '0', decodedBytes: '0', outputBytes: '0', cacheBytes: '0' },
    }),
    wait: async (condition) => {
      conditions.push(condition);
      return { canvas: null, previewResources: {} };
    },
    state: async () => ({ previewResources: {} }),
    checkpoint: async () => {},
    tool: async () => ({
      value: {
        rootProcessId: 10,
        errors: [],
        observations: [],
        processes: [
          {
            processId: 10,
            role: 'application',
            privateBytes: 10 * 1024 ** 2,
            workingSetBytes: 20 * 1024 ** 2,
          },
          {
            processId: 11,
            role: 'webview',
            webviewKind: 'renderer',
            privateBytes: (100 + Math.floor(samples++ / 2) * 3) * 1024 ** 2,
            workingSetBytes: 200 * 1024 ** 2,
          },
        ],
      },
    }),
  };
  let failure;
  const pending = exerciseWorkspaceMemory(
    context,
    async () => ({ previewResources: {} }),
    async () => {},
  ).catch((error) => {
    failure = error;
  });
  await setImmediate();
  for (const ms of [10000, 20000, 30000]) {
    now += ms;
    t.mock.timers.tick(ms);
    await setImmediate();
  }
  await pending;
  assert(failure instanceof assert.AssertionError);
  assert.equal(report.memoryComplete, true);
  assert.equal(report.memoryGate.passed, false);
  assert.equal(report.memory.length, 40);
  assert.equal(report.memoryCycleTimings.length, 20);
  assert.deepEqual(
    report.memoryIdle.map((s) => s.label),
    ['idle-0', 'idle-10', 'idle-30', 'idle-60'],
  );
  assert.equal(changes.filter((a) => a.kind === 'open').length, 19);
  assert.equal(changes.filter((a) => a.kind === 'close').length, 20);
  assert.equal(conditions.filter((c) => c.kind === 'previewReleased').length, 20);
});

test('debug 实例在任何资源工作负载之前拒绝', async () => {
  await assert.rejects(exerciseWorkspaceMemory({ report: { buildProfile: 'debug' } }), /release/);
});
