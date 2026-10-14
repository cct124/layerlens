import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import { windowsPowerShellEnvironment } from './windows-powershell.mjs';

const execFileAsync = promisify(execFile);
const runner = fileURLToPath(new URL('./run-powershell.mjs', import.meta.url));

test('模块环境适配不依赖变量大小写，保留其他配置且不修改父环境', () => {
  const parent = Object.freeze({
    PSModulePath: 'pwsh-modules',
    psMODULEpath: 'duplicate-modules',
    Path: 'executables',
    WinPSModulePath: 'windows-modules',
    WEBVIEW2_BROWSER_EXECUTABLE_FOLDER: 'fixed-runtime',
  });
  assert.deepEqual(windowsPowerShellEnvironment(parent), {
    Path: 'executables',
    WinPSModulePath: 'windows-modules',
    WEBVIEW2_BROWSER_EXECUTABLE_FOLDER: 'fixed-runtime',
  });
  assert.equal(parent.PSModulePath, 'pwsh-modules');
  assert.deepEqual(windowsPowerShellEnvironment({ Path: 'executables' }), { Path: 'executables' });
});

test(
  '真实 Windows PowerShell 子进程在错误模块路径下恢复系统命令并保留参数和退出码',
  { skip: process.platform !== 'win32' },
  async (t) => {
    const directory = await mkdtemp(path.join(tmpdir(), 'layerlens-powershell-'));
    // 只清理本测试创建的绝对临时目录。
    t.after(() => rm(directory, { recursive: true, force: true }));
    const script = path.join(directory, '模块 检查.ps1');
    const source = `param([string]$Label, [int]$ExitCode = 0)
$ErrorActionPreference = 'Stop'
@{
  label = $Label
  hash = (Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash
  signature = (Get-AuthenticodeSignature -LiteralPath $PSCommandPath).Status.ToString()
  cim = (Get-Command Get-CimInstance).Name
} | ConvertTo-Json -Compress
exit $ExitCode
`;
    await writeFile(script, source);
    const label = 'argument with spaces; $literal';
    const args = [runner, '-ExecutionPolicy', 'Bypass', '-File', script, '-Label', label];
    const options = {
      env: { ...windowsPowerShellEnvironment(), PSModulePath: path.join(directory, 'missing') },
      windowsHide: true,
      timeout: 30000,
    };
    const { stdout } = await execFileAsync(process.execPath, args, options);
    assert.deepEqual(JSON.parse(stdout.replace(/^\uFEFF/, '')), {
      label,
      hash: createHash('sha256').update(source).digest('hex').toUpperCase(),
      signature: 'NotSigned',
      cim: 'Get-CimInstance',
    });
    await assert.rejects(
      execFileAsync(process.execPath, [...args, '-ExitCode', '7'], options),
      (error) => error.code === 7,
    );
  },
);
