// Konva 固定回归：通过真实测试 MCP 串行执行，每个子进程有独立 profile 与截止时间。
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { mkdir, mkdtemp, readdir, readFile, writeFile, rename, copyFile } from 'node:fs/promises';
import { constants } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { isolatedEnvironment, runProcess } from './dev-mcp-runner.mjs';
import { windowsPowerShellEnvironment } from './windows-powershell.mjs';
import { summarizeRenderReports } from './render-statistics.mjs';
import { redact } from './dev-mcp-artifacts.mjs';
import { checkDevPython, devPythonExecutable } from './dev-python.mjs';
import { displaySignature } from './dev-mcp-render-environment.mjs';
const root = fileURLToPath(new URL('../', import.meta.url));
const python = checkDevPython();
const { values } = parseArgs({
  options: {
    environment: { type: 'string', default: '.local/dev-mcp/driver-preparation.json' },
    app: { type: 'string', default: 'target/debug/layerlens.exe' },
    cases: { type: 'string' },
    mode: { type: 'string', default: 'shared' },
    repeat: { type: 'string', default: '3' },
    workload: { type: 'string', default: 'benchmark' },
    'keep-going': { type: 'boolean', default: false },
    'project-source': { type: 'string' },
  },
});
assert(['1', '2', '3', '6'].includes(values.repeat));
assert(['shared', 'native'].includes(values.mode));
assert(
  [
    'benchmark',
    'visual',
    'memory',
    'workspace',
    'project',
    'project-memory',
    'environment',
  ].includes(values.workload),
);
const project = values.workload.startsWith('project');
assert.equal(project, Boolean(values['project-source']), '真实大稿需要明确 --project-source');
const env = JSON.parse(await readFile(path.resolve(root, values.environment), 'utf8'));
assert(env.passed);
async function hash(file) {
  const h = createHash('sha256');
  for await (const c of createReadStream(file)) h.update(c);
  return h.digest('hex');
}
assert.equal(await hash(env.driverPath), env.driverSha256);
assert.equal(await hash(path.join(env.runtimePath, 'msedgewebview2.exe')), env.runtimeSha256);
const app = path.resolve(root, values.app),
  appHash = await hash(app);
const corpus = project
  ? path.dirname(path.resolve(root, values['project-source']))
  : path.join(root, '.local/render-corpus');
const prepared = project
  ? JSON.parse(await readFile(path.join(root, '.local/render-tab1/manifest.json'), 'utf8'))
  : null;
const manifest = project
  ? {
      cases: [
        {
          file: path.basename(values['project-source']),
          sha256: prepared.sourceSha256,
          width: prepared.width,
          height: prepared.height,
          bytes: prepared.sourceBytes,
          private: true,
        },
      ],
    }
  : JSON.parse(await readFile(path.join(corpus, 'manifest.json'), 'utf8'));
let publicCases = [];
try {
  publicCases = JSON.parse(await readFile(path.join(corpus, 'public-manifest.json'), 'utf8')).cases;
} catch (error) {
  if (error.code !== 'ENOENT') throw error;
}
const available = [...manifest.cases, ...publicCases];
if (values.workload === 'visual') {
  const acceptance = JSON.parse(
    await readFile(path.join(corpus, 'acceptance-manifest.json'), 'utf8'),
  );
  for (const item of acceptance.cases) {
    const old = available.findIndex((c) => c.file === item.file);
    if (old >= 0) available[old] = item;
    else available.push(item);
    assert.equal(await hash(path.join(corpus, item.referenceFile)), item.referenceSha256);
  }
}
const names = values.cases ? values.cases.split(',') : manifest.cases.map((c) => c.file);
const cases = names.map((name) => {
  const item = available.find((c) => c.file === name || c.file === name + '.psd');
  assert(item, '未知受控样本：' + name);
  return item;
});
for (const c of cases) assert.equal(await hash(path.join(corpus, c.file)), c.sha256);
const base = path.join(root, '.local/dev-mcp');
await mkdir(base, { recursive: true });
const directory = await mkdtemp(path.join(base, 'stability-')),
  runs = path.join(directory, 'runs');
