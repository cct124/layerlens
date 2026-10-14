// 测试子进程有明确期限；输出只保留末尾诊断，不以超时后的强制结束作为通过。
import { spawn } from 'node:child_process';
import { redact } from './dev-mcp-artifacts.mjs';

/** 防止开发者全局 WebView2 覆盖项把测试指向日常 profile 或调试会话。 */
export function isolatedEnvironment(runtimePath, parent = process.env) {
  const environment = { ...parent };
  for (const name of Object.keys(environment))
    if (name.toUpperCase().startsWith('WEBVIEW2_')) delete environment[name];
  environment.WEBVIEW2_BROWSER_EXECUTABLE_FOLDER = runtimePath;
  return environment;
}

export async function runProcess(
  command,
  args,
  { cwd, env = process.env, timeoutMs = 120000 } = {},
) {
  const started = performance.now();
  const child = spawn(command, args, {
    cwd,
    env,
    windowsHide: true,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let output = '',
    timedOut = false,
    spawnError;
  const append = (chunk) => {
    output = (output + chunk.toString()).slice(-128 * 1024);
  };
  child.stdout.on('data', append);
  child.stderr.on('data', append);
  const timer = setTimeout(() => {
    timedOut = true;
    child.kill();
  }, timeoutMs);
  const outcome = await new Promise((resolve) => {
    child.once('error', (error) => {
      spawnError = String(error);
    });
    child.once('close', (code, signal) => resolve({ code, signal }));
  });
  clearTimeout(timer);
  return {
    ...outcome,
    timedOut,
    spawnError,
    output: redact(output),
    durationMs: Math.round(performance.now() - started),
  };
}
