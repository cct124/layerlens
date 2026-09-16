import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// 只编码本文件声明的极小样本；不调用待验证的 PSD 解析器或写入器。
// 字段布局依据 Adobe Photoshop File Formats Specification 的 PSD v1 章节。
const output = resolve(
  dirname(fileURLToPath(import.meta.url)),
  '../crates/layerlens-core/tests/fixtures/psd',
);
const args = process.argv.slice(2);
assert(args.length === 0 || (args.length === 1 && args[0] === '--check'), '仅支持 --check');
const check = args[0] === '--check';

function unsigned16(value) {
  const bytes = Buffer.alloc(2);
  bytes.writeUInt16BE(value);
  return bytes;
}

function signed16(value) {
  const bytes = Buffer.alloc(2);
  bytes.writeInt16BE(value);
  return bytes;
}

function unsigned32(value) {
  const bytes = Buffer.alloc(4);
  bytes.writeUInt32BE(value);
  return bytes;
}

function signed32(value) {
  const bytes = Buffer.alloc(4);
  bytes.writeInt32BE(value);
  return bytes;
}

function ascii(value) {
  return Buffer.from(value, 'ascii');
}

function padded(bytes, alignment) {
  return Buffer.concat([bytes, Buffer.alloc((alignment - (bytes.length % alignment)) % alignment)]);
}

function section(bytes) {
  return Buffer.concat([unsigned32(bytes.length), bytes]);
}

function pascalName(name, alignment) {
  const bytes = ascii(name);
  assert(bytes.length <= 255);
  return padded(Buffer.concat([Buffer.from([bytes.length]), bytes]), alignment);
}

function resolutionResource() {
  // Resource 1005: 水平/垂直 16.16 fixed DPI，分辨率单位 1=pixels/inch，尺寸单位 1=inches。
  const data = Buffer.concat([
    unsigned32(72 * 65536),
    unsigned16(1),
    unsigned16(1),
    unsigned32(72 * 65536),
    unsigned16(1),
    unsigned16(1),
  ]);
  return Buffer.concat([ascii('8BIM'), unsigned16(1005), pascalName('', 2), section(data)]);
}

function header(width, height, depth) {
  return Buffer.concat([
    ascii('8BPS'),
    unsigned16(1),
    Buffer.alloc(6),
    unsigned16(3),
    unsigned32(height),
    unsigned32(width),
    unsigned16(depth),
    unsigned16(3),
  ]);
}

function rgbaChannel(rgbaHex, channel) {
  const rgba = Buffer.from(rgbaHex, 'hex');
  assert.equal(rgba.length % 4, 0);
  return Buffer.from(
    Array.from({ length: rgba.length / 4 }, (_, index) => rgba[index * 4 + channel]),
  );
}

function compressedChannel(bytes, width, height, compression) {
  assert.equal(bytes.length, width * height);
  if (compression === 'raw') return Buffer.concat([unsigned16(0), bytes]);

  // PSD RLE 使用逐行 PackBits；此处小于 128 像素的行只用合法的 literal run。
  // 每行先写长度表，再写压缩数据，避免依赖候选库的压缩逻辑。
  assert(width > 0 && width <= 128);
  const rows = Array.from({ length: height }, (_, row) =>
    Buffer.concat([Buffer.from([width - 1]), bytes.subarray(row * width, (row + 1) * width)]),
  );
  return Buffer.concat([unsigned16(1), ...rows.map((row) => unsigned16(row.length)), ...rows]);
}

function layerRecord(layer, compression) {
  const { x, y, width, height } = layer.bounds;
  // -1 是透明度通道；0/1/2 分别为 R/G/B。通道长度包含两字节 compression。
  const channelIds = [-1, 0, 1, 2];
  const channels = channelIds.map((id) =>
    compressedChannel(rgbaChannel(layer.rgbaHex, id === -1 ? 3 : id), width, height, compression),
  );
  const extra = Buffer.concat([unsigned32(0), unsigned32(0), pascalName(layer.name, 4)]);
  const record = Buffer.concat([
    signed32(y),
    signed32(x),
    signed32(y + height),
    signed32(x + width),
    unsigned16(channelIds.length),
    ...channelIds.map((id, index) =>
      Buffer.concat([signed16(id), unsigned32(channels[index].length)]),
    ),
    ascii('8BIM'),
    ascii('norm'),
    Buffer.from([255, 0, layer.visible ? 0 : 2, 0]),
    section(extra),
  ]);
  return { record, channels };
}

function layerSection(layers, compression) {
  const encoded = layers.map((layer) => layerRecord(layer, compression));
  const info = padded(
    Buffer.concat([
      signed16(layers.length),
      ...encoded.map((layer) => layer.record),
      ...encoded.flatMap((layer) => layer.channels),
    ]),
    2,
  );
  return section(Buffer.concat([section(info), unsigned32(0)]));
}

