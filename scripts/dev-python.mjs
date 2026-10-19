// 测试 Python 使用仓库独立 venv；准备显式安装，检查和运行从不自动下载依赖。
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const environment = path.join(root, '.local/dev-mcp-python');
export const devPythonExecutable = path.join(
  environment,
  process.platform === 'win32' ? 'Scripts/python.exe' : 'bin/python',
);
const executable = devPythonExecutable;
const version = readFileSync(path.join(root, '.python-version'), 'utf8').trim();
const pillow = readFileSync(path.join(root, 'scripts/requirements-dev-mcp.txt'), 'utf8').match(
  /^Pillow==([0-9.]+)$/m,
)?.[1];
if (!pillow) throw new Error('测试 Python 依赖未固定 Pillow 版本');
const options = { cwd: root, windowsHide: true, encoding: 'utf8', stdio: 'pipe', timeout: 30000 };

export function checkDevPython() {
  try {
    const info = JSON.parse(
      execFileSync(
        executable,
        [
          '-I',
          '-c',
          'import json,platform,PIL; print(json.dumps(dict(python=platform.python_version(),pillow=PIL.__version__)))',
        ],
        options,
      ),
    );
    if (info.python !== version || info.pillow !== pillow)
      throw new Error(`需要 Python ${version} / Pillow ${pillow}，实际 ${JSON.stringify(info)}`);
    return info;
  } catch (cause) {
    throw new Error(
      '测试 Python 不可用，请先运行 npm run prepare:dev-python。' +
        (cause.stderr?.toString() || cause.message),
      { cause },
    );
  }
}

export function runDevPython(args, overrides = {}) {
  return execFileSync(executable, ['-I', ...args], { ...options, ...overrides });
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  if (args.length === 1 && args[0] === '--prepare') {
    const actual = execFileSync(
      'python',
      ['-I', '-c', 'import platform; print(platform.python_version())'],
      options,
    ).trim();
    if (actual !== version)
      throw new Error(`请按 .python-version 安装 Python ${version}；当前为 ${actual}`);
    // venv 可重复准备已有目录，同时恢复此前中断的 ensurepip；不删除本机环境。
    execFileSync('python', ['-I', '-m', 'venv', environment], {
      ...options,
      timeout: 120000,
      stdio: 'inherit',
    });
    runDevPython(
      [
        '-m',
        'pip',
        'install',
        '--disable-pip-version-check',
        '--only-binary=:all:',
        '-r',
        path.join(root, 'scripts/requirements-dev-mcp.txt'),
      ],
      { timeout: 120000, stdio: 'inherit' },
    );
  }
  const info = checkDevPython();
  if (args.length === 1 && ['--prepare', '--check'].includes(args[0]))
    console.log(JSON.stringify(info));
  else {
    if (!args.length) throw new Error('需要 Python 脚本或 --prepare / --check');
    runDevPython(args, { stdio: 'inherit' });
  }
}
