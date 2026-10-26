// 独立进程句柄观测；失败路径也保留退出码，驱动清理不能冒充应用正常退出。
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { windowsPowerShellEnvironment } from './windows-powershell.mjs';

/** 有界保留 fetch 的 cause／聚合错误，不展开请求、响应或任意对象属性。 */
export function errorDiagnostic(error, depth = 0, seen = new Set()) {
  if (depth >= 4 || seen.has(error)) return { truncated: true };
  if (error === null || typeof error !== 'object') return { message: String(error).slice(0, 2048) };
  seen.add(error);
  const result = {
    name: String(error.name ?? 'Error').slice(0, 128),
    message: String(error.message ?? error).slice(0, 2048),
  };
  for (const key of ['code', 'errno', 'syscall'])
    if (['string', 'number'].includes(typeof error[key]))
      result[key] = typeof error[key] === 'string' ? error[key].slice(0, 256) : error[key];
  if (error.cause !== undefined) result.cause = errorDiagnostic(error.cause, depth + 1, seen);
  if (Array.isArray(error.errors)) {
    result.errors = error.errors
      .slice(0, 4)
      .map((entry) => errorDiagnostic(entry, depth + 1, seen));
    if (error.errors.length > 4) result.errorsTruncated = true;
  }
  return result;
}

/** 只监测运行器刚启动的应用 PID；必须在删除 WebDriver 会话前保存 snapshot。 */
export function observeProcess(pid) {
  assert(Number.isSafeInteger(pid) && pid > 0, '进程监测需要有效 PID');
  const command =
    "$ErrorActionPreference='Stop'; $testProcess=[Diagnostics.Process]::GetProcessById(" +
    pid +
    "); $null=$testProcess.Handle; Write-Output 'watching'; $testProcess.WaitForExit(); Write-Output $testProcess.ExitCode";
  const child = spawn('powershell', ['-NoProfile', '-NonInteractive', '-Command', command], {
    env: windowsPowerShellEnvironment(),
    windowsHide: true,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let stdout = '',
    stderr = '';
  let state = { status: 'starting', at: new Date().toISOString() };
  let readyResolve, readyReject, exitResolve, exitReject;
  const ready = new Promise((resolve, reject) => {
    readyResolve = resolve;
    readyReject = reject;
  });
  const exit = new Promise((resolve, reject) => {
    exitResolve = resolve;
    exitReject = reject;
  });
  const fail = (error) => {
    state = { status: 'unavailable', at: new Date().toISOString(), error: errorDiagnostic(error) };
    readyReject(error);
    exitReject(error);
  };
  child.once('error', fail);
  child.stdout.on('data', (chunk) => {
    stdout = (stdout + chunk.toString()).slice(-16384);
    if (stdout.includes('watching') && state.status === 'starting') {
      state = { status: 'watching', at: new Date().toISOString() };
      readyResolve();
    }
  });
  child.stderr.on('data', (chunk) => {
    stderr = (stderr + chunk.toString()).slice(-8192);
  });
  child.once('close', (code, signal) => {
    const match = stdout.match(/watching\s+(-?\d+)/);
    if (code === 0 && match) {
      const exitCode = Number(match[1]);
      state = { status: 'exited', at: new Date().toISOString(), exitCode };
      readyResolve();
      exitResolve(exitCode);
    } else if (state.status !== 'unavailable') {
      fail(new Error('退出监测失败：' + code + ' ' + signal + ' ' + stderr));
    }
  });
  ready.catch(() => undefined);
  exit.catch(() => undefined);
  return { ready, exit, child, snapshot: () => structuredClone(state) };
}
