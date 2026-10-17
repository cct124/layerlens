"""固定 Noto CJK 2.004 的实验子集。仅用于离线测试；依赖 FontTools 4.60.1。"""
import hashlib
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / ".local/render-fonttools"))
from fontTools import subset  # noqa: E402
from fontTools.ttLib import TTFont  # noqa: E402

SOURCE = ROOT / ".local/NotoSansCJKsc-Regular.otf"
EXPECTED = "2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b"
if hashlib.sha256(SOURCE.read_bytes()).hexdigest() != EXPECTED:
    raise ValueError("Noto CJK 源字体指纹不符")
font = TTFont(SOURCE, recalcTimestamp=False)
options = subset.Options()
options.name_IDs = [0, 1, 2, 3, 4, 5, 6, 13, 14, 16, 17]
options.name_legacy = True
options.name_languages = [0x409]
worker = subset.Subsetter(options=options)
characters = set(range(32, 127)) | {ord(c) for c in "图层透镜中文排版测试自动换行旋转文字分段颜色字体混合保持清晰只读原稿性能比较路径像素，。"}
worker.populate(unicodes=characters)
worker.subset(font)
for record in font["name"].names:
    if record.nameID in [1, 3, 4, 6, 16]:
        record.string = "LayerLensLabCJK-Regular".encode(record.getEncoding())
font.recalcTimestamp = False
output = ROOT / "src/render-lab/assets/LayerLensLabCJK.otf"
font.save(output)
print(f"{output.name}: {output.stat().st_size} bytes, SHA256 {hashlib.sha256(output.read_bytes()).hexdigest()}")
