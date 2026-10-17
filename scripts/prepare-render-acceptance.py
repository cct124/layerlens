"""用独立 psd-tools 解码原稿保存时合成图；不调用被测渲染器，不重新合成图层。"""
import hashlib
import json
from pathlib import Path
import sys
ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / '.local/render-reference-tools'))
from psd_tools import PSDImage
import psd_tools
import PIL

corpus = ROOT / '.local/render-corpus'
original = json.loads((corpus / 'public-manifest.json').read_text(encoding='utf8'))
cases = []
for name in ['2layers', 'semi-transparent-layers', 'text', 'mask']:
    file = 'public-' + name + '.psd'
    data = (corpus / file).read_bytes()
    source = next((c for c in original['cases'] if c['file'] == file), None)
    if source is None:
        source = json.loads((corpus / (file + '.source.json')).read_text(encoding='utf8'))
    digest = hashlib.sha256(data).hexdigest()
    if digest != source['sha256']:
        raise ValueError('公开源指纹不符：' + file)
    psd = PSDImage.open(corpus / file)
    reference = psd.topil(apply_icc=False)
    if reference is None:
        raise ValueError('没有保存时合成图：' + file)
    ref_name = file.replace('.psd', '-saved-reference.png')
    reference.save(corpus / ref_name)
    cases.append({'file': file, 'sha256': digest, 'sourceUrl': source['sourceUrl'], 'width': psd.width, 'height': psd.height, 'referenceFile': ref_name, 'referenceSha256': hashlib.sha256((corpus / ref_name).read_bytes()).hexdigest(), 'referenceKind': 'independently decoded PSD saved composite; no recomposition, ICC disabled'})
(corpus / 'acceptance-manifest.json').write_text(json.dumps({'psdTools': psd_tools.__version__, 'pillow': PIL.__version__, 'cases': cases}, ensure_ascii=False, indent=2) + '\n', encoding='utf8')
print(json.dumps({'cases':len(cases), 'psdTools':psd_tools.__version__, 'pillow':PIL.__version__}))
