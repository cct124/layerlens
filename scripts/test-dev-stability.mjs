// 每轮并行一个业务实例和一个 WebView 实例；轮间重新启动，不重试失败样本。
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { mkdir, mkdtemp, readdir, readFile, rename, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { redact, summarize } from './dev-mcp-artifacts.mjs';
import { isolatedEnvironment, runProcess } from './dev-mcp-runner.mjs';
import { windowsPowerShellEnvironment } from './windows-powershell.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const { values } = parseArgs({
  options: {
    environment: { type: 'string', default: '.local/dev-mcp/driver-preparation.json' },
    app: { type: 'string', default: 'target/debug/layerlens.exe' },
    'release-app': { type: 'string', default: 'target/release/layerlens.exe' },
    repeat: { type: 'string', default: '5' },
    'webview-suite': { type: 'string', default: 'baseline' },
  },
});
assert.equal(process.platform, 'win32', '稳定性测试当前只支持 Windows。');
assert.match(values.repeat, /^(?:[2-9]|1[0-9]|20)$/, '重复次数为 2–20。');
const repeats = Number(values.repeat);
assert(['baseline', 'm2', 'm2-input', 'm2-cleanup'].includes(values['webview-suite']));
const scenarios =
  values['webview-suite'] === 'm2'
    ? ['baseline', 'geometry-25', 'geometry-100', 'geometry-200', 'cleanup']
    : values['webview-suite'] === 'm2-input'
      ? ['input-pan', 'input-chord', 'input-snapping']
      : values['webview-suite'] === 'm2-cleanup'
        ? ['cleanup-max', 'cleanup-expiry', 'cleanup-recovery', 'cleanup-late']
        : ['baseline'];
