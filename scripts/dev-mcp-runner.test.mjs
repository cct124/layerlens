import assert from 'node:assert/strict';
import { mkdir, mkdtemp, readFile, readdir, rm, symlink, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { collectArtifacts, redact, summarize } from './dev-mcp-artifacts.mjs';
import { checkSelectionObservation, isolatedEnvironment, runProcess } from './dev-mcp-runner.mjs';

test('操作成功时附带观测可忙碌降级，但跨会话、缺失与其他错误仍失败', () => {
  assert.equal(checkSelectionObservation({ sessionId: 'session' }, 'session'), null);
  const unavailable = { code: 'BUSY', message: '选区与清理后台已满' };
  assert.deepEqual(checkSelectionObservation({ unavailable }, 'session'), unavailable);
  for (const invalid of [
    undefined,
    {},
    { sessionId: 'other' },
    { unavailable: { code: 'INTERNAL', message: 'failure' } },
    { unavailable, sessionId: 'session' },
  ])
    assert.throws(() => checkSelectionObservation(invalid, 'session'));
});

test('测试子进程只使用指定运行时，不继承日常 WebView profile／调试覆盖项', () => {
  const parent = {
    PATH: 'unchanged',
    WEBVIEW2_USER_DATA_FOLDER: 'daily-profile',
    webview2_additional_browser_arguments: '--remote-debugging-port=9999',
  };
  const isolated = isolatedEnvironment('runtime-folder', parent);
  assert.deepEqual(isolated, {
    PATH: 'unchanged',
    WEBVIEW2_BROWSER_EXECUTABLE_FOLDER: 'runtime-folder',
  });
  assert.equal(parent.WEBVIEW2_USER_DATA_FOLDER, 'daily-profile');
});

test('任何一轮失败、缺少结果或重复实例均不能由后续成功掩盖', () => {
  const cases = [
    { kind: 'business', passed: false, instanceId: 'a', durationMs: 30 },
    { kind: 'business', passed: true, instanceId: 'b', durationMs: 10 },
    { kind: 'webview', passed: true, instanceId: 'c', durationMs: 20 },
  ];
  const result = summarize(cases, 3);
  assert.equal(result.passed, false);
  assert.deepEqual(result.groups.business, {
    runs: 2,
    passed: 1,
    failed: 1,
    minMs: 10,
    medianMs: 20,
    maxMs: 30,
  });
  cases[0].passed = true;
  assert.equal(summarize(cases, 3).passed, true);
  assert.equal(summarize(cases, 4).passed, false);
  cases[2].instanceId = 'a';
  assert.equal(summarize(cases, 3).passed, false);
});

test('报告保留失败信息与指纹，并删除凭据字段／Bearer／已知实例令牌', () => {
  const source = JSON.stringify({
    error: 'failed',
    token: 'secret1',
    Authorization: 'Bearer secret2',
    nested: { password: 'secret3', message: 'known-secret' },
    sha256: 'a'.repeat(64),
  });
  const output = redact(source, ['known-secret']);
  assert(!output.includes('secret'));
  assert.equal(JSON.parse(output).error, 'failed');
  assert.equal(JSON.parse(output).sha256, 'a'.repeat(64));
});

test('只保留白名单报告与截图，连接凭据、profile 和下载程序不会被收集', async (t) => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'layerlens-artifacts-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const source = path.join(directory, 'source');
  const run = path.join(source, 'stability-abc', 'runs', 'webview-test');
  const profile = path.join(run, 'instances', 'instance', 'webview');
  await mkdir(profile, { recursive: true });
  await writeFile(path.join(profile, 'private-data'), 'DO NOT UPLOAD');
  await writeFile(
    path.join(path.dirname(profile), 'connection.json'),
    JSON.stringify({ token: 'credential' }),
  );
  await writeFile(path.join(run, 'config.json'), 'DO NOT UPLOAD');
  await writeFile(path.join(run, 'msedgedriver.exe'), 'DO NOT UPLOAD');
  await writeFile(
    path.join(run, 'report.json'),
    JSON.stringify({ passed: false, error: 'expected failure', token: 'credential' }),
  );
  await writeFile(path.join(run, 'driver.log'), 'Authorization: Bearer credential');
  await writeFile(path.join(run, 'failure.png'), Buffer.from([137, 80, 78, 71]));
  await writeFile(path.join(run, 'window-minimum.png'), Buffer.from([137, 80, 78, 71]));
  await writeFile(
    path.join(source, 'stability-abc', 'summary.json'),
    JSON.stringify({ status: 'failed' }),
  );
  const output = path.join(directory, 'output');
  const manifest = await collectArtifacts(source, output);
  assert.deepEqual(manifest.errors, []);
  assert.equal(manifest.files.length, 5);
  assert(manifest.files.some((entry) => entry.path.endsWith('/window-minimum.png')));
  assert(manifest.files.every((entry) => !/instances|config|exe/.test(entry.path)));
  const copy = JSON.parse(
    await readFile(path.join(output, 'stability-abc/runs/webview-test/report.json'), 'utf8'),
  );
  assert.equal(copy.passed, false);
  assert.equal(copy.token, '[REDACTED]');
  assert.equal(
    (await readFile(path.join(output, 'stability-abc/runs/webview-test/failure.png'))).length,
    4,
  );
  await assert.rejects(collectArtifacts(source, output), { code: 'EEXIST' });
  const single = await collectArtifacts(
    path.join(source, 'stability-abc'),
    path.join(directory, 'single'),
  );
  assert.deepEqual(single.errors, []);
  assert.equal(single.files.length, 5);
});

