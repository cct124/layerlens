# 最小 PSD 样本

这些文件由 LayerLens 项目人工定义，并按照 Adobe PSD v1 二进制规范编码，采用仓库 [MIT 许可证](../../../../../LICENSE)。不含第三方设计稿、个人信息或待测解析器生成的内容。

生成器仅使用 Node 内置模块。预期属性和像素直接写在生成器中，`manifest.json` 同时记录文件大小和 SHA-256；预期值不来自候选解析器的读取或写入结果。当前没有 Photoshop 打开或视觉对照证据，不能据此声明真实 PSD 兼容性。

## 生成与校验

在仓库根目录运行：

```sh
node scripts/generate-psd-fixtures.mjs
node scripts/generate-psd-fixtures.mjs --check
```

`--check` 在内存中重新构造文件并逐字节核对已提交样本和清单，不写入文件。调整预期需修改[生成器](../../../../../scripts/generate-psd-fixtures.mjs)，重新生成后检查差异；不要手改二进制或清单。

`manifest.json` 使用生成器的确定性 JSON 格式，已从 Prettier 排除；统一检查仍通过 `fixtures:check` 核验全部生成结果。

## 覆盖范围

| 文件                             | 输入与预期                                                                                                                                      |
| -------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `bitmap-raw.psd`                 | RGB/8 位、4×3、72 DPI；RAW 图层和合成图；负坐标、透明和半透明像素、隐藏图层                                                                     |
| `bitmap-rle.psd`                 | 与 RAW 完全相同的人工预期，像素通道改用逐行 PackBits literal run；用于核对图层与合成图不同的 RLE 长度表布局                                     |
| `bitmap-no-composite.psd`        | 与 RAW 相同的完整元数据，故意在 Image Data 节起点结束；这是异常输入，用于核对缺失预览，不视为完整的规范 PSD 文件                                |
| `rgb16.psd`                      | 合法 RGB/16 位、1×1、72 DPI、无图层；RAW 合成图为 `R=0x1234, G=0xabcd, B=0x00ff`，用于首轮不支持位深的明确失败路径                              |
| `resources-groups-alpha-raw.psd` | RGB/8 位、4×1；非空 Resource 1028、奇数长度 opaque ICC Resource 1039、后续 DPI 资源、隐藏父组、透明与半透明合成图                               |
| `resources-groups-alpha-rle.psd` | 上一文件的 RLE 版本；相同独立元数据及像素预期，用于核对懒解码是否保留 global alpha 状态                                                         |
| `metadata-text.psd`              | 合法 TySh 原文／变换、结构有效但不支持的 GdFl 渐变 descriptor、PlLd 智能对象、brit 调整层、未知 `zzzz` 附加块；类型与能力诊断不能退化为普通位图 |

基础图的图层记录按底到顶排列：

1. `negative-origin`：可见，文档边界 `{x:-1,y:1,width:3,height:2}`。第一行是红色不透明、绿色半透明、蓝色全透明；第二行是黄色不透明、青色不透明、品红半透明。RGBA8 alpha 依次为 `255,128,0,255,255,128`。
2. `hidden-overlay`：隐藏，边界 `{x:1,y:0,width:1,height:1}`，像素为不透明橙色 `ff8000ff`。

保存时合成图以白色为底，只显示画布内的可见图层内容，合成后的绿色为 `7fff7f`、青色为 `00ffff`、品红为 `ff7fff`，其余像素为白色，输出预期 alpha 均为 255。合成图在 PSD 中只存 RGB 三个通道；单图层独立素材必须保留全部 3×2 像素，不能裁去左侧画布外内容或透明像素中的原始 RGB。

基础 `bitmap-*` 文件没有 ICC 配置，72 DPI 来自显式 Image Resource 1005，不能把不存在的色彩配置报告为 sRGB。基础图层使用 `normal` 混合、不透明度 255、无蒙版和效果，不覆盖复杂视觉依赖或性能规模。截断和损坏场景可以从文件在测试中构造，避免重复保存大量相似二进制。

### 资源、组与合成透明度

`resources-groups-alpha-*` 的 Resource 1028 有非空 IPTC 形式载荷，Resource 1039 刻意使用奇数长度 `ICC-unvalidated` opaque 字节；它不是有效 ICC 配置，不能据此宣称色彩管理支持。后续 Resource 1005 的 72 DPI 用于检出资源跳读长度／偶数填充错误。

文件有 4 条图层记录：`alpha-art`、组边界、`hidden-child`、`hidden-folder`；预期规范化为 3 个节点。子层自身可见，但其父组隐藏，因此有效可见性为 false，不能导出成可见素材。结构记录不应泄漏为普通图层。

负图层记录数声明 global alpha，合成图有 RGB 与透明度 4 个通道。四个像素的独立 RGBA 预期为 `204060ff 1e3c5a55 183654aa ffffff00`；PSD 实存白底 matte RGB，alpha=85 的通道如 `(180-255×(1-85/255))/(85/255)=30`，alpha=170 同理。透明像素保留其已保存 RGB。选取这些值避免不可逆量化和舍入争议；这只验证指定规则，不代替真实稿半透明边缘视觉验收。

### 文字与类型诊断

`metadata-text.psd` 的 TySh 原文是 ` 中😀é\r\n尾  \r`（转义形式），UTF-16 长度 12。它包含首部空格、中文、代理对 emoji、组合字符、CR/LF、尾部空格及尾随 CR；仿射矩阵为 `[1.5,0.25,-0.5,2,-3,7]`。原文不得被候选的换行归一化覆盖；候选差异应明确报告。此样本没有 EngineData、字体、字号或样式区间，因此这些属性必须保持未支持，不能用默认值补齐。

GdFl 使用 `Grad` 键与空 `Grdn` descriptor，descriptor 结构完整但不具备可渲染的完整渐变语义，仅验证已知不支持特性的分项降级。其余节点缓存了白色像素，缓存不代表该节点可作为独立位图素材。合成预览是明确保存的白色参考，不用它证明这些复杂节点已被重新渲染。测试另构造非法 TySh 版本、descriptor 版本、无界条目数量和资源长度，验证损坏元数据仍为致命错误。

公开集成测试还在基础 RAW 样本内构造文档级连续附加块：覆盖 `8BIM`／`8B64`、1–4 字节载荷、四字节补齐及非法填充，核对后续块偏移和合成像素，避免真实稿兼容修复只依赖私有文件。全局蒙版测试核对诊断的原始字节偏移及独立素材限制。

## 格式依据

[Adobe Photoshop File Formats Specification](https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/) 的以下部分：

- File Header：`8BPS`、version 1、RGB color mode 3、3／4 个合成通道及大端整数。
- Image Resources / ResolutionInfo（1005）：16.16 fixed 分辨率、pixels/inch 单位。
- Layer and Mask Information：图层矩形、通道 ID 与长度、可见性 flags、四字节对齐 Pascal 名称；透明度通道 ID 为 `-1`。
- Channel Image Data / Image Data：RAW 或 PackBits RLE、PSD v1 的两字节行长表。图层通道分别包含 compression 和各自行长表，合成图则共用 compression，连续写全部通道的行长表。
- Additional Layer Information：`lsct` 组结构、`TySh` Type Tool Object Setting、`PlLd` Placed Layer、`brit` Brightness and Contrast、`GdFl` Gradient Fill，以及 Descriptor 的 UTF-16 文本、大端浮点矩阵和条目结构。

本轮样本按规范构造及人工值核对；真实设计稿和 Photoshop 视觉参考仍是后续验收项。
