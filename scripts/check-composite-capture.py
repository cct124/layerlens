"""核对 render-simple 的独立合成色块；输入仅来自 MCP 截图及观测，不调用渲染器。"""
import json
import sys
from PIL import Image

view = json.load(sys.stdin)
canvas = view["canvas"]
image_rect = canvas["image"]
viewport = canvas["viewport"]
probes = []
with Image.open(sys.argv[1]) as captured:
    image = captured.convert("RGB")
    # 远离边缘和文本区域，不把浏览器抗锯齿差别误判为源图失真。
    for x, y, expected in [
        (220, 180, [248, 249, 252]), (260, 200, [248, 249, 252]),
        (40, 180, [40, 115, 210]), (60, 200, [240, 105, 50]),
        (85, 220, [40, 115, 210]), (150, 230, [32, 175, 135]),
        (230, 260, [32, 175, 135]),
    ]:
        css_x = image_rect["x"] + x * canvas["zoom"]
        css_y = image_rect["y"] + y * canvas["zoom"]
        if not (viewport["x"] + 3 < css_x < viewport["x"] + viewport["width"] - 3
                and viewport["y"] + 3 < css_y < viewport["y"] + viewport["height"] - 3):
            continue
        pixel = (round(css_x * view["dpr"]), round(css_y * view["dpr"]))
        actual = list(image.getpixel(pixel))
        valid = max(abs(a - b) for a, b in zip(actual, expected)) <= 2
        probes.append(dict(document=[x, y], pixel=pixel, actual=actual, expected=expected, passed=valid))
assert len(probes) >= 4, "可见色块采样不足"
result = dict(passed=all(p["passed"] for p in probes), probes=probes, limits="仅工程合成图色块；不代表字体重绘、ICC 或完整 PSD 保真")
print(json.dumps(result, ensure_ascii=True))
if not result["passed"]:
    sys.exit(1)
