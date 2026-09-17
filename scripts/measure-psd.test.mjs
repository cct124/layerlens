import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { copyFile, mkdtemp, readFile, readdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);
const root = fileURLToPath(new URL('../', import.meta.url));
const script = join(root, 'scripts/measure-psd.ps1');
const fixture = (name) => join(root, 'crates/layerlens-core/tests/fixtures/psd', name);

async function measure(input, output, ...extra) {
  return execFileAsync(
    'powershell',
    [
      '-NoProfile',
      '-ExecutionPolicy',
      'Bypass',
      '-File',
      script,
      '-InputPath',
      input,
      '-OutputPath',
      output,
      '-Runs',
      '2',
      ...extra,
    ],
    { cwd: root, windowsHide: true, timeout: 120_000 },
  );
}

test(
  'Windows 测量会话的结果、目录保护与失败清理',
  { skip: process.platform !== 'win32' },
  async (t) => {
    const directory = await mkdtemp(join(tmpdir(), 'layerlens-measure-'));
    // 清理范围仅为本测试创建的隔离目录。
    t.after(() => rm(directory, { recursive: true, force: true, maxRetries: 3 }));
    const input = join(directory, '输入 样本.psd');
    await copyFile(fixture('bitmap-rle.psd'), input);
    const original = await readFile(input);
    const output = join(directory, '测量 结果');

    await t.test('路径含空格和中文时完成重复读取、PNG 输出及释放检查点', async () => {
      await measure(input, output, '-Mode', 'all');
      const baseline = JSON.parse(await readFile(join(output, 'baseline.json'), 'utf8'));
      assert.equal(baseline.failure, null);
      assert.equal(baseline.exitCode, 0);
      assert.equal(baseline.runs.length, 2);
      assert.equal(baseline.repeatOpenStatisticsMs.totalMs.count, 1);
      assert.deepEqual(
        baseline.checkpoints.map(({ run, phase }) => [run, phase]),
        [
          [0, 'ready'],
          [1, 'opened'],
          [1, 'exported'],
          [1, 'released'],
          [2, 'opened'],
          [2, 'exported'],
          [2, 'released'],
        ],
      );
      for (const checkpoint of baseline.checkpoints) {
        assert.ok(checkpoint.workingSetBytes > 0);
        assert.ok(checkpoint.privateBytes > 0);
        assert.ok(checkpoint.processPeakWorkingSetBytes >= checkpoint.workingSetBytes);
      }
      for (const run of baseline.runs) {
        assert.equal(run.buildMode, 'release');
        assert.equal(run.succeeded, true);
        assert.equal(run.summary.export.supported, 1);
        assert.equal(run.summary.export.blockers.hidden, 1);
        assert.equal(run.document, undefined);
      }
      const first = join(output, 'run-001');
      assert.deepEqual(
        await readFile(join(first, 'preview.png')),
        await readFile(join(first, 'preview-repeat.png')),
      );
      assert.deepEqual(await readFile(input), original);
    });

    await t.test('拒绝覆盖既有结果', async () => {
      const path = join(output, 'baseline.json');
      const before = await readFile(path);
      await assert.rejects(measure(input, output), /OutputPath must not already exist/);
      assert.deepEqual(await readFile(path), before);
    });

    await t.test('超资源阈值时终止子进程且不报告成功', async () => {
      const guarded = join(directory, 'guarded');
      await assert.rejects(
        measure(input, guarded, '-MaxWorkingSetMiB', '1'),
        /Working-set guard exceeded/,
      );
      // 超限可能发生在 ready 之前；只有已确认归属的目录才允许写失败报告。
      const files = await readdir(guarded).catch((error) => {
        if (error.code === 'ENOENT') return [];
        throw error;
      });
      if (files.includes('baseline.json')) {
        const report = JSON.parse(await readFile(join(guarded, 'baseline.json'), 'utf8'));
        assert.match(report.failure, /Working-set guard exceeded/);
        assert.deepEqual(report.repeatOpenStatisticsMs, {});
      }
      assert.deepEqual(await readFile(input), original);
    });

    await t.test('解析失败保留原因且不生成成功轮次', async () => {
      const rejected = join(directory, 'unsupported');
      await assert.rejects(measure(fixture('rgb16.psd'), rejected), /Measurement child failed/);
      const report = JSON.parse(await readFile(join(rejected, 'baseline.json'), 'utf8'));
      assert.notEqual(report.exitCode, 0);
      assert.equal(report.runs.length, 0);
      assert.match(report.stderr, /Unsupported/);
      assert.deepEqual(report.repeatOpenStatisticsMs, {});
    });
  },
);
