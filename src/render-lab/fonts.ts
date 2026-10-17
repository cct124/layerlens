/** 固定实验字体，不把任意 PSD 字体静默替换为测试字体。 */
import latinUrl from './assets/NotoSans.ttf?url';
import cjkUrl from './assets/LayerLensLabCJK.otf?url';
import localFonts from 'virtual:render-lab-fonts';
export const FONT_FAMILY = 'LayerLens Lab Noto';
export const FONT_SHA = 'bfb7bb691513f12e734dc346c03a03f784912432d7e3fa8e56efcf906fe86b3d';
const chinese = new Set(
  '图层透镜中文排版测试自动换行旋转文字分段颜色字体混合保持清晰只读原稿性能比较路径像素，。',
);
export const FONT_SPECS = [
  ...localFonts.map((font) => {
    const covered = new Set(font.codepoints);
    return {
      ...font,
      covers: (s: string) =>
        [...s].every((c) => c === '\r' || c === '\n' || covered.has(c.codePointAt(0) ?? -1)),
    };
  }),
  {
    name: 'NotoSans-Regular',
    family: FONT_FAMILY,
    url: latinUrl,
    sha: FONT_SHA,
    covers: (s: string) => /^[\x20-\x7e\r\n]*$/.test(s),
  },
  {
    name: 'LayerLensLabCJK-Regular',
    family: 'LayerLens Lab CJK',
    url: cjkUrl,
    sha: 'b7113b5364f678f38d5abcea8d7a8fe315b039f3e7a6a6d85b534632e83afdfd',
    covers: (s: string) => [...s].every((c) => /^[\x20-\x7e\r\n]$/.test(c) || chinese.has(c)),
  },
];
/** 加载后只返回指纹；FontFace 生命周期由调用方持有，不再为已移除的引擎保留原始字节。 */
export async function loadFonts(names: Set<string>, faces: FontFace[]): Promise<string[]> {
  const result: string[] = [];
  for (const spec of FONT_SPECS.filter((s) => names.has(s.name))) {
    const response = await fetch(spec.url);
    if (!response.ok) throw new Error('实验字体读取失败：' + spec.name);
    const bytes = await response.arrayBuffer();
    const hash = Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256', bytes)), (n) =>
      n.toString(16).padStart(2, '0'),
    ).join('');
    if (hash !== spec.sha) throw new Error('实验字体指纹不符：' + spec.name);
    const face = new FontFace(spec.family, bytes);
    await face.load();
    document.fonts.add(face);
    faces.push(face);
    result.push(hash);
  }
  return result;
}
