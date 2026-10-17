"""无损拼接同一次 MCP 捕获；检查分块位置、尺寸、覆盖与完整 RGBA SHA-256。"""
import argparse
import hashlib
import json
import math
from pathlib import Path
from PIL import Image

def assemble(manifest_path, output):
    data = json.loads(manifest_path.read_text(encoding='utf8'))
    w, h, size = data['width'], data['height'], data['tileSize']
    if not (0 < w <= 8192 and 0 < h <= 8192 and w*h*4 <= 64*1024**2 and size == 256):
        raise ValueError('捕获尺寸超出额度')
    count = math.ceil(w/size)*math.ceil(h/size)
    if data['tileCount'] != count or len(data['tiles']) != count:
        raise ValueError('截图分块缺失')
    image = Image.new('RGBA', (w,h))
    for index, tile in enumerate(data['tiles']):
        x = index % math.ceil(w/size)*size; y = index // math.ceil(w/size)*size
        width, height = min(size,w-x), min(size,h-y)
        if tile['captureId'] != data['captureId'] or tile['index'] != index or tile['file'] != f'{index}.png' or (tile['x'],tile['y'],tile['width'],tile['height']) != (x,y,width,height):
            raise ValueError('分块身份、顺序或矩形不符')
        file = manifest_path.parent / tile['file']
        if file.stat().st_size > 512*1024:
            raise ValueError('分块超过传输额度')
        with Image.open(file) as part:
            if part.size != (width,height):
                raise ValueError('分块 PNG 尺寸不符')
            image.paste(part.convert('RGBA'),(x,y))
    if hashlib.sha256(image.tobytes()).hexdigest() != data['rgbaSha256']:
        raise ValueError('完整 RGBA 指纹不符，拒绝不完整截图')
    with output.open('xb') as file:
        image.save(file,format='PNG')

if __name__ == '__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('--manifest',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    assemble(args.manifest,args.output)