assert(
  repeats >= scenarios.length && repeats % scenarios.length === 0,
  '轮数必须是场景数的正整数倍，确保每个场景重复次数一致。',
);
const base = path.join(root, '.local/dev-mcp');
await mkdir(base, { recursive: true });
const directory = await mkdtemp(path.join(base, 'stability-'));
const runs = path.join(directory, 'runs');
await mkdir(runs);
const started = performance.now();
const report = {
  formatVersion: 1,
  status: 'running',
  startedAt: new Date().toISOString(),
  repeats,
  concurrency: 2,
  automaticRetries: 0,
  webviewScenarios: scenarios,
  expectedBuildModes: { development: 'debug with dev-mcp', production: 'release without dev-mcp' },
  ci: {
    runId: process.env.GITHUB_RUN_ID ?? null,
    attempt: process.env.GITHUB_RUN_ATTEMPT ?? null,
    job: process.env.GITHUB_JOB ?? null,
    imageOS: process.env.ImageOS ?? null,
    imageVersion: process.env.ImageVersion ?? null,
  },
  results: [],
};
const expected = 1 + 2 * repeats;
let environment, childEnv, appHash, releaseHash;
async function hash(file) {
  const h = createHash('sha256');
  for await (const chunk of createReadStream(file)) h.update(chunk);
  return h.digest('hex');
}
async function checkpoint() {
  report.summary = summarize(report.results, expected);
  report.scenarioSummary = Object.fromEntries(
    scenarios.map((scenario) => {
      const runs = report.results.filter((result) => result.scenario === scenario);
      return [
        scenario,
        {
          expected: repeats / scenarios.length,
          completed: runs.length,
          passed: runs.filter((result) => result.passed).length,
        },
      ];
    }),
  );
  if (report.status !== 'passed') report.summary.passed = false;
  report.durationMs = Math.round(performance.now() - started);
  const temporary = path.join(directory, 'summary.tmp');
  await writeFile(temporary, redact(JSON.stringify(report, null, 2)));
  await rename(temporary, path.join(directory, 'summary.json'));
}
async function execute(kind, iteration) {
  const prefix = kind === 'webview' ? 'webview-' : 'desktop-';
  const previous = new Set(await readdir(runs));
  const script = kind === 'webview' ? 'test-dev-webview.mjs' : 'test-dev-mcp.mjs';
  const args = [
    path.join(root, 'scripts', script),
    '--output-root',
    runs,
    '--app',
    path.resolve(root, kind === 'disabled' ? values['release-app'] : values.app),
  ];
  if (kind === 'disabled') args.push('--expect-disabled');
  const scenario = kind === 'webview' ? scenarios[(iteration - 1) % scenarios.length] : null;
  if (kind === 'webview') args.push('--driver', environment.driverPath, '--scenario', scenario);
  const outcome = await runProcess(process.execPath, args, { cwd: root, env: childEnv });
  const result = {
    kind,
    iteration,
    scenario,
    passed: false,
    code: outcome.code,
    timedOut: outcome.timedOut,
    durationMs: outcome.durationMs,
    processChecks: [],
  };
  const created = (await readdir(runs)).filter(
    (name) => name.startsWith(prefix) && !previous.has(name),
  );
  try {
    assert.equal(created.length, 1, '场景必须生成一份独立报告目录。');
    const run = path.join(runs, created[0]);
    result.report = path
      .relative(directory, path.join(run, 'report.json'))
      .split(path.sep)
      .join('/');
    await writeFile(path.join(run, 'runner.log'), outcome.output, { flag: 'wx' });
    const data = JSON.parse(await readFile(path.join(run, 'report.json'), 'utf8'));
    result.phase = data.phase ?? (data.passed ? 'complete' : 'business');
    result.instanceId = data.instanceId;
    result.scenarioDurationMs = data.durationMs;
    result.checks = data.checks;
    if (data.error) result.error = data.error;
    assert.equal(outcome.timedOut, false, '场景子进程超时，不能计为稳定通过。');
    assert.equal(outcome.code, 0, outcome.spawnError ?? '场景进程返回失败。');
    assert.equal(data.passed, true, '场景断言未全部通过。');
    assert.equal(data.forcedExit, undefined, '发生强制退出。');
    assert.equal(data.appSha256, kind === 'disabled' ? releaseHash : appHash, '轮间程序指纹变化。');
    if (kind !== 'disabled') assert.match(data.instanceId, /^[a-f0-9-]{36}$/);
    if (kind === 'webview') {
      assert.equal(data.scenario, scenario, 'WebView 报告必须对应本轮指定场景。');
      assert.equal(
        data.driver.browserVersion,
        environment.runtimeVersion,
        '实际 WebView 版本漂移。',
      );
      assert.equal(data.driverSha256, environment.driverSha256, '实际驱动指纹漂移。');
    }
    result.passed = true;
  } catch (error) {
    result.error = redact(result.error ?? error.stack ?? String(error));
    if (!created.length) result.diagnostic = outcome.output;
  }
  // 包括异常路径；先等待正常释放，发现残留即失败，再清理确属本轮的进程。
  for (const name of created) {
    const checked = await runProcess(
      'powershell',
      [
        '-NoProfile',
        '-ExecutionPolicy',
        'Bypass',
        '-File',
        path.join(root, 'scripts/dev-mcp-processes.ps1'),
        '-RunRoot',
        path.join(runs, name),
        '-Terminate',
      ],
      { cwd: root, env: windowsPowerShellEnvironment(), timeoutMs: 15000 },
    );
    try {
      assert.equal(checked.code, 0, checked.output);
      const audit = JSON.parse(checked.output.replace(/^\uFEFF/, ''));
      result.processChecks.push(audit);
      assert.equal(audit.remaining.length, 0, '正常退出期限后仍有本轮进程，已进入失败清理。');
    } catch (error) {
      result.passed = false;
      result.processError = String(error);
    }
  }
  console.log(
    JSON.stringify({
      kind,
      iteration,
      passed: result.passed,
      phase: result.phase,
      durationMs: result.durationMs,
    }),
  );
  return result;
}
await checkpoint();
try {
  environment = JSON.parse(await readFile(path.resolve(root, values.environment), 'utf8'));
  assert.equal(environment.passed, true, '驱动环境准备未成功。');
  assert.match(environment.runtimeVersion, /^\d+\.\d+\.\d+\.\d+$/);
  assert.equal(
    await hash(environment.driverPath),
    environment.driverSha256,
    '驱动文件在准备后发生变化。',
  );
  assert.equal(
    await hash(path.join(environment.runtimePath, 'msedgewebview2.exe')),
    environment.runtimeSha256,
    '运行时在准备后发生变化，请重新准备。',
  );
  appHash = await hash(path.resolve(root, values.app));
  releaseHash = await hash(path.resolve(root, values['release-app']));
  await writeFile(path.join(directory, 'environment.json'), JSON.stringify(environment, null, 2), {
    flag: 'wx',
  });
  // 仅传给本套测试子进程，固定本轮运行时；不修改机器或用户环境变量。
  childEnv = isolatedEnvironment(environment.runtimePath);
  report.results.push(await execute('disabled', 1));
  await checkpoint();
  if (report.results[0].processError) {
    throw new Error('普通构建检查出现进程清理异常，停止开发场景。');
  }
  for (let iteration = 1; iteration <= repeats; iteration++) {
    report.results.push(
      ...(await Promise.all([execute('business', iteration), execute('webview', iteration)])),
    );
    await checkpoint();
    if (report.results.some((result) => result.processError)) {
      throw new Error('存在进程清理异常，停止后续轮次以免污染隔离证据。');
    }
  }
  assert(
    summarize(report.results, expected).passed,
    '有失败场景、缺少结果或实例 ID 重复；不自动重试。',
  );
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = redact(error.stack ?? String(error));
  process.exitCode = 1;
} finally {
  await checkpoint();
  const lines = [
    '# 开发 MCP 稳定性报告',
    '',
    '状态：' + report.status + '；自动重试：0。',
    'WebView 场景：' + scenarios.join('、') + '。',
    '',
    '| 场景 | 次数 | 通过 | 失败 | 最小 / 中位 / 最大 ms |',
    '| --- | --- | --- | --- | --- |',
  ];
  for (const [kind, data] of Object.entries(report.summary.groups))
    lines.push(
      '| ' +
        [
          kind,
          data.runs,
          data.passed,
          data.failed,
          [data.minMs, data.medianMs, data.maxMs].join(' / '),
        ].join(' | ') +
        ' |',
    );
  lines.push('', '| WebView 场景 | 预期次数 | 完成 | 通过 |', '| --- | --- | --- | --- |');
  for (const [scenario, data] of Object.entries(report.scenarioSummary))
    lines.push('| ' + [scenario, data.expected, data.completed, data.passed].join(' | ') + ' |');
  lines.push(
    '',
    '每轮业务与 WebView 各一个独立实例并行，轮次之间重新启动。耗时包含启动与测试，不是产品性能或提速结论。',
    '',
    '完整操作、失败阶段、版本、指纹和截图见同目录的逐次报告。',
  );
  await writeFile(path.join(directory, 'summary.md'), lines.join('\n') + '\n', { flag: 'wx' });
  console.log(
    JSON.stringify(
      {
        status: report.status,
        summary: report.summary,
        report: path.join(directory, 'summary.json'),
      },
      null,
      2,
    ),
  );
}
