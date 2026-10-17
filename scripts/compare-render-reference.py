"""比较测试 MCP 的 100% 截图与独立 PNG；只输出诊断，不用单一像素分数宣称保真。"""
import argparse
import hashlib
import json
import math
from pathlib import Path
from PIL import Image

parser = argparse.ArgumentParser()
parser.add_argument("--report", type=Path, required=True)
parser.add_argument("--reference", type=Path, required=True)
parser.add_argument("--sha256", required=True)
parser.add_argument("--acceptance", choices=['raster', 'text'])
args = parser.parse_args()
reference_bytes = args.reference.read_bytes()
if hashlib.sha256(reference_bytes).hexdigest() != args.sha256:
    raise ValueError("独立参考指纹不符")
report = json.loads(args.report.read_text(encoding="utf-8"))
if not report["passed"]:
    raise ValueError("功能场景未通过；请先检查原失败报告")
reference = Image.open(args.reference).convert("RGBA")
results = []
for capture in report["images"]:
    if not capture["name"].endswith("-100.png"):
        continue
    if not capture["unchanged"] or capture["sourceSha256"] != report["sourceHashes"][0]:
        raise ValueError("截图身份或视图版本不一致")
    canvas = capture["canvas"]
    if reference.size != (canvas["documentWidth"], canvas["documentHeight"]):
        raise ValueError("独立参考尺寸与 PSD 不一致，不自动缩放参考")
    screenshot = Image.open(args.report.parent / capture["name"]).convert("RGB")
    viewport, image = canvas["viewport"], canvas["image"]
    dpr, zoom = capture["dpr"], canvas["zoom"]
    if zoom != 1:
        raise ValueError("只比较原尺寸截图")
    count = differing = total = peak = 0
    expected_ink, actual_ink = set(), set()
    # 排除两像素边缘的选择边框；像素中心采样仍受 DPR 重采样与颜色管理影响。
    for y in range(2, reference.height - 2):
        for x in range(2, reference.width - 2):
            sx, sy = image["x"] + x + 0.5, image["y"] + y + 0.5
            if not (viewport["x"] <= sx < viewport["x"] + viewport["width"] and viewport["y"] <= sy < viewport["y"] + viewport["height"]):
                continue
            px, py = math.floor(sx * dpr), math.floor(sy * dpr)
            if not (0 <= px < screenshot.width and 0 <= py < screenshot.height):
                continue
            rgba = reference.getpixel((x, y))
            # 对应实验 viewport 的固定背景；参考与输出均比较屏幕合成后的 RGB。
            expected = [round(rgba[i] * rgba[3] / 255 + (50, 59, 73)[i] * (1 - rgba[3] / 255)) for i in range(3)]
            actual = screenshot.getpixel((px, py))
            differences = [abs(actual[i] - expected[i]) for i in range(3)]
            maximum = max(differences)
            count += 1
            total += sum(differences)
            peak = max(peak, maximum)
            differing += maximum > 8
            if args.acceptance == 'text':
                if max(expected) < 180:
                    expected_ink.add((x,y))
                if max(actual) < 180:
                    actual_ink.add((x,y))
    if count == 0:
        raise ValueError("截图没有可比较的原稿像素")
    item = {"engine": capture["engine"], "pixels": count, "meanAbsoluteRgbError": total / (count * 3), "maxChannelError": peak, "fractionOver8": differing / count, "coverage": count / ((reference.width-4)*(reference.height-4))}
    if args.acceptance == 'raster':
        item['passed'] = item['coverage'] >= .99 and item['meanAbsoluteRgbError'] <= 2 and item['fractionOver8'] <= .02
    if args.acceptance == 'text':
        def expanded(points):
            return {(x+dx,y+dy) for x,y in points for dx in [-1,0,1] for dy in [-1,0,1]}
        def lines(points):
            groups = []
            for y in sorted({p[1] for p in points}):
                if not groups or y > groups[-1][-1]+1:
                    groups.append([y])
                else:
                    groups[-1].append(y)
            return [[min(x for x,y in points if y in rows),rows[0],max(x for x,y in points if y in rows),rows[-1]] for rows in groups]
        recall = len(expected_ink & expanded(actual_ink)) / max(1,len(expected_ink))
        precision = len(actual_ink & expanded(expected_ink)) / max(1,len(actual_ink))
        expected_lines, actual_lines = lines(expected_ink), lines(actual_ink)
        aligned = len(expected_lines) == len(actual_lines) and len(expected_lines) > 0 and all(abs(a-b)<=2 for aa,bb in zip(expected_lines,actual_lines) for a,b in zip(aa,bb))
        item.update({'foregroundRecallWithin1px':recall,'foregroundPrecisionWithin1px':precision,'expectedLineBoxes':expected_lines,'actualLineBoxes':actual_lines,'passed':item['coverage']>=.99 and aligned and recall>=.95 and precision>=.95})
    results.append(item)
result = {"referenceSha256": args.sha256, "sourceSha256": report["sourceHashes"][0], "diagnosticOnly": True, "note": "MCP 截图像素中心与独立 PNG 的可见区域比较；排除边缘，未校正浏览器重采样或 ICC，不能独立作为保真通过标准。", "results": results}
output = args.report.parent / "visual-reference.json"
if args.acceptance:
    result['acceptance'] = {'kind':args.acceptance,'passed':bool(results) and all(r['passed'] for r in results),'scope':'预先固定的最小样本像素／文字布局门槛，仍需目视；不代表完整 Photoshop 保真'}
output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
print(json.dumps({"output": str(output), "results": results}, ensure_ascii=False))
if args.acceptance and not result['acceptance']['passed']:
    raise SystemExit(1)
