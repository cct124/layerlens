// 独立声明 EngineData 语法与属性预期，不调用候选库的编码或解码。
// 数字只用于核对原始值；合成样本不证明 Photoshop 的单位、字体或颜色视觉保真。
import assert from 'node:assert/strict';

export function encodeEngineData(value) {
  if (typeof value === 'string') {
    const utf16 = Buffer.from(value, 'utf16le').swap16();
    // 在 UTF-16BE 字节层转义括号和反斜线，包括高字节中的这些值。
    const escaped = [...utf16].flatMap((byte) =>
      [40, 41, 92].includes(byte) ? [92, byte] : [byte],
    );
    return Buffer.from([40, 0xfe, 0xff, ...escaped, 41]);
  }
  if (Array.isArray(value)) {
    return Buffer.concat([
      Buffer.from('[ '),
      ...value.flatMap((v) => [encodeEngineData(v), Buffer.from(' ')]),
      Buffer.from(']'),
    ]);
  }
  if (value !== null && typeof value === 'object') {
    return Buffer.concat([
      Buffer.from('<< '),
      ...Object.entries(value).flatMap(([key, v]) => {
        assert(/^[A-Za-z][A-Za-z0-9]*$/.test(key));
        return [Buffer.from('/' + key + ' '), encodeEngineData(v), Buffer.from(' ')];
      }),
      Buffer.from('>>'),
    ]);
  }
  assert(
    value === null ||
      typeof value === 'boolean' ||
      (typeof value === 'number' && Number.isFinite(value)),
  );
  return Buffer.from(String(value));
}

const cr = String.fromCharCode(13);
const lf = String.fromCharCode(10);
const raw = ' 中😀e' + String.fromCharCode(0x301) + cr + lf + '尾  ' + cr;
const character = (properties) => ({ StyleSheet: { StyleSheetData: properties } });
const paragraph = (properties) => ({ ParagraphSheet: { Properties: properties } });
function mixed(extraTerminator = true) {
  return {
    EngineDict: {
      Editor: { Text: raw + (extraTerminator ? cr : '') },
      StyleRun: {
        DefaultRunData: character({
          Font: 1,
          FontSize: 18,
          FillColor: { Type: 1, Values: [1, 0.125, 0.5, 0.75] },
          AutoLeading: false,
          Leading: 22,
          Tracking: -15,
          BaselineShift: 2,
          HorizontalScale: 1.25,
          VerticalScale: 0.75,
          FillFlag: true,
          StrokeFlag: false,
        }),
        RunArray: [
          character({}),
          character({
            Font: 2,
            FontSize: 24,
            FillColor: { Type: 1, Values: [0.5, 1, 0, 0.25] },
            FauxBold: true,
            FauxItalic: false,
          }),
          character({ FontSize: 12 }),
        ],
        RunLengthArray: [2, 4, extraTerminator ? 7 : 6],
      },
      ParagraphRun: {
        DefaultRunData: paragraph({
          Justification: 0,
          AutoLeading: 1.2,
          FirstLineIndent: -2,
          StartIndent: 3,
          EndIndent: 4,
          SpaceBefore: 5,
          SpaceAfter: 6,
        }),
        RunArray: [paragraph({}), paragraph({ Justification: 2, AutoLeading: 1.5, SpaceAfter: 9 })],
        RunLengthArray: [8, extraTerminator ? 5 : 4],
      },
    },
    ResourceDict: {
      FontSet: [
        { Name: 'DO-NOT-DEFAULT' },
        { Name: 'FixtureCJK-Regular', FamilyName: 'Fixture CJK', StyleName: 'Regular' },
        { Name: 'FixtureLatin-Semibold' },
      ],
    },
  };
}

