// 固定到已核验 psd-tools 树的两个简单蒙版样本；只写忽略目录，下载后核对 Git blob 与长度。
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
const output = fileURLToPath(new URL('../.local/render-corpus/', import.meta.url));
await mkdir(output, { recursive: true });
const commit = '51082d37f041425a9721e8a4252214a12b35cfa9';
for (const [name, size, blob] of [
  ['mask', 72905, '461802afc140cf2eabf228a5cfbdd0276697e99a'],
  ['mask-disabled', 91988, '94fd3a07fde97799629f8fe84f4ac63583710b64'],
]) {
  const sourceUrl = `https://raw.githubusercontent.com/psd-tools/psd-tools/${commit}/tests/psd_files/${name}.psd`;
  const file = 'public-' + name + '.psd';
  let bytes;
  try {
    bytes = await readFile(output + file);
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
    const response = await fetch(sourceUrl, { signal: AbortSignal.timeout(30000) });
    assert(response.ok, '下载样本失败：' + response.status);
    bytes = Buffer.from(await response.arrayBuffer());
  }
  assert.equal(bytes.length, size);
  assert.equal(
    createHash('sha1')
      .update('blob ' + size + '\0')
      .update(bytes)
      .digest('hex'),
    blob,
  );
  await writeFile(output + file, bytes);
  await writeFile(
    output + file + '.source.json',
    JSON.stringify(
      {
        sourceUrl,
        commit,
        bytes: size,
        gitBlobSha1: blob,
        sha256: createHash('sha256').update(bytes).digest('hex'),
      },
      null,
      2,
    ) + '\n',
  );
  console.log(JSON.stringify({ file, bytes: size, verified: true }));
}
