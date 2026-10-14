import { mkdir, mkdtemp } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { collectArtifacts } from './dev-mcp-artifacts.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const { values } = parseArgs({
  options: { source: { type: 'string' }, output: { type: 'string' } },
});
const source = path.resolve(root, values.source ?? '.local/dev-mcp');
await mkdir(source, { recursive: true });
// 固定 CI 目标必须尚不存在；默认每次创建独立目录，保留以往失败报告。
const parent = values.output ? null : await mkdtemp(path.join(source, 'artifacts-'));
const output = values.output ? path.resolve(root, values.output) : path.join(parent, 'files');
const manifest = await collectArtifacts(source, output);
console.log(
  JSON.stringify(
    { output, files: manifest.files.length, bytes: manifest.bytes, errors: manifest.errors },
    null,
    2,
  ),
);
if (manifest.errors.length) process.exitCode = 1;
