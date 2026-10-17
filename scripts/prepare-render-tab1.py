"""私有大稿参考与显式字体：源稿只读，产物仅进入 .local，不上传 PSD。"""
import argparse
import hashlib
import json
from pathlib import Path
import sys
import urllib.request

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / '.local/render-reference-tools'))
sys.path.insert(0, str(ROOT / '.local/render-fonttools'))
COMMIT = '7889f11bf31170b5d092a083b357c8c8130f89e0'
FILES = [
    ('SourceHanSerifTC-Bold.otf', 25520548, '6ebf9cb4e49f495380a62c115ba5c7c4c74d1970'),
    ('SourceHanSerifTC-Heavy.otf', 24062096, 'a61f85925bcf3648461cec4f7ae9d8b01637e8fc'),
    ('SourceHanSerifTC-SemiBold.otf', 24699348, '17031895a448f9ccbeecd1d1091d78e0e60c5885'),
]

def digest_file(path):
    with path.open('rb') as file:
        return hashlib.file_digest(file, 'sha256').hexdigest()

def fetch(directory, relative, name, size, git_sha):
    target = directory / name
    url = f'https://raw.githubusercontent.com/adobe-fonts/source-han-serif/{COMMIT}/{relative}'
    if target.exists():
        data = target.read_bytes()
    else:
        with urllib.request.urlopen(url, timeout=45) as response:
            data = response.read(size + 1)
    actual = hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest()
    if len(data) != size or actual != git_sha:
        raise ValueError('官方字体长度或 Git blob 指纹不符：' + name)
    if not target.exists():
        target.write_bytes(data)
    return target, url

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--download-fonts', action='store_true')
    args = parser.parse_args()
    if args.source.stat().st_size > 512 * 1024**2:
        raise ValueError('私有样本超过 512 MiB 实验上限')
    out = ROOT / '.local/render-tab1'
    out.mkdir(exist_ok=True, parents=True)
    source_hash = digest_file(args.source)
    if args.download_fonts:
        from fontTools.ttLib import TTFont
        fonts = ROOT / '.local/render-fonts'
        fonts.mkdir(exist_ok=True, parents=True)
        manifest_file = fonts / 'manifest.json'
        manifest = json.loads(manifest_file.read_text(encoding='utf8')) if manifest_file.exists() else []
        fetch(fonts, 'LICENSE.txt', 'SourceHanSerif-LICENSE.txt', 4463, '07235a78b936958799812313565ba0d03e7cd7dd')
        for name, size, git_sha in FILES:
            file, url = fetch(fonts, 'OTF/TraditionalChinese/' + name, name, size, git_sha)
            with TTFont(file, recalcTimestamp=False) as font:
                postscript = font['name'].getDebugName(6)
                if postscript != file.stem:
                    raise ValueError('字体 PostScript 名称不匹配')
                entry = {'name': postscript, 'family': 'LayerLens Local ' + postscript,
                         'file': name, 'sha': digest_file(file), 'source': url,
                         'version': font['name'].getDebugName(5), 'codepoints': sorted(font.getBestCmap())}
            manifest = [item for item in manifest if item['name'] != postscript] + [entry]
        manifest_file.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + '\n', encoding='utf8')
        print(json.dumps({'fonts': [{k: f[k] for k in ['name', 'sha', 'version']} for f in manifest]}), flush=True)
    import psd_tools
    import PIL
    from psd_tools import PSDImage
    psd = PSDImage.open(args.source)
    reference = psd.topil(apply_icc=False)
    if reference is None:
        raise ValueError('PSD 没有保存时合成图')
    reference_file = out / 'saved-reference.png'
    reference.save(reference_file)
    inventory = json.loads((ROOT / '.local/render-tab1-inventory.json').read_text(encoding='utf-8-sig'))
    projection = next(item for item in inventory if item['profile'] == 'largeLayers')
    if projection['sha256'] != source_hash:
        raise ValueError('核心覆盖清单与源文件不符')
    visible = [item for item in projection['layers'] if item['visible'] and item['opacity'] > 0]
    drawn = [item for item in visible if item['content'] in ['bitmap', 'text', 'path']]
    unsupported = [item for item in visible if item['content'] not in ['bitmap', 'text', 'path', 'group']]
    texts = [item for item in drawn if item['content'] == 'text' and item['bounds']['width'] > 0]
    bitmaps = [item for item in drawn if item['content'] == 'bitmap' and item['bounds']['width'] > 60 and item['bounds']['height'] > 60]
    def in_canvas(item):
        b = item['bounds']
        return 0 <= b['x'] + b['width']/2 < psd.width and 0 <= b['y'] + b['height']/2 < psd.height
    texts = list(filter(in_canvas, texts))
    bitmaps = list(filter(in_canvas, bitmaps))
    chosen = [texts[0], texts[len(texts)//2], bitmaps[len(bitmaps)//2]]
    records = psd._record.layer_and_mask_information.layer_info.layer_records
    layer_by_record = {id(layer._record): layer for layer in psd.descendants()}
    hit_points = []
    for item in bitmaps:
        layer = layer_by_record.get(id(records[item['sourceRecordIndex']]))
        if layer is None:
            raise ValueError('独立图层记录无法对应核心引用')
        image = layer.topil(apply_icc=False)
        if image is None:
            continue
        rgba = image.convert('RGBA')
        b = item['bounds']
        if rgba.size != (b['width'], b['height']):
            raise ValueError('独立图层像素尺寸不符')
        # 选取内部连续不透明区域，降低 fit 缩放后的指针取整对命中的影响。
        found = None
        for y in range(12, rgba.height-12, 17):
            for x in range(12, rgba.width-12, 17):
                if not (20 < b['x']+x < psd.width-20 and 20 < b['y']+y < psd.height-20):
                    continue
                if all(rgba.getpixel((x+dx, y+dy))[3] >= 200 for dx,dy in [(0,0),(-8,0),(8,0),(0,-8),(0,8)]):
                    found = {'x': b['x']+x, 'y': b['y']+y, 'sourceLayerId': item['id']}
                    break
            if found:
                break
        if found and all((found['x']-p['x'])**2+(found['y']-p['y'])**2 > 400**2 for p in hit_points):
            hit_points.append(found)
        if len(hit_points) == 2:
            break
    if len(hit_points) != 2:
        raise ValueError('未找到两个独立可命中的区域')
    result = {'sourceSha256': source_hash, 'sourceBytes': args.source.stat().st_size,
              'width': psd.width, 'height': psd.height, 'psdTools': psd_tools.__version__,
              'pillow': PIL.__version__, 'referenceSha256': digest_file(reference_file),
              'reference': 'saved-reference.png',
              'layerCount': len(projection['layers']),
              'diagnosticDrawnLayerIds': [item['id'] for item in drawn],
              'diagnosticUnsupportedLayerIds': [item['id'] for item in unsupported],
              'details': [{'layerId': item['id'], 'bounds': item['bounds'], 'sourceKind': item['content']} for item in chosen],
              'hitPoints': hit_points,
              'limitations': ['独立解码保存时合成图，不重新合成图层；ICC 关闭。',
                              '所用字体版本可追溯，但源 PSD 没有制作时字体二进制，不能保证版本相同。']}
    if digest_file(args.source) != source_hash:
        raise ValueError('准备期间源 PSD 改变')
    (out / 'manifest.json').write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n', encoding='utf8')
    print(json.dumps(result, ensure_ascii=False), flush=True)

if __name__ == '__main__':
    main()
