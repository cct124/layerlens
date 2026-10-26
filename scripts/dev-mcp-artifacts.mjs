// 开发回归产物：只复制白名单文件，不遍历连接目录、WebView profile 或下载目录。
import { createHash } from 'node:crypto';
import { lstat, mkdir, readdir, readFile, realpath, writeFile } from 'node:fs/promises';
import path from 'node:path';

const runName = /^(desktop|webview)-[a-zA-Z0-9]+$/;
const suiteName = /^stability-[a-zA-Z0-9]+$/;
const runFiles = [
  'report.json',
  'process.log',
  'driver.log',
  'driver-service.log',
  'runner.log',
  'fixed-task.png',
  'cleanup-confirmation.png',
  'cleanup-sync-failure.png',
  'cleanup-window-minimum.png',
  'cleanup-window-default.png',
  'final-state.png',
  'generic-capture.png',
  'workspace-inspection.png',
  'workspace-25.png',
  'workspace-100.png',
  'workspace-200.png',
  'failure.png',
  'window-minimum.png',
  'window-default.png',
  'window-maximized.png',
  'window-restored.png',
  'window-recovered.png',
  'window-blur.png',
  'window-menu.png',
  'window-exit.png',
  'render-comparison.png', // 只读收集历史证据；当前运行器不再生成多引擎对比。
  'visual-reference.json',
  'render-reference.png',
  'report-progress.json',
  'project-visual.json',
  ...['leafer', 'konva'].flatMap((engine) => [
    'project-' + engine + '-strict.png',
    'project-' + engine + '-fit.png',
    ...[0, 1, 2].flatMap((index) =>
      [100, 200, 400].map((zoom) => 'project-' + engine + '-detail' + index + '-' + zoom + '.png'),
    ),
  ]),
  ...['leafer', 'konva', 'canvaskit'].flatMap((engine) =>
    [100, 200, 400].map((zoom) => 'render-' + engine + '-' + zoom + '.png'),
  ),
];

