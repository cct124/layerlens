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

function float64(value) {
  const bytes = Buffer.alloc(8);
  bytes.writeDoubleBE(value);
  return bytes;
}

function float32(value) {
  const bytes = Buffer.alloc(4);
  bytes.writeFloatBE(value);
  return bytes;
}

function unicode(value) {
  const bytes = Buffer.from(value, 'utf16le');
  bytes.swap16();
  return Buffer.concat([unsigned32(value.length), bytes]);
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

function imageResource(id, data) {
  return Buffer.concat([
    ascii('8BIM'),
    unsigned16(id),
    pascalName('', 2),
    unsigned32(data.length),
    padded(data, 2),
  ]);
}

function additionalInfo(key, data) {
  return Buffer.concat([ascii('8BIM'), ascii(key), unsigned32(data.length), padded(data, 2)]);
}

function classId(value) {
  return Buffer.concat([unsigned32(value.length === 4 ? 0 : value.length), ascii(value)]);
}

// Adobe Action Descriptor 的结构编码；各字段和值由下方样本显式给定。
function descriptor(id, entries = []) {
  return Buffer.concat([
    unicode(''),
    classId(id),
    unsigned32(entries.length),
    ...entries.map(([key, type, value]) => Buffer.concat([classId(key), ascii(type), value])),
  ]);
}

function versionedDescriptor(id, entries = []) {
  return Buffer.concat([unsigned32(16), descriptor(id, entries)]);
}

function header(width, height, depth, channelCount = 3) {
  return Buffer.concat([
    ascii('8BPS'),
    unsigned16(1),
    Buffer.alloc(6),
    unsigned16(channelCount),
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
  const channelIds = layer.rgbaHex === undefined ? [] : [-1, 0, 1, 2];
  const channels = channelIds.map((id) =>
    compressedChannel(rgbaChannel(layer.rgbaHex, id === -1 ? 3 : id), width, height, compression),
  );
  const extra = Buffer.concat([
    unsigned32(0),
    unsigned32(0),
    pascalName(layer.name, 4),
    ...(layer.additionalInfo ?? []),
  ]);
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

function layerSection(layers, compression, globalAlpha = false) {
  const encoded = layers.map((layer) => layerRecord(layer, compression));
  const info = padded(
    Buffer.concat([
      signed16(globalAlpha ? -layers.length : layers.length),
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

function compositeData(compression, image = composite, channelCount = 3) {
  const channels = Array.from({ length: channelCount }, (_, channel) =>
    rgbaChannel(image.rgbaHex, channel),
  );
  for (const channel of channels) assert.equal(channel.length, image.width * image.height);
  if (compression === 'raw') return Buffer.concat([unsigned16(0), ...channels]);
  // 合成图 RLE 的长度表覆盖所有通道全部行，不能将三个独立通道块直接拼接。
  const rows = channels.flatMap((bytes) =>
    Array.from({ length: image.height }, (_, row) =>
      Buffer.concat([
        Buffer.from([image.width - 1]),
        bytes.subarray(row * image.width, (row + 1) * image.width),
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

// 合成透明度遵循白底 matte 编码。选 alpha=85/170 使逆变换的人工预期精确可表示；
// 例如 (180 - 255 * (1 - 85/255)) / (85/255) = 30。
const alphaComposite = {
  width: 4,
  height: 1,
  rgbaHex: '204060ff1e3c5a55183654aaffffff00',
};
const alphaStoredComposite = {
  ...alphaComposite,
  rgbaHex: '204060ffb4bec85565798daaffffff00',
};
const groupedLayers = [
  {
    name: 'alpha-art',
    kind: 'bitmap',
    visible: true,
    effectiveVisible: true,
    parentName: null,
    bounds: { x: 0, y: 0, width: 4, height: 1 },
    rgbaHex: alphaComposite.rgbaHex,
  },
  {
    name: 'hidden-child',
    kind: 'bitmap',
    visible: true,
    effectiveVisible: false,
    parentName: 'hidden-folder',
    bounds: { x: 0, y: 0, width: 1, height: 1 },
    rgbaHex: 'ff8000ff',
  },
  {
    name: 'hidden-folder',
    kind: 'group',
    visible: false,
    effectiveVisible: false,
    parentName: null,
    bounds: { x: 0, y: 0, width: 0, height: 0 },
  },
];
const unknownResource = Buffer.from('1c0205000141', 'hex');
// 故意不声明为有效 ICC；只验证资源作为有界 opaque 字节被保留并报告颜色限制。
const opaqueProfile = ascii('ICC-unvalidated');

function groupsAlphaPsd(compression) {
  const records = [
    groupedLayers[0],
    {
      name: '</Layer group>',
      visible: true,
      bounds: { x: 0, y: 0, width: 0, height: 0 },
      additionalInfo: [additionalInfo('lsct', unsigned32(3))],
    },
    groupedLayers[1],
    {
      ...groupedLayers[2],
      additionalInfo: [additionalInfo('lsct', unsigned32(1))],
    },
  ];
  return Buffer.concat([
    header(4, 1, 8, 4),
    unsigned32(0),
    section(
      Buffer.concat([
        imageResource(1028, unknownResource),
        imageResource(1039, opaqueProfile),
        resolutionResource(),
      ]),
    ),
    layerSection(records, compression, true),
    compositeData(compression, alphaStoredComposite, 4),
  ]);
}

const rawText = ' 中😀e\u0301\r\n尾  \r';
const textTransform = [1.5, 0.25, -0.5, 2, -3, 7];
function typeToolData() {
  return Buffer.concat([
    signed16(1),
    ...textTransform.map(float64),
    signed16(50),
    versionedDescriptor('TxLr', [['Txt ', 'TEXT', unicode(rawText)]]),
    signed16(1),
    versionedDescriptor('warp'),
    ...[0, 0, 1, 1].map(float32),
  ]);
}

function placedLayerData() {
  return Buffer.concat([
    ascii('plcL'),
    signed32(3),
    pascalName('fixture-object', 1),
    ...[1, 1, 16, 2].map(signed32),
    ...[0, 0, 1, 0, 1, 1, 0, 1].map(float64),
    signed32(0),
    versionedDescriptor('warp'),
  ]);
}

// 渐变内容不承诺可渲染；这是结构有效的最小 descriptor，用于验证已知能力缺口。
const gradientDescriptor = versionedDescriptor('null', [['Grad', 'Objc', descriptor('Grdn')]]);
const typedLayerDefinitions = [
  ['unsupported-gradient', 'shape', 'GdFl', gradientDescriptor],
  ['literal-text', 'text', 'TySh', typeToolData()],
  ['placed-object', 'smartObject', 'PlLd', placedLayerData()],
  [
    'brightness-adjustment',
    'adjustment',
    'brit',
    Buffer.concat([signed16(10), signed16(-5), signed16(127), Buffer.from([0])]),
  ],
  ['unknown-semantic-tag', 'unknown', 'zzzz', Buffer.from('aabbccdd', 'hex')],
];
const typedLayers = typedLayerDefinitions.map(([name, kind]) => ({
  name,
  kind,
  visible: true,
  bounds: { x: 0, y: 0, width: 1, height: 1 },
  rgbaHex: 'ffffffff',
}));
const typedRecords = typedLayers.map((layer, index) => ({
  ...layer,
  additionalInfo: [
    additionalInfo(typedLayerDefinitions[index][2], typedLayerDefinitions[index][3]),
  ],
}));
const metadataComposite = { width: 1, height: 1, rgbaHex: 'ffffffff' };

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
  ...['raw', 'rle'].map((compression) => ({
    file: `resources-groups-alpha-${compression}.psd`,
    compression,
    bytes: groupsAlphaPsd(compression),
    document: { ...document, width: 4, height: 1 },
    layers: groupedLayers,
    composite: alphaComposite,
    storedComposite: alphaStoredComposite,
    resources: [
      { id: 1028, dataHex: unknownResource.toString('hex') },
      { id: 1039, dataHex: opaqueProfile.toString('hex'), profileValidated: false },
      { id: 1005, resolutionDpi: 72 },
    ],
    layerRecordCount: 4,
    globalAlpha: true,
    note: '非空未知图像资源、奇数长度 opaque ICC、资源后续读取、隐藏父组和合成透明度。',
  })),
  {
    file: 'metadata-text.psd',
    compression: 'raw',
    bytes: Buffer.concat([
      header(1, 1, 8),
      unsigned32(0),
      section(resolutionResource()),
      layerSection(typedRecords, 'raw'),
      compositeData('raw', metadataComposite),
    ]),
    document: { ...document, width: 1, height: 1 },
    layers: typedLayers,
    composite: metadataComposite,
    text: { rawText, utf16Length: rawText.length, transform: textTransform, styleRuns: null },
    additionalTags: typedLayerDefinitions.map(([name, , key]) => ({ layerName: name, key })),
    note: '最小 descriptor 不含 EngineData；验证原文和仿射矩阵，不声明字体/字号/分段样式保真。',
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
