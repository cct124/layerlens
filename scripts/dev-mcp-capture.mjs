// 大截图只经开发 MCP 读取固定捕获的 PNG 分块；本地拼接校验完整 RGBA 指纹。
import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { runDevPython } from './dev-python.mjs';
import path from 'node:path';

export async function captureTiled({ rpc, args, frontend, run, root, name }) {
  const call = (operation) =>
    rpc('tools/call', {
      name: 'dev_ui_capture',
      arguments: args({ frontendId: frontend, operation }),
    });
  const first = await call({ kind: 'create' });
  assert.equal(first.isError, false, JSON.stringify(first.structuredContent));
  const { capture, screenshot } = first.structuredContent.data.value;
  let failure;
  try {
    assert.match(capture.captureId, /^[0-9a-f-]{36}$/);
    assert.equal(capture.tileSize, 256);
    assert(capture.tileCount > 0 && capture.tileCount <= 1024);
    assert.equal(screenshot.unchanged, true, '截图捕获期间观测已变化');
    const directory = path.join(run, 'captures', capture.captureId);
    await mkdir(directory, { recursive: true });
    const tiles = [];
    for (let index = 0; index < capture.tileCount; index++) {
      const result = await call({ kind: 'tile', captureId: capture.captureId, index });
      assert.equal(result.isError, false, JSON.stringify(result.structuredContent));
      const tile = result.structuredContent.data.value;
      assert.equal(tile.kind, 'tile');
      assert.equal(tile.captureId, capture.captureId);
      assert.equal(tile.index, index);
      const content = result.content.find((c) => c.type === 'image');
      assert.equal(content?.mimeType, 'image/png');
      const bytes = Buffer.from(content.data, 'base64');
      assert(bytes.length <= 512 * 1024);
      const file = index + '.png';
      await writeFile(path.join(directory, file), bytes, { flag: 'wx' });
      tiles.push({ ...tile, file });
    }
    const manifest = path.join(directory, 'manifest.json');
    await writeFile(manifest, JSON.stringify({ ...capture, tiles }), { flag: 'wx' });
    runDevPython(
      [
        path.join(root, 'scripts/assemble-dev-capture.py'),
        '--manifest',
        manifest,
        '--output',
        path.join(run, name),
      ],
      { cwd: root, windowsHide: true, timeout: 30000, stdio: 'pipe' },
    );
  } catch (error) {
    failure = error;
  }
  try {
    const release = await call({ kind: 'release', captureId: capture.captureId });
    assert.equal(release.isError, false, JSON.stringify(release.structuredContent));
  } catch (error) {
    failure = failure ? new AggregateError([failure, error], '截图及释放均失败') : error;
  }
  if (failure) throw failure;
  return { data: screenshot, capture };
}