await mkdir(runs);
await writeFile(path.join(directory, 'environment.json'), JSON.stringify(env, null, 2));
const report = {
  python,
  runnerSha256: await hash(fileURLToPath(import.meta.url)),
  displaySignatures: {},
  formatVersion: 1,
  kind: 'konva-regression',
  engine: 'konva',
  status: 'running',
  concurrency: 1,
  automaticRetries: 0,
  continueIndependentTrialsAfterFailure: values['keep-going'],
  mode: values.mode,
  workload: values.workload,
  repeats: Number(values.repeat),
  cases,
  results: [],
  startedAt: new Date().toISOString(),
  appSha256: appHash,
};
const data = [];
async function checkpoint() {
  report.statistics = summarizeRenderReports(data);
  const pending = path.join(directory, 'summary.tmp');
  await writeFile(pending, redact(JSON.stringify(report, null, 2)));
  await rename(pending, path.join(directory, 'summary.json'));
}
console.log(
  JSON.stringify({ directory, cases: cases.length, repeat: report.repeats, mode: values.mode }),
);
await checkpoint();
try {
  for (let round = 0; round < report.repeats; round++) {
    for (const c of cases) {
      const previous = new Set(await readdir(runs));
      const outcome = await runProcess(
        process.execPath,
        [
          path.join(root, 'scripts/test-dev-webview.mjs'),
          '--scenario',
          'render-benchmark',
          '--app',
          app,
          '--driver',
          env.driverPath,
          '--sample-root',
          corpus,
          '--sample',
          c.file,
          '--render-mode',
          values.mode,
          '--render-workload',
          values.workload,
          ...(project ? ['--render-large'] : []),
          '--output-root',
          runs,
        ],
        {
          cwd: root,
          env: isolatedEnvironment(env.runtimePath),
          timeoutMs:
            values.workload === 'project-memory'
              ? 900000
              : values.workload === 'memory'
                ? 300000
                : 180000,
        },
      );
      const created = (await readdir(runs)).filter(
        (n) => n.startsWith('webview-') && !previous.has(n),
      );
      const result = {
        round,
        sample: c.file,
        engine: 'konva',
        passed: false,
        timedOut: outcome.timedOut,
        code: outcome.code,
        runs: created,
        durationMs: outcome.durationMs,
        processChecks: [],
      };
      let singleReport;
      try {
        assert.equal(created.length, 1);
        const single = JSON.parse(
          await readFile(path.join(runs, created[0], 'report.json'), 'utf8'),
        );
        singleReport = single;
        result.buildProfile = single.buildProfile;
        result.instanceId = single.instanceId;
        result.acceptance = single.acceptance;
        result.displayEnvironment = single.displayEnvironment;
        assert(
          single.displayEnvironment?.valid && single.displayEnvironment.terminal?.valid,
          '缺少有效逐轮／终态显示环境',
        );
        const display = displaySignature(single.displayEnvironment.baseline);
        report.displaySignatures[c.file] ??= display;
        assert.deepEqual(
          display,
          report.displaySignatures[c.file],
          '独立进程显示条件不一致，禁止合并',
        );
        assert(
          !outcome.timedOut && outcome.code === 0 && single.passed,
          single.error ?? outcome.output,
        );
        assert.equal(single.appSha256, appHash);
        assert.equal(single.driverSha256, env.driverSha256);
        assert.equal(single.driver.browserVersion, env.runtimeVersion);
        assert.equal(single.sourceHashes[0], c.sha256);
        if (project) {
          const coverage = single.acceptance.stages.find((s) => s.stage === 'source-layers').render;
          const signature = JSON.stringify(
            Object.fromEntries(
              [
                'drawnLayerIds',
                'rasterLayerIds',
                'textRasterLayerIds',
                'unsupportedLayerIds',
                'degradedLayerIds',
                'fontSha256',
              ].map((k) => [k, coverage[k]]),
            ),
          );
          report.coverageSignature ??= signature;
          assert.equal(signature, report.coverageSignature, '各轮必须使用相同实际绘制内容与字体');
          report.fullSceneAccepted = false;
        }
        if (values.workload === 'visual') {
          assert(c.referenceFile && c.referenceSha256, '视觉样本必须有独立参考清单');
          await copyFile(
            path.join(corpus, c.referenceFile),
            path.join(runs, created[0], 'render-reference.png'),
            constants.COPYFILE_EXCL,
          );
          const visual = await runProcess(
            devPythonExecutable,
            [
              '-I',
              path.join(root, 'scripts/compare-render-reference.py'),
              '--report',
              path.join(runs, created[0], 'report.json'),
              '--reference',
              path.join(corpus, c.referenceFile),
              '--sha256',
              c.referenceSha256,
              '--acceptance',
              c.file === 'public-text.psd' ? 'text' : 'raster',
            ],
            { cwd: root, timeoutMs: 30000 },
          );
          result.visual = JSON.parse(
            await readFile(path.join(runs, created[0], 'visual-reference.json'), 'utf8'),
          );
          assert(
            visual.code === 0 && result.visual.acceptance.passed,
            JSON.stringify(result.visual.results),
          );
        }
        result.passed = true;
      } catch (error) {
        result.error = String(error);
        result.output = outcome.output.slice(-5000);
      }
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
          assert.equal(audit.remaining.length, 0);
        } catch (error) {
          result.passed = false;
          result.processError = String(error);
        }
      }
      if (singleReport) data.push({ ...singleReport, passed: result.passed });
      report.results.push(result);
      await checkpoint();
      console.log(
        JSON.stringify({
          round,
          sample: c.file,
          passed: result.passed,
          durationMs: result.durationMs,
        }),
      );
      if (!result.passed && !values['keep-going'])
        throw new Error('保留失败报告并停止本批，不自动重试：' + c.file);
    }
  }
  assert(report.results.every((r) => r.instanceId));
  assert.equal(new Set(report.results.map((r) => r.instanceId)).size, report.results.length);
  assert(
    report.results.every((r) => r.passed),
    '独立试验已全部执行，失败结果保留，整个批次不能通过',
  );
  report.status = project ? 'completedWithLimitations' : 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = String(error);
  process.exitCode = 1;
} finally {
  await checkpoint();
}
console.log(JSON.stringify({ directory, status: report.status, completed: report.results.length }));
