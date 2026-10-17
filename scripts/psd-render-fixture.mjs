// 人工定义的 PSD 字段与几何，独立于被测解析器和三个渲染器。合成图不包含文字。
export function renderPrimitives(c, width, height) {
  const bounds = (x, y, w, h) => ({ x, y, width: w, height: h });
  const knot = (x, y) => [x, y, x, y, x, y];
  function vector(name, box, points, color, visible = true) {
    const lengths = Buffer.concat([
      c.unsigned16(0),
      c.unsigned16(points.length),
      c.signed16(-1),
      c.unsigned16(2),
      Buffer.alloc(18),
    ]);
    const records = points.map((p) =>
      Buffer.concat([
        c.unsigned16(1),
        ...[
          p[1] / height,
          p[0] / width,
          p[3] / height,
          p[2] / width,
          p[5] / height,
          p[4] / width,
        ].map((n) => c.signed32(Math.round(n * 2 ** 24))),
      ]),
    );
    return {
      name,
      visible,
      bounds: box,
      additionalInfo: [
        c.additionalInfo(
          'vmsk',
          Buffer.concat([c.unsigned32(3), c.unsigned32(0), lengths, ...records]),
        ),
        c.additionalInfo(
          'SoCo',
          c.versionedDescriptor('null', [
            [
              'Clr ',
              'Objc',
              c.descriptor(
                'RGBC',
                ['Rd  ', 'Grn ', 'Bl  '].map((key, i) => [key, 'doub', c.float64(color[i])]),
              ),
            ],
          ]),
        ),
      ],
    };
  }
  const rect = (name, x, y, w, h, color, visible = true) =>
    vector(
      name,
      bounds(x, y, w, h),
      [knot(x, y), knot(x + w, y), knot(x + w, y + h), knot(x, y + h)],
      color,
      visible,
    );
  function text(
    name,
    value,
    x,
    y,
    sizes = [24],
    lengths = [value.length],
    transform = [1, 0, 0, 1, x, y],
    options = {},
  ) {
    const style = (size, index = 0) => ({
      StyleSheet: {
        StyleSheetData: {
          Font: 0,
          FontSize: size,
          FillColor: { Type: 1, Values: index ? [1, 0.1, 0.42, 0.8] : [1, 0.12, 0.16, 0.24] },
          FillFlag: true,
          StrokeFlag: false,
          FauxBold: false,
          FauxItalic: false,
          AutoLeading: false,
          Leading: 30,
          Tracking: 0,
          BaselineShift: 0,
          HorizontalScale: 1,
          VerticalScale: 1,
        },
      },
    });
    const engine = {
      EngineDict: {
        Editor: { Text: value },
        StyleRun: {
          DefaultRunData: style(sizes[0]),
          RunArray: sizes.map(style),
          RunLengthArray: lengths,
        },
        ParagraphRun: {
          RunArray: [{ ParagraphSheet: { Properties: { Justification: 0 } } }],
          RunLengthArray: [value.length],
        },
      },
      ResourceDict: {
        FontSet: [
          {
            Name: options.font ?? 'NotoSans-Regular',
            FamilyName: options.font ?? 'Noto Sans',
            StyleName: 'Regular',
          },
        ],
      },
    };
    if (options.box)
      engine.EngineDict.Rendered = {
        Shapes: { Children: [{ Cookie: { Photoshop: { ShapeType: 1, BoxBounds: options.box } } }] },
      };
    return {
      name,
      visible: true,
      bounds: options.bounds ?? bounds(x, y - 26, 400, 64),
      additionalInfo: [c.additionalInfo('TySh', c.typeToolData(value, engine, transform))],
    };
  }
  return { bounds, knot, vector, rect, text };
}

export function renderFixture(c) {
  const width = 480,
    height = 320;
  const { bounds, vector, rect, text } = renderPrimitives(c, width, height);
  const bitmap = Buffer.alloc(80 * 64 * 4);
  for (let y = 0; y < 64; y++)
    for (let x = 0; x < 80; x++) {
      const p = (y * 80 + x) * 4;
      bitmap.set([240, 105, 50, x >= 24 && x < 56 && y >= 16 && y < 48 ? 0 : 255], p);
    }
  const groupBounds = bounds(0, 0, 0, 0);
  const records = [
    rect('background', 0, 0, width, height, [248, 249, 252]),
    rect('blue-card', 24, 170, 150, 105, [40, 115, 210]),
    rect('overlap-card', 125, 215, 130, 65, [32, 175, 135]),
    {
      name: 'alpha-window',
      visible: true,
      bounds: bounds(48, 190, 80, 64),
      rgbaHex: bitmap.toString('hex'),
    },
    vector(
      'curve',
      bounds(290, 180, 130, 90),
      [
        [280, 210, 300, 200, 315, 165],
        [360, 160, 390, 190, 425, 225],
        [425, 265, 380, 270, 330, 280],
        [285, 270, 290, 235, 285, 220],
      ],
      [180, 95, 205],
    ),
    text('title', 'LayerLens render lab', 24, 48, [26]),
    text('mixed', 'Same PSD / three engines', 24, 91, [19, 24], [11, 13]),
    text('multiline', 'Fonts, paths and pixels\rRead-only experiment', 24, 125, [16]),
    {
      name: '</Layer group>',
      visible: true,
      bounds: groupBounds,
      additionalInfo: [c.additionalInfo('lsct', c.unsigned32(3))],
    },
    rect('hidden-child', 0, 0, 50, 50, [255, 0, 0]),
    {
      name: 'hidden-group',
      visible: false,
      bounds: groupBounds,
      additionalInfo: [c.additionalInfo('lsct', c.unsigned32(1))],
    },
    rect('hidden-leaf', 450, 0, 30, 30, [255, 0, 0], false),
  ];
  // 独立的整数矩形/alpha 合成预期；曲线与文字缺少独立排版参考，明确不画入。
  const image = Buffer.alloc(width * height * 4);
  for (let y = 0; y < height; y++)
    for (let x = 0; x < width; x++) {
      let color = [248, 249, 252, 255];
      if (x >= 24 && x < 174 && y >= 170 && y < 275) color = [40, 115, 210, 255];
      if (x >= 125 && x < 255 && y >= 215 && y < 280) color = [32, 175, 135, 255];
      if (x >= 48 && x < 128 && y >= 190 && y < 254) {
        const p = ((y - 190) * 80 + x - 48) * 4;
        if (bitmap[p + 3]) color = [240, 105, 50, 255];
      }
      image.set(color, (y * width + x) * 4);
    }
  return {
    file: 'render-simple.psd',
    compression: 'raw',
    bytes: Buffer.concat([
      c.header(width, height, 8),
      c.unsigned32(0),
      c.section(c.resolutionResource()),
      c.layerSection(records, 'raw'),
      c.compositeData('raw', { width, height, rgbaHex: image.toString('hex') }),
    ]),
    document: { width, height, depth: 8, colorMode: 'RGB', resolutionDpi: 72 },
    renderLab: {
      layerCount: 11,
      font: 'NotoSans-Regular',
      fontSizeUnit: 'unverifiedEngine: fixture uses pixel-valued metrics at 72 DPI',
      recordNames: records.map((r) => r.name),
      opaqueHit: [55, 200],
      transparentHit: [85, 220],
      overlapHit: [140, 230],
      referenceIncludes: ['background', 'blue-card', 'overlap-card', 'alpha-window'],
    },
    note: '工程生成样本，无 Photoshop 独立参考；合成图只包含矩形与 alpha 位图，不包含文字和曲线。中文字体、旋转、复杂蒙版和真实 DPI 待补独立样本。',
  };
}
