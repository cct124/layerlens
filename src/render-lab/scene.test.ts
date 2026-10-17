import { describe, expect, it } from 'vitest';
import { isRenderScene, pathData } from './scene';

const source = () => ({
  sourceSha256: 'a'.repeat(64),
  width: 480,
  height: 320,
  referencePng: [137, 80, 78, 71],
  warnings: [],
  layers: [
    {
      id: 0,
      parentId: null,
      name: 'shape',
      bounds: { x: 2, y: 3, width: 20, height: 30 },
      visible: true,
      opacity: 255,
      compositionWarning: null,
      content: {
        kind: 'path',
        paths: [
          {
            knots: [
              [2, 3, 2, 3, 4, 5],
              [6, 7, 8, 9, 10, 11],
            ],
            evenOdd: false,
          },
        ],
        color: [1, 0, 0, 1],
      },
    },
  ],
});
describe('实验输入边界', () => {
  it('保留原稿路径、负坐标与闭合曲线控制点', () => {
    const value = source();
    expect(isRenderScene(value)).toBe(true);
    value.layers[0]!.bounds.x = -8;
    expect(isRenderScene(value)).toBe(true);
    expect(
      pathData({
        knots: [
          [2, 3, 2, 3, 4, 5],
          [6, 7, 8, 9, 10, 11],
        ],
        evenOdd: false,
      }),
    ).toBe('M 2 3 C 4 5 6 7 8 9 C 10 11 2 3 2 3 Z');
  });
  it('拒绝重复引用、非有限几何与超预算画布', () => {
    const duplicate = source();
    duplicate.layers.push(duplicate.layers[0]!);
    expect(isRenderScene(duplicate)).toBe(false);
    const invalid = source();
    invalid.layers[0]!.content.paths[0]!.knots[0]![0] = Infinity;
    expect(isRenderScene(invalid)).toBe(false);
    const huge = source();
    huge.width = 4096;
    expect(isRenderScene(huge)).toBe(false);
  });
  it('未知内容不会伪装成图层位图', () => {
    const value = source();
    value.layers[0]!.content.kind = 'smartObject';
    expect(isRenderScene(value)).toBe(false);
  });
  it('只有明确 bounded 档位接受规模输入，仍拒绝超出实验上限', () => {
    const value = source();
    value.width = 2000;
    expect(isRenderScene(value)).toBe(false);
    expect(isRenderScene(value, 'bounded')).toBe(true);
    value.layers = Array.from({ length: 2049 }, (_, id) => ({ ...value.layers[0]!, id }));
    expect(isRenderScene(value, 'bounded')).toBe(false);
  });
  it('大稿额度显式启用，仍校验累计像素与诊断字段', () => {
    const value = source();
    value.width = 5047;
    value.height = 3517;
    value.referencePng = [];
    expect(isRenderScene(value, 'bounded')).toBe(false);
    expect(isRenderScene(value, 'large')).toBe(true);
    expect(
      isRenderScene(
        { ...value, layers: [{ ...value.layers[0], compositionWarning: '未实现组合成' }] },
        'largeLayers',
      ),
    ).toBe(true);
    value.layers[0]!.bounds.width = 10000;
    value.layers[0]!.bounds.height = 10000;
    expect(isRenderScene(value, 'large')).toBe(false);
  });
});
