import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { runProcess } from './dev-mcp-runner.mjs';
import { windowsPowerShellEnvironment } from './windows-powershell.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
for (const fakeBinary of [false, true]) {
  test(
    fakeBinary
      ? '独立运行时拒绝无版本的伪 EXE，失败记录停在下载前'
      : '独立运行时目录缺失时不回退到系统安装；支持空格及绝对报告路径',
    { skip: process.platform !== 'win32', timeout: 20000 },
    async (t) => {
      const directory = await mkdtemp(path.join(os.tmpdir(), 'layerlens runtime-'));
      t.after(() => rm(directory, { recursive: true, force: true }));
      const runtime = path.join(directory, 'test runtime');
      if (fakeBinary) {
        await mkdir(runtime);
        await writeFile(path.join(runtime, 'msedgewebview2.exe'), 'not an executable');
      }
      const reportPath = path.join(directory, 'preparation.json');
      const result = await runProcess(
        'powershell',
        [
          '-NoProfile',
          '-NonInteractive',
          '-ExecutionPolicy',
          'Bypass',
          '-File',
          path.join(root, 'scripts/prepare-dev-webdriver.ps1'),
          '-RuntimePath',
          runtime,
          '-ReportPath',
          reportPath,
          '-OutputRoot',
          path.join(directory, 'driver'),
        ],
        { cwd: root, env: windowsPowerShellEnvironment(), timeoutMs: 15000 },
      );
      assert.equal(result.timedOut, false);
      assert.equal(result.spawnError, undefined);
      assert.notEqual(result.code, 0);
      const report = JSON.parse(await readFile(reportPath, 'utf8'));
      assert.equal(report.passed, false);
      assert.equal(report.phase, 'runtime');
      assert.match(report.error, fakeBinary ? /有效版本/ : /不回退/);
      assert.equal(report.sourceUrl, undefined);
      assert.equal(report.runtimeSelection, undefined);
    },
  );
}
