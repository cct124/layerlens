import { describe, it, expect } from 'vitest';
import { prepareText } from './text';
import type { TextData } from './generated';
const fact = <T>(value: T) => ({ value, source: 'EngineData' });
const metric = (value: number) => fact({ value, unit: 'unverifiedEngine' as const });
const source = (): TextData => ({
  rawText: 'A\rB',
  utf16Length: 3,
  transform: [1, 0, 0, 1, 20, 30],
  normalizationChanged: false,
  styles: { status: 'partial', reason: 'test' },
  indexMapping: 'exact',
  diagnostics: [],
  styleRuns: [
    {
      start: 0,
      end: 3,
      style: {
        font: fact({ index: 0, name: 'NotoSans-Regular', family: null, style: null }),
        fontSize: metric(20),
        fillColor: fact({ red: 0, green: 0, blue: 0, alpha: 1 }),
        fillEnabled: fact(true),
        strokeEnabled: fact(false),
        fauxBold: fact(false),
        fauxItalic: fact(false),
        autoLeading: fact(true),
        leading: metric(0.01),
        tracking: metric(0),
        baselineShift: metric(0),
        horizontalScale: fact(1),
        verticalScale: fact(1),
      },
    },
  ],
  paragraphRuns: [
    {
      start: 0,
      end: 3,
      style: {
        alignment: fact('left'),
        autoLeading: fact(1.2),
        firstLineIndent: metric(0),
        startIndent: metric(0),
        endIndent: metric(0),
        spaceBefore: metric(0),
        spaceAfter: metric(0),
      },
    },
  ],
});
const measure = { font: '', measureText: (text: string) => ({ width: text.length * 8 }) };
describe('实验文字源排版', () => {
  it('显式自动行距覆盖非活动 Leading，点文字从源基线开始', () => {
    const result = prepareText(source(), null, measure);
    expect(result.spans.map((s) => ({ text: s.text, x: s.x, y: s.y, leading: s.leading }))).toEqual(
      [
        { text: 'A', x: 0, y: 0, leading: 24 },
        { text: 'B', x: 0, y: 24, leading: 24 },
      ],
    );
  });
  it('缺失自动行距、未知字体和未支持段落属性不能猜默认值', () => {
    const data = source();
    data.paragraphRuns![0]!.style.autoLeading = null;
    expect(() => prepareText(data, null, measure)).toThrow();
    const aligned = source();
    aligned.paragraphRuns![0]!.style.alignment = fact('right');
    expect(() => prepareText(aligned, null, measure)).toThrow();
    const missing = source();
    missing.styleRuns![0]!.style.font!.value.name = 'AbsentFont';
    expect(() => prepareText(missing, null, measure)).toThrow('缺少源字体');
  });
});