const document = { width: 4, height: 3, depth: 8, colorMode: 'RGB', resolutionDpi: 72 };
// 显式定义人工预期，不从编码字节或任何解析器反推；文件图层记录按底到顶排列。
const layers = [
  {
    name: 'negative-origin',
    visible: true,
    bounds: { x: -1, y: 1, width: 3, height: 2 },
    rgbaHex: 'ff0000ff00ff00800000ff00ffff00ff00ffffffff00ff80',
  },
  {
    name: 'hidden-overlay',
    visible: false,
    bounds: { x: 1, y: 0, width: 1, height: 1 },
    rgbaHex: 'ff8000ff',
  },
];
// 保存时合成图是白底上的可见内容：负坐标像素被画布裁掉；隐藏图层不参与。
// 128/255 alpha 的绿色/品红覆盖白底后，未覆盖分量精确为 127。
const composite = {
  width: 4,
  height: 3,
  rgbaHex:
    'ffffffffffffffffffffffffffffffff' +
    '7fff7fffffffffffffffffffffffffff' +
    '00ffffffff7fffffffffffffffffffff',
};

function compositeData(compression) {
  const channels = [0, 1, 2].map((channel) => rgbaChannel(composite.rgbaHex, channel));
  for (const channel of channels) assert.equal(channel.length, document.width * document.height);
  if (compression === 'raw') return Buffer.concat([unsigned16(0), ...channels]);
  // 合成图 RLE 的长度表覆盖所有通道全部行，不能将三个独立通道块直接拼接。
  const rows = channels.flatMap((bytes) =>
    Array.from({ length: document.height }, (_, row) =>
      Buffer.concat([
        Buffer.from([document.width - 1]),
        bytes.subarray(row * document.width, (row + 1) * document.width),
      ]),
    ),
  );
  return Buffer.concat([unsigned16(1), ...rows.map((row) => unsigned16(row.length)), ...rows]);
}

function bitmapPsd(compression, includeComposite = true) {
  return Buffer.concat([
    header(document.width, document.height, document.depth),
    unsigned32(0),
    section(resolutionResource()),
    layerSection(layers, compression),
    ...(includeComposite ? [compositeData(compression)] : []),
  ]);
}

const cases = [
  {
    file: 'bitmap-raw.psd',
    compression: 'raw',
    bytes: bitmapPsd('raw'),
    document,
    layers,
    composite,
  },
  {
    file: 'bitmap-rle.psd',
    compression: 'rle',
    bytes: bitmapPsd('rle'),
    document,
    layers,
    composite,
  },
  {
    file: 'bitmap-no-composite.psd',
    compression: 'raw',
    bytes: bitmapPsd('raw', false),
    document,
    layers,
    composite: null,
    note: '完整图层与蒙版节后 EOF；故意省略 Image Data，验证元数据与缺失预览的错误边界。',
  },
  {
    file: 'rgb16.psd',
    compression: 'raw',
    bytes: Buffer.concat([
      header(1, 1, 16),
      unsigned32(0),
      section(resolutionResource()),
      unsigned32(0),
      unsigned16(0),
      Buffer.from('1234abcd00ff', 'hex'),
    ]),
    document: { ...document, width: 1, height: 1, depth: 16 },
    layers: [],
    composite: null,
    rgb16Hex: '1234abcd00ff',
    note: '合法 RGB/16 位输入；合成图为 R=0x1234、G=0xabcd、B=0x00ff，不伪造 RGBA8 预期。',
  },
];

const manifest = {
  schemaVersion: 1,
  source: 'LayerLens 人工定义并按 Adobe PSD v1 规范编码的合成样本',
  license: 'MIT',
  pixelOrder: 'rgbaHex 使用行优先 RGBA8，不预乘 alpha；图层像素保留透明边距和画布外内容。',
  layerOrder: 'bottom-to-top',
  samples: cases.map(({ bytes, ...sample }) => ({
    ...sample,
    sizeBytes: bytes.length,
    sha256: createHash('sha256').update(bytes).digest('hex'),
  })),
};

const files = [
  ...cases.map(({ file, bytes }) => [file, bytes]),
  ['manifest.json', Buffer.from(`${JSON.stringify(manifest, null, 2)}\n`)],
];
if (!check) mkdirSync(output, { recursive: true });
for (const [file, bytes] of files) {
  const path = resolve(output, file);
  if (check) {
    assert(readFileSync(path).equals(bytes), `${file} 与生成规则不一致；请检查 diff 后重新生成。`);
  } else {
    writeFileSync(path, bytes);
  }
}
console.log(`${check ? '已核对' : '已生成'} ${cases.length} 个 PSD 样本及 manifest.json。`);
