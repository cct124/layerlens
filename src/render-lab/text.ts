/** 共享定位只是实验基线；原生排版保留源分段和源文字框，不能由像素 bounds 猜测。 */
import type { TextData, RenderTextBox } from './generated';
import { FONT_SPECS } from './fonts';
import { cssColor } from './scene';
export type TextRun = {
  text: string;
  size: number;
  leading: number;
  family: string;
  color: string;
  rgba: [number, number, number, number];
};
export type TextSpan = TextRun & { x: number; y: number };
export function prepareText(
  data: TextData,
  box: RenderTextBox | null,
  measure: { font: string; measureText(text: string): Pick<TextMetrics, 'width'> },
): { spans: TextSpan[]; runs: TextRun[] } {
  if (!data.transform || !data.styleRuns) throw new Error('文字缺少变换或源样式区间');
  const runs: TextRun[] = [],
    spans: TextSpan[] = [];
  let covered = 0,
    x = box?.x ?? 0,
    y = box ? box.y : 0;
  for (const run of data.styleRuns) {
    const s = run.style,
      size = s.fontSize?.value.value,
      fill = s.fillColor?.value;
    const font = FONT_SPECS.find((f) => f.name === s.font?.value.name);
    if (!font) throw new Error('缺少源字体：' + (s.font?.value.name ?? '未声明'));
    const paragraphs = data.paragraphRuns?.filter((p) => p.start < run.end && p.end > run.start);
    const ratios = paragraphs?.map((p) => p.style.autoLeading?.value);
    const ratio = ratios?.[0];
    // 只解释源中明确且一致的自动行距比例，不以常见的 120% 代替缺失数据。
    const leading =
      s.autoLeading?.value === true
        ? size && ratio && ratios?.every((r) => r === ratio)
          ? size * ratio
          : undefined
        : s.autoLeading?.value === false
          ? s.leading?.value.value
          : undefined;
    if (
      run.start !== covered ||
      !size ||
      size < 1 ||
      size > 200 ||
      !fill ||
      s.fillEnabled?.value !== true ||
      s.strokeEnabled?.value ||
      s.fauxBold?.value ||
      s.fauxItalic?.value ||
      !leading ||
      leading <= 0 ||
      leading > 500 ||
      paragraphs?.some(
        (p) =>
          (p.style.alignment && p.style.alignment.value !== 'left') ||
          [
            p.style.firstLineIndent,
            p.style.startIndent,
            p.style.endIndent,
            p.style.spaceBefore,
            p.style.spaceAfter,
          ].some((v) => v && v.value.value !== 0),
      ) ||
      (s.tracking?.value.value ?? 0) !== 0 ||
      (s.baselineShift?.value.value ?? 0) !== 0 ||
      (s.horizontalScale?.value ?? 1) !== 1 ||
      (s.verticalScale?.value ?? 1) !== 1
    )
      throw new Error('源文字样式超出当前实验映射范围');
    covered = run.end;
    const text = data.rawText.slice(run.start, run.end).replace(/\r\n|\r/g, '\n');
    if (!font.covers(text)) throw new Error('固定实验字体未覆盖源字形；禁止静默回退');
    const rgba: [number, number, number, number] = [fill.red, fill.green, fill.blue, fill.alpha];
    const prepared = {
      text,
      size,
      family: font.family,
      leading,
      color: cssColor(rgba),
      rgba,
    };
    runs.push(prepared);
    measure.font = size + 'px "' + font.family + '"';
    if (box && runs.length === 1) y += size;
    let part = '',
      start = x;
    const flush = () => {
      if (part) spans.push({ ...prepared, text: part, x: start, y });
      part = '';
    };
    for (const character of text) {
      if (character === '\n') {
        flush();
        x = box?.x ?? 0;
        start = x;
        y += prepared.leading;
        continue;
      }
      const nextWidth = measure.measureText(part + character).width;
      if (box && start + nextWidth > box.x + box.width && (part || x > box.x)) {
        flush();
        x = box.x;
        start = x;
        y += prepared.leading;
      }
      part += character;
      x = start + measure.measureText(part).width;
    }
    flush();
  }
  if (covered !== data.utf16Length) throw new Error('文字样式未覆盖原文');
  return { spans, runs };
}
export const uniformRun = (runs: TextRun[]) => {
  const first = runs[0];
  return first &&
    runs.every(
      (r) =>
        r.family === first.family &&
        r.size === first.size &&
        r.color === first.color &&
        r.leading === first.leading,
    )
    ? { ...first, text: runs.map((r) => r.text).join('') }
    : null;
};
