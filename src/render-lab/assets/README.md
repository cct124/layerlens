# 实验字体

显式本机 ArialMT 绑定入口为 [prepare-render-local-font.py](../../../scripts/prepare-render-local-font.py)。校验 PostScript 名称后仅复制到忽略目录 `.local/render-fonts/`，Konva 验证构建校验长度、SHA-256 和字符覆盖。无清单时仍报告缺字体。实测 Arial Version 7.06，SHA-256 `b3658eadae55e682b5f69eb64c439c1ecc8f196c0bb8d4756d145d13bc86476a`；原 PSD 未提供制作时字体指纹，不能声称版本相同。

Arial 不纳入仓库或公开产物；含此绑定的实验构建只用于本机测试，不分发。普通构建没有引用字体模块，并由隔离检查排除字体。本入口不提供系统字体自动发现或静默替换。

`NotoSans.ttf` 来自 Google Fonts 的 `ofl/notosans/NotoSans[wdth,wght].ttf`，2026-10-15 获取，许可证见 `OFL.txt`。SHA-256：`bfb7bb691513f12e734dc346c03a03f784912432d7e3fa8e56efcf906fe86b3d`。

来源：<https://github.com/google/fonts/tree/main/ofl/notosans>。固定字体字节供浏览器 FontFace 与 Konva 使用；只包含在验证构建。英文样本使用 Regular 默认实例，不静默回退中文或其他未覆盖字形。该文件不是任意 PSD 字体的替代方案。

第二轮 `LayerLensLabCJK.otf` 是 Noto Sans CJK SC 2.004 的固定子集，已重命名字体家族为 `LayerLensLabCJK-Regular`，仅覆盖脚本声明的 ASCII 与实验中文字符。来源为 `notofonts/noto-cjk` 的 `Sans2.004` 标签下 `Sans/OTF/SimplifiedChinese/NotoSansCJKsc-Regular.otf`，源 SHA-256：`2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b`；许可证见 `OFL-CJK.txt`。

子集由 [prepare-render-font.py](../../../scripts/prepare-render-font.py) 和固定 FontTools 4.60.1 生成，不是运行时依赖。源放入 `.local/NotoSansCJKsc-Regular.otf`、FontTools 放入 `.local/render-fonttools` 后执行该脚本；结果 36,676 字节，SHA-256 `b7113b5364f678f38d5abcea8d7a8fe315b039f3e7a6a6d85b534632e83afdfd`。字体构建保留源时间戳和固定字节。真实 PSD 使用其他字体时明确标记缺失，不用此子集替换。