const metric = (value) => ({ value, unit: 'unverifiedEngine' });
const property = (value, source) => ({ value, source });
const dc = 'EngineDict.StyleRun.DefaultRunData.StyleSheet.StyleSheetData.';
const dp = 'EngineDict.ParagraphRun.DefaultRunData.ParagraphSheet.Properties.';
const rc = (index) => 'EngineDict.StyleRun.RunArray[' + index + '].StyleSheet.StyleSheetData.';
const rp = (index) => 'EngineDict.ParagraphRun.RunArray[' + index + '].ParagraphSheet.Properties.';
const baseCharacter = {
  font: property(
    { index: 1, name: 'FixtureCJK-Regular', family: 'Fixture CJK', style: 'Regular' },
    dc + 'Font',
  ),
  fontSize: property(metric(18), dc + 'FontSize'),
  fillColor: property({ alpha: 1, red: 0.125, green: 0.5, blue: 0.75 }, dc + 'FillColor'),
  fillEnabled: property(true, dc + 'FillFlag'),
  strokeEnabled: property(false, dc + 'StrokeFlag'),
  fauxBold: null,
  fauxItalic: null,
  autoLeading: property(false, dc + 'AutoLeading'),
  leading: property(metric(22), dc + 'Leading'),
  tracking: property(metric(-15), dc + 'Tracking'),
  baselineShift: property(metric(2), dc + 'BaselineShift'),
  horizontalScale: property(1.25, dc + 'HorizontalScale'),
  verticalScale: property(0.75, dc + 'VerticalScale'),
};
const baseParagraph = {
  alignment: property('left', dp + 'Justification'),
  autoLeading: property(1.2, dp + 'AutoLeading'),
  firstLineIndent: property(metric(-2), dp + 'FirstLineIndent'),
  startIndent: property(metric(3), dp + 'StartIndent'),
  endIndent: property(metric(4), dp + 'EndIndent'),
  spaceBefore: property(metric(5), dp + 'SpaceBefore'),
  spaceAfter: property(metric(6), dp + 'SpaceAfter'),
};
const mixedExpected = {
  rawText: raw,
  utf16Length: 12,
  indexMapping: 'trailingParagraphTerminator',
  styleRuns: [
    { start: 0, end: 2, style: baseCharacter },
    {
      start: 2,
      end: 6,
      style: {
        ...baseCharacter,
        font: property(
          { index: 2, name: 'FixtureLatin-Semibold', family: null, style: null },
          rc(1) + 'Font',
        ),
        fontSize: property(metric(24), rc(1) + 'FontSize'),
        fillColor: property({ alpha: 0.5, red: 1, green: 0, blue: 0.25 }, rc(1) + 'FillColor'),
        fauxBold: property(true, rc(1) + 'FauxBold'),
        fauxItalic: property(false, rc(1) + 'FauxItalic'),
      },
    },
    {
      start: 6,
      end: 12,
      style: { ...baseCharacter, fontSize: property(metric(12), rc(2) + 'FontSize') },
    },
  ],
  paragraphRuns: [
    { start: 0, end: 8, style: baseParagraph },
    {
      start: 8,
      end: 12,
      style: {
        ...baseParagraph,
        alignment: property('center', rp(1) + 'Justification'),
        autoLeading: property(1.5, rp(1) + 'AutoLeading'),
        spaceAfter: property(metric(9), rp(1) + 'SpaceAfter'),
      },
    },
  ],
};

export const textFixtures = [
  { name: 'mixed', rawText: raw, engineData: mixed(), expected: mixedExpected },
  {
    name: 'exact',
    rawText: raw,
    engineData: mixed(false),
    expected: { ...mixedExpected, indexMapping: 'exact' },
  },
];

function variant(name, change, expected) {
  const data = mixed();
  change(data);
  textFixtures.push({ name, rawText: raw, engineData: data, expected });
}

