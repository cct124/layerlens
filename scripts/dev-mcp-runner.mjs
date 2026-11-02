// 测试子进程有明确期限；输出只保留末尾诊断，不以超时后的强制结束作为通过。
import { spawn } from 'node:child_process';
import assert from 'node:assert/strict';
import { mkdir, realpath } from 'node:fs/promises';
import path from 'node:path';
import { redact } from './dev-mcp-artifacts.mjs';

/** 附带选区观测与操作结果独立；只接受契约声明的忙碌降级，不重做已接受操作。 */
export function checkSelectionObservation(observation, sessionId) {
  assert(observation && typeof observation === 'object', '缺少附带选区观测');
  if ('unavailable' in observation) {
    assert.deepEqual(Object.keys(observation), ['unavailable']);
    assert.equal(observation.unavailable?.code, 'BUSY', JSON.stringify(observation));
    assert.equal(typeof observation.unavailable.message, 'string');
    assert(observation.unavailable.message.length > 0);
    return observation.unavailable;
  }
  assert.equal(observation.sessionId, sessionId, JSON.stringify(observation));
  return null;
}

/** 防止开发者全局 WebView2 覆盖项把测试指向日常 profile 或调试会话。 */
export function isolatedEnvironment(runtimePath, parent = process.env) {
  const environment = { ...parent };
  for (const name of Object.keys(environment))
    if (name.toUpperCase().startsWith('WEBVIEW2_')) delete environment[name];
  environment.WEBVIEW2_BROWSER_EXECUTABLE_FOLDER = runtimePath;
  return environment;
}

/** 驱动和应用必须共享同一 UUID 测试目录；否则 WebDriver 会创建临时 profile。 */
export async function prepareReusableProfile(runRoot, key) {
  assert.equal(typeof key, 'string');
  assert(
    /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(key),
    'profileKey 必须是小写 UUID',
  );
  const root = await realpath(runRoot);
  const parent = path.join(root, 'profiles');
  const profile = path.join(parent, key);
  for (const directory of [parent, profile]) {
    await mkdir(directory, { recursive: true });
    assert.equal(await realpath(directory), directory, '测试 profile 不允许目录别名或重解析路径');
  }
  return profile;
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
