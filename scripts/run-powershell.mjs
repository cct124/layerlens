// npm 经 pwsh → cmd/Node 启动时也使用同一 Windows PowerShell 环境边界。
import { spawn } from 'node:child_process';
import { windowsPowerShellEnvironment } from './windows-powershell.mjs';

const child = spawn('powershell', ['-NoProfile', '-NonInteractive', ...process.argv.slice(2)], {
  env: windowsPowerShellEnvironment(),
  windowsHide: true,
  stdio: 'inherit',
});
child.once('error', (error) => {
  console.error('Windows PowerShell 启动失败：', error.message);
  process.exitCode = 1;
});
child.once('close', (code) => {
  process.exitCode = code ?? 1;
});