// 每个错误样本仍是结构可解析的 PSD；验证仅关闭受影响的类别／属性。
for (const [name, lengths] of [
  ['short-coverage', [2, 4, 6]],
  ['long-coverage', [2, 4, 8]],
  ['surrogate-split', [3, 3, 7]],
  ['negative-length', [-1, 7, 7]],
  ['fractional-length', [2.5, 3.5, 7]],
  ['overflow-length', [2, 4, 4294967295]],
  ['zero-length', [0, 6, 7]],
  ['count-mismatch', [13]],
]) {
  variant(
    name,
    (data) => {
      data.EngineDict.StyleRun.RunLengthArray = lengths;
    },
    { styleRuns: null, diagnostic: 'invalidRange', paragraphAvailable: true },
  );
}
variant(
  'bad-paragraph',
  (data) => {
    data.EngineDict.ParagraphRun.RunLengthArray = [7, 5];
  },
  { paragraphRuns: null, diagnostic: 'invalidRange', characterAvailable: true },
);
variant(
  'text-mismatch',
  (data) => {
    data.EngineDict.Editor.Text = raw.replace(cr + lf, lf);
  },
  { styleRuns: null, paragraphRuns: null, diagnostic: 'textMismatch' },
);
variant(
  'missing-font',
  (data) => {
    delete data.EngineDict.StyleRun.DefaultRunData.StyleSheet.StyleSheetData.Font;
  },
  { firstFont: null, diagnostic: 'missing' },
);
variant(
  'invalid-local',
  (data) => {
    data.EngineDict.StyleRun.RunArray[0] = character({
      Font: 99,
      FontSize: 'large',
      FillColor: { Type: 1, Values: [1, 2, 0, 0] },
      FauxBold: 1,
      HorizontalScale: 0,
    });
    data.EngineDict.ParagraphRun.RunArray[0] = paragraph({ Justification: 7, AutoLeading: -1 });
  },
  {
    firstFont: null,
    firstFontSize: null,
    firstColor: null,
    firstAlignment: null,
    diagnostic: 'invalid',
  },
);
variant(
  'unsupported-color',
  (data) => {
    data.EngineDict.StyleRun.RunArray[0] = character({
      FillColor: { Type: 2, Values: [1, 0, 0, 0, 1] },
      Ligatures: true,
    });
  },
  { firstColor: null, diagnostic: 'unsupported' },
);
variant(
  'invalid-container',
  (data) => {
    data.EngineDict.StyleRun.RunArray[0] = { StyleSheet: false };
  },
  { firstFont: null, firstFontSize: null, diagnostic: 'invalid' },
);
variant(
  'style-reference',
  (data) => {
    data.EngineDict.StyleRun.RunArray[0] = { StyleSheet: { StyleSheet: 3 } };
  },
  { diagnostic: 'unsupported' },
);
variant(
  'marker-only-run',
  (data) => {
    data.EngineDict.StyleRun.RunArray.push(character({ Font: 99 }));
    data.EngineDict.StyleRun.RunLengthArray = [2, 4, 6, 1];
  },
  mixedExpected,
);

variant(
  'missing-engine-text',
  (data) => {
    delete data.EngineDict.Editor.Text;
  },
  { styleRuns: null, paragraphRuns: null, diagnostic: 'missing' },
);
variant(
  'invalid-engine-text',
  (data) => {
    data.EngineDict.Editor.Text = false;
  },
  { styleRuns: null, paragraphRuns: null, diagnostic: 'invalid' },
);
variant(
  'missing-style-run',
  (data) => {
    delete data.EngineDict.StyleRun;
  },
  { styleRuns: null, diagnostic: 'missing', paragraphAvailable: true },
);
variant(
  'nondictionary-run',
  (data) => {
    data.EngineDict.StyleRun.RunArray[0] = 0;
  },
  { styleRuns: null, diagnostic: 'invalidRange', paragraphAvailable: true },
);
variant(
  'invalid-font-record',
  (data) => {
    data.ResourceDict.FontSet[1].Name = false;
  },
  { firstFont: null, diagnostic: 'invalid' },
);
variant(
  'missing-font-set',
  (data) => {
    delete data.ResourceDict.FontSet;
  },
  { firstFont: null, diagnostic: 'invalid' },
);

const escaped = String.fromCharCode(40, 41, 92, 0x2800, 0x2900, 0x5c00) + '😀';
for (const [name, value] of [
  ['escaped', escaped],
  ['empty', ''],
]) {
  const data = mixed();
  data.EngineDict.Editor.Text = value + cr;
  data.EngineDict.StyleRun.RunArray = [character({})];
  data.EngineDict.StyleRun.RunLengthArray = [value.length + 1];
  data.EngineDict.ParagraphRun.RunArray = [paragraph({})];
  data.EngineDict.ParagraphRun.RunLengthArray = [value.length + 1];
  textFixtures.push({
    name,
    rawText: value,
    engineData: data,
    expected: {
      rawText: value,
      utf16Length: value.length,
      indexMapping: 'trailingParagraphTerminator',
      styleRuns: value ? [{ start: 0, end: value.length, style: baseCharacter }] : [],
      paragraphRuns: value ? [{ start: 0, end: value.length, style: baseParagraph }] : [],
    },
  });
}
textFixtures.push({
  name: 'absent-tysh-text',
  rawText: null,
  engineData: mixed(),
  expected: { text: null },
});