test('来源目录联接不能导出外部文件，空产物明确报错', async (t) => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'layerlens-artifact-link-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const source = path.join(directory, 'source'),
    external = path.join(directory, 'external');
  await mkdir(source);
  await mkdir(external);
  await writeFile(path.join(external, 'report.json'), 'private');
  await symlink(
    external,
    path.join(source, 'webview-link'),
    process.platform === 'win32' ? 'junction' : 'dir',
  );
  const output = path.join(directory, 'output');
  const manifest = await collectArtifacts(source, output);
  assert(manifest.errors.length > 0);
  assert.equal(manifest.files.length, 0);
  assert.deepEqual(await readdir(output), ['manifest.json']);
});

test('子进程失败和启动失败保留非零结果及诊断', async () => {
  const failed = await runProcess(process.execPath, [
    '-e',
    'console.error("fixture failure");process.exit(7)',
  ]);
  assert.equal(failed.code, 7);
  assert.match(failed.output, /fixture failure/);
  const missing = await runProcess(path.join(os.tmpdir(), 'missing-layerlens-runner.exe'), []);
  assert(missing.spawnError);
  assert.notEqual(missing.code, 0);
});

test('超量日志显式拒绝，已收集的失败报告仍可诊断', async (t) => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'layerlens-artifact-budget-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const source = path.join(directory, 'source');
  const run = path.join(source, 'desktop-budget');
  await mkdir(run, { recursive: true });
  await writeFile(path.join(run, 'report.json'), JSON.stringify({ passed: false }));
  await writeFile(path.join(run, 'process.log'), Buffer.alloc(8 * 1024 * 1024 + 1));
  const manifest = await collectArtifacts(source, path.join(directory, 'output'));
  assert.equal(manifest.errors.length, 1);
  assert.equal(manifest.files.length, 1);
  assert.equal(manifest.files[0].path, 'desktop-budget/report.json');
});

test('挂起子进程有界退出，超时保留失败标志', async () => {
  const result = await runProcess(process.execPath, ['-e', 'setInterval(()=>{},1000)'], {
    timeoutMs: 250,
  });
  assert.equal(result.timedOut, true);
  assert.notEqual(result.code, 0);
  assert(result.durationMs < 10000);
});
