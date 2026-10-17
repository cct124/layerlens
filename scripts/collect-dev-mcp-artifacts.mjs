import { mkdir, mkdtemp, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { collectArtifacts } from './dev-mcp-artifacts.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const { values } = parseArgs({
  options: { source: { type: 'string' }, output: { type: 'string' }, round: { type: 'string' } },
});
const source = path.resolve(root, values.source ?? '.local/dev-mcp');
await mkdir(source, { recursive: true });
// 固定 CI 目标必须尚不存在；默认每次创建独立目录，保留以往失败报告。
const parent = values.output ? null : await mkdtemp(path.join(source, 'artifacts-'));
const output = values.output ? path.resolve(root, values.output) : path.join(parent, 'files');
let runs;
if (values.round !== undefined) {
  if (!/^[0-5]$/.test(values.round)) throw new Error('round 为从零开始的 0–5');
  const summary = JSON.parse(await readFile(path.join(source, 'summary.json'), 'utf8'));
  // 保留历史报告的只读收集能力；当前运行入口只生成 Konva 回归报告。
  if (!['konva-regression', 'render-comparison'].includes(summary.kind))
    throw new Error('按轮收集只用于渲染回归报告');
  runs = summary.results.filter((r) => r.round === Number(values.round)).flatMap((r) => r.runs);
}
const manifest = await collectArtifacts(source, output, { runs });
console.log(
  JSON.stringify(
    { output, files: manifest.files.length, bytes: manifest.bytes, errors: manifest.errors },
    null,
    2,
  ),
);
if (manifest.errors.length) process.exitCode = 1;
