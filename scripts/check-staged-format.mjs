import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as prettier from 'prettier';

const formatterConfigs = new Set([
  '.editorconfig',
  '.prettierignore',
  '.prettierrc',
  '.prettierrc.json',
  '.prettierrc.yaml',
  '.prettierrc.yml',
  '.prettierrc.js',
  '.prettierrc.cjs',
  '.prettierrc.mjs',
  'prettier.config.js',
  'prettier.config.cjs',
  'prettier.config.mjs',
  'package.json',
  'rustfmt.toml',
  '.rustfmt.toml',
  'rust-toolchain.toml',
]);

function git(cwd, args) {
  return execFileSync('git', args, { cwd, maxBuffer: 64 * 1024 * 1024 });
}

function indexFiles(cwd) {
  const entries = new Map();
  for (const entry of git(cwd, ['ls-files', '--stage', '-z']).toString('utf8').split('\0')) {
    if (!entry) continue;
    const separator = entry.indexOf('\t');
    const [mode, hash, stage] = entry.slice(0, separator).split(' ');
    const path = entry.slice(separator + 1);
    if (stage !== '0') throw new Error(`请先解决合并冲突：${path}`);
    // 不跟随符号链接或子模块，避免格式检查访问仓库外的内容。
    if (mode === '100644' || mode === '100755') entries.set(path, hash);
  }
  return entries;
}

function rustConfigPath(filePath, snapshot) {
  let directory = dirname(filePath);
  while (true) {
    for (const name of ['.rustfmt.toml', 'rustfmt.toml']) {
      const candidate = join(directory, name);
      if (existsSync(candidate)) return candidate;
    }
    if (directory === snapshot) return null;
    directory = dirname(directory);
  }
}

/**
 * 检查 Git 索引中的格式；配置也取自索引。仅写入隔离的临时目录，
 * 不执行 stash、add 或工作区格式修复，保留部分暂存的边界。
 */
export async function checkStagedFormat({ cwd = process.cwd() } = {}) {
  const root = git(cwd, ['rev-parse', '--show-toplevel']).toString('utf8').trim();
  const entries = indexFiles(root);
  // 将重命名展开为删除与新增，格式配置被改名移走时也触发全量索引检查。
  const changed = git(root, ['diff', '--cached', '--no-renames', '--name-only', '-z'])
    .toString('utf8')
    .split('\0')
    .filter(Boolean);
  const allFiles = changed.some((path) => formatterConfigs.has(basename(path)));
  const candidates = allFiles ? [...entries.keys()] : changed.filter((path) => entries.has(path));
  const result = { checked: 0, failures: [] };
  if (candidates.length === 0) return result;

  const snapshot = await mkdtemp(join(tmpdir(), 'layerlens-format-'));
  const readBlob = (path) => git(root, ['cat-file', 'blob', entries.get(path)]);
  try {
    for (const path of entries.keys()) {
      if (!formatterConfigs.has(basename(path))) continue;
      const target = join(snapshot, path);
      await mkdir(dirname(target), { recursive: true });
      await writeFile(target, readBlob(path));
    }

    // resolveConfig 有进程内缓存；同一进程验证不同暂存快照时也必须读取各自配置。
    await prettier.clearConfigCache();
    for (const path of candidates) {
      const filePath = join(snapshot, path);
      const tool = path.endsWith('.rs') ? 'rustfmt' : 'prettier';
      try {
        const info = await prettier.getFileInfo(filePath, {
          ignorePath: join(snapshot, '.prettierignore'),
          resolveConfig: false,
        });
        if (info.ignored || (!path.endsWith('.rs') && !info.inferredParser)) continue;
        result.checked++;
        const source = readBlob(path).toString('utf8');
        let formatted;
        if (tool === 'rustfmt') {
          await mkdir(dirname(filePath), { recursive: true });
          const configPath = rustConfigPath(filePath, snapshot);
          if (!configPath) throw new Error('暂存快照缺少 rustfmt 配置，无法确定 Rust edition。');
          // stdin 避免递归读取工作树中的 mod 文件；每个暂存 Rust 文件分别检查。
          formatted = execFileSync(
            'rustfmt',
            ['--emit', 'stdout', '--config-path', configPath, '--config', 'skip_children=true'],
            {
              cwd: dirname(filePath),
              input: source,
              encoding: 'utf8',
              maxBuffer: 64 * 1024 * 1024,
              stdio: ['pipe', 'pipe', 'pipe'],
            },
          );
        } else {
          const config = await prettier.resolveConfig(filePath, { editorconfig: true });
          formatted = await prettier.format(source, { ...config, filepath: filePath });
        }
        if (formatted !== source) {
          result.failures.push({ path, tool, message: '暂存版本的格式不符合项目配置' });
        }
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        result.failures.push({ path, tool, message });
      }
    }
    return result;
  } finally {
    await rm(snapshot, { recursive: true, force: true });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const result = await checkStagedFormat();
    if (result.failures.length > 0) {
      for (const failure of result.failures) {
        console.error(`[${failure.tool}] ${failure.path}: ${failure.message}`);
      }
      console.error('提交已阻止。请运行 npm run format，核对 diff 后重新暂存需要提交的内容。');
      process.exitCode = 1;
    } else {
      console.log(`暂存格式检查通过（${result.checked} 个文件）。`);
    }
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
