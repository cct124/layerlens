"""绑定本机明确指定的 ArialMT；只复制到忽略目录，不分发商业字体。"""
import argparse
import hashlib
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / '.local/render-fonttools'))
from fontTools.ttLib import TTFont

parser = argparse.ArgumentParser()
parser.add_argument('--font', type=Path, required=True)
args = parser.parse_args()
data = args.font.read_bytes()
if len(data) > 8 * 1024 * 1024:
    raise ValueError('字体超过 8 MiB')
font = TTFont(args.font, recalcTimestamp=False)
name = font['name'].getDebugName(6)
if name != 'ArialMT':
    raise ValueError('源 PostScript 名称不是 ArialMT，不允许替换')
out = ROOT / '.local/render-fonts'
out.mkdir(exist_ok=True, parents=True)
(out / 'ArialMT.ttf').write_bytes(data)
entry = {'name': name, 'family': 'LayerLens Local Arial', 'file': 'ArialMT.ttf', 'sha': hashlib.sha256(data).hexdigest(), 'version': font['name'].getDebugName(5), 'codepoints': sorted(font.getBestCmap())}
(out / 'manifest.json').write_text(json.dumps([entry], ensure_ascii=False, indent=2) + '\n', encoding='utf8')
print(json.dumps({k:v for k,v in entry.items() if k != 'codepoints'}))
