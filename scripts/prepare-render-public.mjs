// 只复用明确指定的已下载公开样本；源文件与独立参考均先校验原始下载清单。
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const source = process.argv[2];
assert(source, '用法：node scripts/prepare-render-public.mjs <已下载的 psd-tools upstream 目录>');
const output = fileURLToPath(new URL('../.local/render-corpus/', import.meta.url));
await mkdir(output, { recursive: true });
const cases = [];
async function verified(relative) {
  const file = path.join(source, relative);
  const metadata = JSON.parse(await readFile(file + '.source.json', 'utf8'));
  const bytes = await readFile(file);
  assert.equal(createHash('sha256').update(bytes).digest('hex'), metadata.sha256);
  assert.equal(bytes.length, metadata.bytes);
  return { bytes, metadata };
}
for (const name of [
  '2layers',
  'semi-transparent-layers',
  'background-red-opacity-80',
  'text',
  'masks2',
  'masks/2',
]) {
  const { bytes, metadata } = await verified('tests/psd_files/' + name + '.psd');
  const file = 'public-' + name.replaceAll('/', '-') + '.psd';
  await writeFile(path.join(output, file), bytes);
  const record = {
    file,
    sha256: metadata.sha256,
    sourceUrl: metadata.sourceUrl,
    width: bytes.readUInt32BE(18),
    height: bytes.readUInt32BE(14),
    reference: 'PSD saved composite; renderer output is separate',
  };
  if (name === 'masks/2') {
    const ref = await verified('tests/psd_files/masks/2.png');
    await writeFile(path.join(output, 'public-masks-2-reference.png'), ref.bytes);
    record.independentReference = { file: 'public-masks-2-reference.png', ...ref.metadata };
  }
  cases.push(record);
}
await writeFile(
  path.join(output, 'public-manifest.json'),
  JSON.stringify({ source: 'psd-tools 51082d37f041425a9721e8a4252214a12b35cfa9', cases }, null, 2) +
    '\n',
);
console.log(JSON.stringify({ output, cases: cases.length }));
