import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import test from 'node:test';
import { errorDiagnostic, observeProcess } from './dev-mcp-process-monitor.mjs';

test('连接错误保留底层错误码，拒绝展开凭据并限制循环及聚合深度', () => {
  const socket = Object.assign(new Error('connection refused'), {
    code: 'ECONNREFUSED',
    token: 'secret',
  });
  const error = new TypeError('fetch failed', { cause: socket });
  socket.cause = error;
  const result = errorDiagnostic(error);
  assert.equal(result.cause.code, 'ECONNREFUSED');
  assert.deepEqual(result.cause.cause, { truncated: true });
  assert(!JSON.stringify(result).includes('secret'));
  const aggregate = errorDiagnostic(
    new AggregateError(Array.from({ length: 8 }, () => new Error('x'))),
  );
  assert.equal(aggregate.errors.length, 4);
  assert.equal(aggregate.errorsTruncated, true);
});

test('拒绝将非整数或无效 PID 传入系统命令', () => {
  for (const pid of [0, -1, 1.5, '1; exit', Number.MAX_SAFE_INTEGER + 1])
    assert.throws(() => observeProcess(pid));
});

test(
  '应用在失败收尾前退出时仍保留实际非零退出码',
  { skip: process.platform !== 'win32', timeout: 15000 },
  async (t) => {
    const application = spawn(
      process.execPath,
      ['-e', "process.stdin.once('data',()=>process.exit(7));process.stdout.write('ready')"],
      {
        windowsHide: true,
        stdio: ['pipe', 'pipe', 'pipe'],
      },
    );
    t.after(() => {
      if (application.exitCode === null) application.kill();
    });
    await once(application.stdout, 'data');
    const monitor = observeProcess(application.pid);
    t.after(() => {
      if (monitor.child.exitCode === null) monitor.child.kill();
    });
    await monitor.ready;
    assert.equal(monitor.snapshot().status, 'watching');
    application.stdin.end('exit');
    assert.equal(await monitor.exit, 7);
    const observed = monitor.snapshot();
    assert.equal(observed.status, 'exited');
    assert.equal(observed.exitCode, 7);
    assert(Number.isFinite(Date.parse(observed.at)));
  },
);
