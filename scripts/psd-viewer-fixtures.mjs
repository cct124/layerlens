// 查看器规模样本的人工输入，不从解析结果计算预期；不代表真实字体或排版。
const character = (properties) => ({ StyleSheet: { StyleSheetData: properties } });
const paragraph = (properties) => ({ ParagraphSheet: { Properties: properties } });

function textFixture(name, rawText, lengths, fontName) {
  return {
    name,
    rawText,
    engineData: {
      EngineDict: {
        Editor: { Text: rawText },
        StyleRun: {
          DefaultRunData: character({
            Font: 0,
            FontSize: 18,
            FillColor: { Type: 1, Values: [1, 0.125, 0.5, 0.75] },
            FillFlag: true,
            StrokeFlag: false,
            FauxBold: false,
            FauxItalic: false,
            AutoLeading: false,
            Leading: 22,
            Tracking: 0,
            BaselineShift: 0,
            HorizontalScale: 1,
            VerticalScale: 1,
          }),
          RunArray: lengths.map((_, index) => character({ FontSize: 18 + (index % 2) })),
          RunLengthArray: lengths,
        },
        ParagraphRun: {
          RunArray: [
            paragraph({
              Justification: 0,
              AutoLeading: 1.2,
              FirstLineIndent: 0,
              StartIndent: 0,
              EndIndent: 0,
              SpaceBefore: 0,
              SpaceAfter: 0,
            }),
          ],
          RunLengthArray: [rawText.length],
        },
      },
      ResourceDict: { FontSet: [{ Name: fontName, FamilyName: 'Fixture', StyleName: 'Regular' }] },
    },
    expected: {
      rawText,
      utf16Length: rawText.length,
      styleRunLengths: lengths,
      fontName,
      fontSizeAlternating: [18, 19],
      paragraphRunCount: 1,
      indexMapping: 'exact',
    },
  };
}

// 2048 边界落在代理对内部，首段必须退到 2047；保留 CR/LF 和尾部空格。
const cr = String.fromCharCode(13);
const longText = ('a😀b' + cr + String.fromCharCode(10)).repeat(800) + '尾  ' + cr;
export const viewerTextFixtures = [
  textFixture('long-text', longText, [longText.length], 'FixtureLongText'),
  textFixture('many-style-runs', '😀ab'.repeat(300), Array(300).fill(4), 'FixtureManyRuns'),
  // 字体字符串只存一份，但每个样式响应都携带它；每段 <32 KiB，合计 >256 KiB。
  // 多字节及 JSON 转义字符防止以字符串长度代替实际响应字节数。
  textFixture(
    'large-styles',
    '😀ab'.repeat(100),
    Array(100).fill(4),
    ('字"' + String.fromCharCode(92)).repeat(1100),
  ),
];

export const viewerGrid = { columns: 32, rows: 16, tileSize: 8 };