/** 保留诊断与指纹，删除显式凭据字段、Bearer 值及已知实例令牌。 */
export function redact(text, secrets = []) {
  for (const secret of secrets.filter(Boolean)) text = text.split(secret).join('[REDACTED]');
  return text
    .replace(/(Bearer\s+)[a-zA-Z0-9._~+/-]+/gi, '$1[REDACTED]')
    .replace(/("(?:token|authorization|password)"\s*:\s*")[^"\r\n]*(")/gi, '$1[REDACTED]$2');
}

/** 有界次数的每次结果都计入统计，不以重试后的成功覆盖失败。 */
export function summarize(results, expected) {
  const groups = {};
  for (const kind of ['disabled', 'business', 'webview']) {
    const entries = results.filter((entry) => entry.kind === kind);
    const durations = entries
      .map((entry) => entry.durationMs)
      .filter(Number.isFinite)
      .sort((a, b) => a - b);
    groups[kind] = {
      runs: entries.length,
      passed: entries.filter((entry) => entry.passed).length,
      failed: entries.filter((entry) => !entry.passed).length,
      minMs: durations[0] ?? null,
      medianMs: durations.length
        ? (durations[Math.floor((durations.length - 1) / 2)] +
            durations[Math.floor(durations.length / 2)]) /
          2
        : null,
      maxMs: durations.at(-1) ?? null,
    };
  }
  const ids = results.map((entry) => entry.instanceId).filter(Boolean);
  const uniqueInstances = new Set(ids).size === ids.length;
  return {
    passed:
      results.length === expected && results.every((entry) => entry.passed) && uniqueInstances,
    expected,
    completed: results.length,
    uniqueInstances,
    groups,
  };
}

/** 新建产物目录；拒绝目录联接／符号链接和超预算文件，清单记录复制后 SHA-256。 */
export async function collectArtifacts(source, destination, { runs } = {}) {
  if (
    runs !== undefined &&
    (!Array.isArray(runs) ||
      !runs.length ||
      runs.length > 128 ||
      runs.some((n) => typeof n !== 'string' || !runName.test(n)) ||
      new Set(runs).size !== runs.length)
  )
    throw new Error('产物运行名单无效');
  const selected = runs ? new Set(runs) : null;
  const found = new Set();
  const root = await realpath(source);
  await mkdir(destination, { recursive: false });
  const manifest = {
    formatVersion: 1,
    files: [],
    errors: [],
    bytes: 0,
    selectedRuns: runs ?? null,
  };
  async function safeDirectory(directory) {
    const info = await lstat(directory);
    if (!info.isDirectory() || info.isSymbolicLink())
      throw new Error('产物来源不是普通目录：' + directory);
    const relative = path.relative(root, await realpath(directory));
    if (relative === '..' || relative.startsWith('..' + path.sep) || path.isAbsolute(relative))
      throw new Error('产物目录越界：' + directory);
  }
  async function copy(directory, name) {
    const file = path.join(directory, name);
    let info;
    try {
      info = await lstat(file);
    } catch (error) {
      if (error.code === 'ENOENT') return;
      throw error;
    }
    if (!info.isFile() || info.isSymbolicLink() || info.size > 8 * 1024 * 1024)
      throw new Error('产物不是有效的有界普通文件：' + file);
    // 五套 CI 场景合计超过 512 文件，保留固定上限，字节与单文件预算不变。
    if (manifest.files.length >= 768 || manifest.bytes + info.size > 64 * 1024 * 1024)
      throw new Error('产物超过 768 文件／64 MiB 上限。');
    const relative = path.relative(root, file);
    const original = await readFile(file);
    const bytes = name.endsWith('.png') ? original : Buffer.from(redact(original.toString('utf8')));
    const target = path.join(destination, relative);
    await mkdir(path.dirname(target), { recursive: true });
    await writeFile(target, bytes, { flag: 'wx' });
    manifest.bytes += bytes.length;
    manifest.files.push({
      path: relative.split(path.sep).join('/'),
      bytes: bytes.length,
      sha256: createHash('sha256').update(bytes).digest('hex'),
    });
  }
  async function visitRuns(parent) {
    await safeDirectory(parent);
    for (const entry of await readdir(parent, { withFileTypes: true })) {
      if (!runName.test(entry.name)) continue;
      if (selected && !selected.has(entry.name)) continue;
      found.add(entry.name);
      const directory = path.join(parent, entry.name);
      try {
        await safeDirectory(directory);
        for (const name of runFiles) await copy(directory, name);
      } catch (error) {
        manifest.errors.push(String(error));
      }
    }
  }
  try {
    await safeDirectory(root);
    await copy(root, 'driver-preparation.json');
    await visitRuns(root);
    if (suiteName.test(path.basename(root))) {
      for (const name of ['summary.json', 'summary.md', 'environment.json', 'project-visual.json'])
        await copy(root, name);
      await visitRuns(path.join(root, 'runs'));
    }
    for (const entry of await readdir(root, { withFileTypes: true })) {
      if (!suiteName.test(entry.name)) continue;
      const suite = path.join(root, entry.name);
      try {
        await safeDirectory(suite);
        for (const name of [
          'summary.json',
          'summary.md',
          'environment.json',
          'project-visual.json',
        ])
          await copy(suite, name);
        await visitRuns(path.join(suite, 'runs'));
      } catch (error) {
        manifest.errors.push(String(error));
      }
    }
  } catch (error) {
    manifest.errors.push(String(error));
  }
  if (selected)
    for (const name of selected)
      if (!found.has(name)) manifest.errors.push('未找到指定运行：' + name);
  if (!manifest.files.length) manifest.errors.push('没有可保留的报告；检查是否已执行准备或回归。');
  await writeFile(path.join(destination, 'manifest.json'), JSON.stringify(manifest, null, 2), {
    flag: 'wx',
  });
  return manifest;
}
