import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { mkdtemp, mkdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

import { checkStagedFormat } from './check-staged-format.mjs';

const execFileAsync = promisify(execFile);
const checkerPath = fileURLToPath(new URL('./check-staged-format.mjs', import.meta.url));
const prettierConfig = { singleQuote: true, semi: true, trailingComma: 'all', printWidth: 100 };
const rustfmtConfig = 'edition = "2024"\nstyle_edition = "2024"\n';

async function git(cwd, ...args) {
  const result = await execFileAsync('git', args, { cwd, encoding: 'buffer', windowsHide: true });
  return result.stdout;
}

async function put(cwd, path, content) {
  const absolutePath = join(cwd, path);
  await mkdir(dirname(absolutePath), { recursive: true });
  await writeFile(absolutePath, content);
}

async function stage(cwd, path, content) {
  await put(cwd, path, content);
  await git(cwd, 'add', '--', path);
}

async function createRepo(t) {
  const cwd = await mkdtemp(join(tmpdir(), 'layerlens-staged-format-'));
  // 仅删除本测试由 mkdtemp 创建的目录，不使用用户仓库或外部路径。
  t.after(() => rm(cwd, { recursive: true, force: true, maxRetries: 3 }));
  await git(cwd, 'init', '--quiet');
  await git(cwd, 'config', 'user.name', 'LayerLens format test');
  await git(cwd, 'config', 'user.email', 'format-test@example.invalid');
  await git(cwd, 'config', 'core.autocrlf', 'false');
  await git(cwd, 'config', 'core.safecrlf', 'false');
  await git(cwd, 'config', 'commit.gpgsign', 'false');
  await git(cwd, 'config', 'core.hooksPath', '.git/test-hooks');
  await put(cwd, '.prettierrc.json', `${JSON.stringify(prettierConfig, null, 2)}\n`);
  await put(cwd, '.prettierignore', 'src/shared/api/generated.ts\n');
  await put(cwd, '.rustfmt.toml', rustfmtConfig);
  await put(cwd, 'rust-toolchain.toml', '[toolchain]\nchannel = "1.96.0"\nprofile = "minimal"\n');
  await git(cwd, 'add', '.');
  await git(cwd, 'commit', '--quiet', '-m', 'fixture: formatter configuration');
  return cwd;
}

async function capture(cwd, paths) {
  return {
    stagedDiff: await git(cwd, 'diff', '--cached', '--binary', '--no-ext-diff'),
    indexEntries: await git(cwd, 'ls-files', '--stage', '-z'),
    files: await Promise.all(paths.map((path) => readFile(join(cwd, path)))),
  };
}

function assertFailure(result, path, tool) {
  const failure = result.failures.find((item) => item.path === path && item.tool === tool);
  assert.ok(failure, `应报告 ${path} 的 ${tool} 格式问题，实际为 ${JSON.stringify(result)}`);
  assert.equal(typeof failure.message, 'string');
  assert.ok(failure.message.length > 0);
}

test('暂存 TypeScript 正确时，未暂存的坏格式不阻止提交且文件与索引不变', async (t) => {
  const cwd = await createRepo(t);
  const path = 'src/partial.ts';
  const staged = 'export const answer = 42;\n';
  await stage(cwd, path, staged);
  await put(cwd, path, 'export  const answer={value:42}\n');
  const before = await capture(cwd, [path]);

  const result = await checkStagedFormat({ cwd });

  assert.equal(result.checked, 1);
  assert.deepEqual(result.failures, []);
  assert.deepEqual(await capture(cwd, [path]), before);
  assert.equal((await git(cwd, 'show', `:${path}`)).toString(), staged);
});

test('暂存 TypeScript 格式错误时，即使工作树已修正也必须失败且不改动文件', async (t) => {
  const cwd = await createRepo(t);
  const path = 'src/partial.ts';
  const staged = 'export  const answer={value:42}\n';
  await stage(cwd, path, staged);
  await put(cwd, path, 'export const answer = { value: 42 };\n');
  const before = await capture(cwd, [path]);

  const result = await checkStagedFormat({ cwd });

  assertFailure(result, path, 'prettier');
  assert.deepEqual(await capture(cwd, [path]), before);
  assert.equal((await git(cwd, 'show', `:${path}`)).toString(), staged);
});

test('暂存 Rust 坏格式被拒绝，重新暂存修正内容后通过', async (t) => {
  const cwd = await createRepo(t);
  const path = 'src/lib.rs';
  await stage(cwd, path, 'pub fn answer()->u32{42}\n');
  await put(cwd, path, 'pub fn answer() -> u32 {\n    42\n}\n');
  const before = await capture(cwd, [path]);

  assertFailure(await checkStagedFormat({ cwd }), path, 'rustfmt');
  assert.deepEqual(await capture(cwd, [path]), before);

  await stage(cwd, path, 'pub fn answer() -> u32 {\n    42\n}\n');
  const result = await checkStagedFormat({ cwd });
  assert.equal(result.checked, 1);
  assert.deepEqual(result.failures, []);
});

test('Rust 单文件检查不读取工作树中的子模块', async (t) => {
  const cwd = await createRepo(t);
  const parent = 'src/lib.rs';
  const child = 'src/child.rs';
  await stage(cwd, parent, 'mod child;\n');
  await stage(cwd, child, 'pub fn answer() -> u32 {\n    42\n}\n');
  await git(cwd, 'commit', '--quiet', '-m', 'fixture: Rust modules');
  await stage(cwd, parent, 'mod child;\n\npub fn answer() -> u32 {\n    child::answer()\n}\n');
  await put(cwd, child, 'this is deliberately invalid Rust in the working tree\n');
  const before = await capture(cwd, [parent, child]);

  const result = await checkStagedFormat({ cwd });

  assert.equal(result.checked, 1);
  assert.deepEqual(result.failures, []);
  assert.deepEqual(await capture(cwd, [parent, child]), before);
});

test('支持带空格、中文目录及文件名的暂存重命名', async (t) => {
  const cwd = await createRepo(t);
  const previous = 'src/旧 名.ts';
  const next = 'src/目录 有空格/新 名.ts';
  await stage(cwd, previous, "export const message = '你好';\n");
  await git(cwd, 'commit', '--quiet', '-m', 'fixture: named file');
  await mkdir(dirname(join(cwd, next)), { recursive: true });
  await rename(join(cwd, previous), join(cwd, next));
  await git(cwd, 'add', '--', previous, next);
  const before = await capture(cwd, [next]);

  const result = await checkStagedFormat({ cwd });

  assert.equal(result.checked, 1);
  assert.deepEqual(result.failures, []);
  assert.deepEqual(await capture(cwd, [next]), before);
});

test('暂存删除不尝试读取或格式化已删除文件', async (t) => {
  const cwd = await createRepo(t);
  const path = 'src/deleted.ts';
  await stage(cwd, path, 'export const answer = 42;\n');
  await git(cwd, 'commit', '--quiet', '-m', 'fixture: removable file');
  await rm(join(cwd, path));
  await git(cwd, 'add', '--', path);
  const before = await capture(cwd, []);

  assert.deepEqual(await checkStagedFormat({ cwd }), { checked: 0, failures: [] });
  assert.deepEqual(await capture(cwd, []), before);
});

test('生成文件遵循暂存的 Prettier ignore', async (t) => {
  const cwd = await createRepo(t);
  const path = 'src/shared/api/generated.ts';
  await stage(cwd, path, 'export  type Generated={value:number}\n');
  await put(cwd, '.prettierignore', '');
  const before = await capture(cwd, [path, '.prettierignore']);

  assert.deepEqual(await checkStagedFormat({ cwd }), { checked: 0, failures: [] });
  assert.deepEqual(await capture(cwd, [path, '.prettierignore']), before);
});

test('使用暂存的 Prettier 配置而非工作树配置', async (t) => {
  const cwd = await createRepo(t);
  const path = 'src/quotes.ts';
  const configPath = '.prettierrc.json';
  const config = { ...prettierConfig, singleQuote: false };
  await stage(cwd, configPath, `${JSON.stringify(config, null, 2)}\n`);
  await stage(cwd, path, 'export const message = "staged configuration";\n');
  await put(cwd, configPath, `${JSON.stringify(prettierConfig, null, 2)}\n`);
  const before = await capture(cwd, [path, configPath]);

  const result = await checkStagedFormat({ cwd });

  assert.ok(result.checked >= 1);
  assert.deepEqual(result.failures, []);
  assert.deepEqual(await capture(cwd, [path, configPath]), before);
});

test('格式配置变化时也检查没有内容改动的索引文件', async (t) => {
  const cwd = await createRepo(t);
  const path = 'src/existing.ts';
  await stage(cwd, path, "export const message = 'old configuration';\n");
  await git(cwd, 'commit', '--quiet', '-m', 'fixture: existing formatted file');
  const config = { ...prettierConfig, singleQuote: false };
  await stage(cwd, '.prettierrc.json', `${JSON.stringify(config, null, 2)}\n`);

  assertFailure(await checkStagedFormat({ cwd }), path, 'prettier');
});

test('格式配置重命名移走时重新检查未改动的索引文件', async (t) => {
  const cwd = await createRepo(t);
  const path = 'src/existing.ts';
  const previous = 'src/.prettierrc.json';
  const next = 'src/prettier.saved.json';
  const config = { ...prettierConfig, singleQuote: false };
  await git(cwd, 'config', 'diff.renames', 'true');
  await stage(cwd, previous, `${JSON.stringify(config, null, 2)}\n`);
  await stage(cwd, path, 'export const message = "nested configuration";\n');
  await git(cwd, 'commit', '--quiet', '-m', 'fixture: nested formatter configuration');
  await rename(join(cwd, previous), join(cwd, next));
  await git(cwd, 'add', '--', previous, next);
  const before = await capture(cwd, [path, next]);

  assertFailure(await checkStagedFormat({ cwd }), path, 'prettier');
  assert.deepEqual(await capture(cwd, [path, next]), before);
});

test('使用暂存的 rustfmt 配置而非工作树配置', async (t) => {
  const cwd = await createRepo(t);
  const path = 'src/lib.rs';
  const configPath = '.rustfmt.toml';
  await stage(cwd, configPath, `${rustfmtConfig}hard_tabs = true\n`);
  await stage(cwd, path, 'pub fn answer() -> u32 {\n\t42\n}\n');
  await put(cwd, configPath, rustfmtConfig);
  const before = await capture(cwd, [path, configPath]);

  const result = await checkStagedFormat({ cwd });

  assert.deepEqual(result.failures, []);
  assert.deepEqual(await capture(cwd, [path, configPath]), before);
});

test('无暂存改动时直接成功', async (t) => {
  const cwd = await createRepo(t);
  const before = await capture(cwd, []);

  assert.deepEqual(await checkStagedFormat({ cwd }), { checked: 0, failures: [] });
  assert.deepEqual(await capture(cwd, []), before);
});

test('CLI 对暂存格式错误返回非零状态', async (t) => {
  const cwd = await createRepo(t);
  await stage(cwd, 'src/invalid.ts', 'export  const answer={value:42}\n');

  await assert.rejects(
    execFileAsync(process.execPath, [checkerPath], { cwd, windowsHide: true }),
    (error) => {
      assert.equal(error.code, 1);
      assert.match(`${error.stdout}${error.stderr}`, /src\/invalid\.ts/u);
      return true;
    },
  );
});
