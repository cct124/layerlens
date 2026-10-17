// 规模样本只生成在 .local，不把生成的合成图误作 Photoshop 参考。
import { renderCodec as c } from './generate-psd-fixtures.mjs';
import { renderPrimitives } from './psd-render-fixture.mjs';
import { mkdir, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
const output = fileURLToPath(new URL('../.local/render-corpus/', import.meta.url));
await mkdir(output, { recursive: true });
const manifest = {
  version: 1,
  source: 'LayerLens synthetic PSD, not Photoshop visual truth',
  cases: [],
};
async function save(name, width, height, layers, description) {
  const bytes = Buffer.concat([
    c.header(width, height, 8),
    c.unsigned32(0),
    c.section(c.resolutionResource()),
    c.layerSection(layers, 'raw'),
    c.compositeData('raw', { width, height, rgbaHex: 'ffffffff'.repeat(width * height) }),
  ]);
  const file = name + '.psd';
  await writeFile(output + file, bytes);
  manifest.cases.push({
    file,
    width,
    height,
    layerCount: layers.length,
    bytes: bytes.length,
    sha256: createHash('sha256').update(bytes).digest('hex'),
    description,
  });
}
for (const kind of ['path', 'text', 'bitmap']) {
  for (const count of [100, 500, 2000]) {
    const width = kind === 'text' ? 1000 : 800,
      height = kind === 'text' ? Math.max(800, Math.ceil(count / 20) * 30) : 800;
    const p = renderPrimitives(c, width, height);
    const layers = Array.from({ length: count }, (_, i) => {
      const x = kind === 'text' ? (i % 20) * 50 : (i % 50) * 16;
      const y = kind === 'text' ? Math.floor(i / 20) * 30 + 20 : Math.floor(i / 50) * 16;
      const name = kind + '-' + i;
      if (kind === 'path') return p.rect(name, x, y, 14, 14, [30 + (i % 180), 130, 180]);
      if (kind === 'text')
        return p.text(name, 'T' + i, x, y, [12], undefined, undefined, {
          bounds: p.bounds(x, y - 16, 48, 24),
        });
      return {
        name,
        visible: true,
        bounds: p.bounds(x, y, 14, 14),
        rgbaHex: '3695d7ff'.repeat(14 * 14),
      };
    });
    await save(
      kind + '-' + count,
      width,
      height,
      layers,
      '固定可见区域；部分图层在视口外，所有源节点都实例化',
    );
  }
}
{
  const p = renderPrimitives(c, 750, 8000);
  await save(
    'long-page',
    750,
    8000,
    Array.from({ length: 500 }, (_, i) =>
      p.rect('section-' + i, (i % 5) * 145, Math.floor(i / 5) * 80, 140, 70, [
        40,
        100 + (i % 100),
        180,
      ]),
    ),
    '750x8000 长页面，500 路径',
  );
}
{
  const p = renderPrimitives(c, 1280, 1280);
  await save(
    'large-bitmaps',
    1280,
    1280,
    Array.from({ length: 6 }, (_, i) => ({
      name: 'image-' + i,
      visible: true,
      bounds: p.bounds(i * 20, i * 20, 1024, 1024),
      rgbaHex: (i % 2 ? '3599cc80' : '40bb7880').repeat(1024 * 1024),
    })),
    '6 张 1024x1024 透明位图；压缩友好的像素，不代表照片解码成本',
  );
}
{
  const p = renderPrimitives(c, 600, 400),
    font = 'LayerLensLabCJK-Regular';
  const text = '中文排版测试 English 123，自动换行保持清晰。';
  const mixed = '图层透镜 / LayerLens';
  await save(
    'typography',
    600,
    400,
    [
      p.rect('background', 0, 0, 600, 400, [248, 249, 252]),
      p.text('cjk-paragraph', text, 20, 30, [24], undefined, undefined, {
        font,
        box: [0, 0, 250, 160],
        bounds: p.bounds(20, 30, 250, 160),
      }),
      p.text('mixed-style', mixed, 20, 235, [20, 28], [7, mixed.length - 7], undefined, { font }),
      p.text(
        'rotated',
        '旋转文字 Rotation',
        320,
        90,
        [22],
        undefined,
        [0.94, 0.342, -0.342, 0.94, 320, 90],
        { font, bounds: p.bounds(300, 60, 240, 120) },
      ),
      p.text(
        'latin-wrap',
        'Read only PSD. Native paragraph layout with fixed font.',
        320,
        240,
        [18],
        undefined,
        undefined,
        { box: [0, 0, 220, 140], bounds: p.bounds(320, 240, 220, 140) },
      ),
    ],
    '固定中文子集、分段样式、旋转及真实 EngineData 文字框；无独立 Photoshop 文字参考',
  );
}
{
  const p = renderPrimitives(c, 750, 4800);
  const layers = [p.rect('background', 0, 0, 750, 4800, [248, 249, 252])];
  for (let i = 0; i < 60; i++) {
    const y = i * 80;
    layers.push(p.rect('card-' + i, 12, y + 5, 726, 70, [220, 232, 245]));
    layers.push(
      p.text('heading-' + i, 'LayerLens section ' + i, 40, y + 32, [20], undefined, undefined, {
        bounds: p.bounds(40, y + 8, 450, 30),
      }),
    );
    layers.push({
      name: 'image-' + i,
      visible: true,
      bounds: p.bounds(600, y + 15, 100, 50),
      rgbaHex: '3695d780'.repeat(5000),
    });
  }
  await save(
    'mixed-long-page',
    750,
    4800,
    layers,
    '真实交互用混合长页：60 组文字、路径和透明位图；生成样本不作保真参考',
  );
}
await writeFile(output + 'manifest.json', JSON.stringify(manifest, null, 2) + '\n');
console.log(JSON.stringify({ output, cases: manifest.cases.length }));
