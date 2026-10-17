// 普通构建隔离 Konva 验证入口；验证构建也不得残留未选引擎与 WASM。
import assert from 'node:assert/strict';
import { readdir, readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
assert(
  process.argv.slice(2).every((arg) => arg === '--render-lab'),
  '未知构建检查参数',
);
const lab = process.argv.includes('--render-lab');
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  lab ? '../dist-render-lab' : '../dist',
);
let files = 0;
async function visit(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const file = path.join(dir, entry.name);
    assert(!entry.isSymbolicLink(), '构建产物包含链接');
    if (entry.isDirectory()) {
      await visit(file);
      continue;
    }
    assert(
      !(
        lab
          ? /canvaskit|leafer|\.wasm$/i
          : /render-lab|canvaskit|leafer|konva|NotoSans|LayerLensLabCJK|\.wasm$|\.ttf$|\.otf$/i
      ).test(entry.name),
      '构建包含应隔离的资源：' + entry.name,
    );
    if (/\.(js|html)$/.test(entry.name))
      assert(
        !(
          lab
            ? /CanvasKitInit|canvaskit-wasm|leafer-ui/
            : /read_render_scene|read_render_resource|CanvasKitInit|LayerLens Lab Noto|\/renderers\//
        ).test(await readFile(file, 'utf8')),
        '构建包含应隔离的代码：' + entry.name,
      );
    files++;
  }
}
await visit(root);
assert(files > 0);
console.log(
  (lab ? 'Konva 验证构建无 Leafer／CanvasKit／WASM：' : '普通构建隔离通过：') + files + ' 个文件。',
);
