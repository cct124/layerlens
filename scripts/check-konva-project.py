"""Konva 大稿截图与独立参考诊断；源图层投影不能由相似分数批准完整 PSD 保真。"""
import argparse
import hashlib
import json
import math
from pathlib import Path
from PIL import Image

parser = argparse.ArgumentParser()
parser.add_argument('--suite', type=Path, required=True)
parser.add_argument('--output', type=Path, help='另存只读复核结果；目标须尚不存在，不覆盖原运行报告')
args = parser.parse_args()
root = Path(__file__).resolve().parent.parent
manifest = json.loads((root / '.local/render-tab1/manifest.json').read_text(encoding='utf8'))
ref_path = root / '.local/render-tab1' / manifest['reference']
if hashlib.sha256(ref_path.read_bytes()).hexdigest() != manifest['referenceSha256']:
    raise ValueError('独立参考指纹不符')
reference = Image.open(ref_path).convert('RGBA')
summary = json.loads((args.suite / 'summary.json').read_text(encoding='utf8'))
if summary.get('kind') != 'konva-regression' or summary.get('workload') != 'project':
    raise ValueError('只接受 Konva 真实大稿视觉／交互套件')
output = {'sourceSha256': manifest['sourceSha256'], 'referenceSha256': manifest['referenceSha256'],
          'fullSceneAccepted': False, 'diagnosticOnly': True, 'reports': [],
          'limitations': ['截图只比较可见区域；采样不校正 ICC 或重采样。',
                         '原生与位图文字分别按 MCP 覆盖清单解释；位图诊断不能代表原生文字通过。']}
expected_images = {'project-konva-fit.png'} | {
    f'project-konva-detail{index}-{zoom}.png'
    for index in range(3) for zoom in (100, 200, 400)
}
for entry in summary['results']:
    for name in entry['runs']:
        directory = args.suite / 'runs' / name
        report = json.loads((directory / 'report.json').read_text(encoding='utf8'))
        if report['sourceHashes'][0] != manifest['sourceSha256']:
            raise ValueError('报告源文件身份不符')
        row = {'run': name, 'functionalPassed': report['passed'], 'images': []}
        for capture in report['images']:
            if not capture['name'].startswith('project-') or capture.get('profile') != 'largeLayers':
                continue
            if capture['engine'] != 'konva' or not capture['unchanged'] or capture['sourceSha256'] != manifest['sourceSha256']:
                raise ValueError('截图在变化或源身份不符')
            rgba = Image.open(directory / capture['name']).convert('RGBA')
            if hashlib.sha256(rgba.tobytes()).hexdigest() != capture['rgbaSha256']:
                raise ValueError('保留截图与捕获 RGBA 指纹不符')
            screenshot = rgba.convert('RGB')
            canvas = capture['canvas']
            viewport, image = canvas['viewport'], canvas['image']
            dpr, zoom = capture['dpr'], canvas['zoom']
            counts = differing = total = peak = 0
            # 固定 CSS 网格，比较相同屏幕采样密度；排除视口边缘与原稿外区域。
            for sy in range(math.ceil(viewport['y'])+3, math.floor(viewport['y']+viewport['height'])-3, 3):
                for sx in range(math.ceil(viewport['x'])+3, math.floor(viewport['x']+viewport['width'])-3, 3):
                    x, y = math.floor((sx-image['x'])/zoom), math.floor((sy-image['y'])/zoom)
                    px, py = math.floor(sx*dpr), math.floor(sy*dpr)
                    if not (0 <= x < reference.width and 0 <= y < reference.height and 0 <= px < screenshot.width and 0 <= py < screenshot.height):
                        continue
                    color = reference.getpixel((x,y))
                    expected = [round(color[i]*color[3]/255 + (50,59,73)[i]*(1-color[3]/255)) for i in range(3)]
                    actual = screenshot.getpixel((px,py))
                    differences = [abs(actual[i]-expected[i]) for i in range(3)]
                    counts += 1
                    total += sum(differences)
                    peak = max(peak, max(differences))
                    differing += max(differences) > 8
            if not counts:
                raise ValueError('没有可比较的原稿区域')
            row['images'].append({'name': capture['name'], 'screenSamples': counts,
                                  'rgbMAE': total/(counts*3), 'maxChannelError': peak, 'fractionOver8': differing/counts})
        # 性能门槛失败不代表缺图；截图完整性与原功能结果分别保留。
        row['capturesComplete'] = (len(row['images']) == 10
                                   and {image['name'] for image in row['images']} == expected_images)
        if args.output is None:
            (directory / 'project-visual.json').write_text(json.dumps(row, ensure_ascii=False, indent=2)+'\n', encoding='utf8')
        output['reports'].append(row)
path = args.output or args.suite / 'project-visual.json'
output['capturesComplete'] = bool(output['reports']) and all(row['capturesComplete'] for row in output['reports'])
output['functionalPassed'] = bool(output['reports']) and all(row['functionalPassed'] for row in output['reports'])
with path.open('x' if args.output else 'w', encoding='utf8') as stream:
    stream.write(json.dumps(output, ensure_ascii=False, indent=2)+'\n')
print(json.dumps({'path': str(path), 'runs':len(output['reports']),
                  'capturesComplete':output['capturesComplete'], 'functionalPassed':output['functionalPassed'],
                  'fullSceneAccepted':False}, ensure_ascii=False))
if not output['capturesComplete']:
    raise ValueError('诊断截图不完整，报告已保留但对照未完成')
if not output['functionalPassed']:
    raise ValueError('诊断截图齐全，但原功能／性能回归未通过；不改判原结果')
