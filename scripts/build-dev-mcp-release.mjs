// 复用 Cargo release 缓存；无论构建成败都恢复普通发行 EXE，避免覆盖隔离门禁。
import { copyFile, mkdir, mkdtemp } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const binary = path.join(root, 'target/release/layerlens.exe');
const local = path.join(root, '.local/dev-mcp');
await mkdir(local, { recursive: true });
const backup = path.join(await mkdtemp(path.join(local, 'production-backup-')), 'layerlens.exe');
await copyFile(binary, backup);
try {
  execFileSync(
    process.execPath,
    [
      path.join(root, 'node_modules/@tauri-apps/cli/tauri.js'),
      'build',
      '--no-bundle',
      '--features',
      'dev-mcp',
      '--',
      '--locked',
    ],
    { cwd: root, stdio: 'inherit', windowsHide: true },
  );
  const output = path.join(root, 'target/dev-mcp-release');
  await mkdir(output, { recursive: true });
  await copyFile(binary, path.join(output, 'layerlens.exe'));
} finally {
  await copyFile(backup, binary);
}
