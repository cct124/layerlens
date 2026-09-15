import { copyFileSync, mkdirSync, readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const output = resolve(root, '.local/generated-icons');
const result = spawnSync(
  process.execPath,
  [
    resolve(root, 'node_modules/@tauri-apps/cli/tauri.js'),
    'icon',
    resolve(root, 'scripts/app-icon.svg'),
    '--output',
    output,
  ],
  { cwd: root, stdio: 'inherit' },
);
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);

// 仅提交桌面配置实际引用的图标，其他平台的生成结果保留在忽略目录。
const config = JSON.parse(readFileSync(resolve(root, 'src-tauri/tauri.conf.json'), 'utf8'));
for (const icon of config.bundle.icon) {
  const target = resolve(root, 'src-tauri', icon);
  mkdirSync(dirname(target), { recursive: true });
  copyFileSync(resolve(output, icon.replace(/^icons\//, '')), target);
}
