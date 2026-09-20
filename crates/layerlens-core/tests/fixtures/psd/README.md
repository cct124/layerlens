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
| `text-engine-72.psd`             | 72 DPI、27 个文字层；独立 EngineData 编码，覆盖混合字符／段落样式、原文映射、属性来源及异常降级                                                 |
| `text-engine-300.psd`            | 300 DPI、单个混合样式层；原文、矩阵和引擎样式数值与 72 DPI 的对应层相同                                                                         |
| `text-engine-no-dpi.psd`         | 不含分辨率资源、单个混合样式层；不得填入默认 DPI 或换算文字度量                                                                                 |
| `viewer-scale.psd`               | RGB/8 位、256×128、72 DPI；512 个 8×8 位图层和独立色块合成图，用于默认预算下跨四页查询、深滚动及定位                                            |
| `viewer-text.psd`                | RGB/8 位、256×128、72 DPI；长原文、300 段样式及字节预算分页三类文字层，合成图为纯白而非文字渲染参考                                             |

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

### EngineData 分段样式

[文字样本模块](../../../../../scripts/psd-text-fixtures.mjs)使用 Node 内置 Buffer 独立编码 EngineData 字典、数组、数值和带 BOM 的 UTF-16BE 字符串；括号及反斜杠按字节转义，覆盖高字节转义。样式预期、来源路径与诊断单独声明，未使用候选解码／写入器计算预期。字体名称均为虚构测试值，不表示字体已安装或可按样本渲染。

混合样式原文、UTF-16 长度和矩阵与 `metadata-text.psd` 相同。EngineData 原文可与 TySh 精确相同，或只多一个尾随 CR；预期字符区间为 `[0,2)`、`[2,6)`、`[6,12)`，段落区间为 `[0,8)`、`[8,12)`。显式字号为 18、24、12；字体索引使用 1／2，第 0 项故意设为不同名称，以检出默认字体回退。预期同时核对局部属性及同类别 `DefaultRunData` 的来源，不接受从另一类别或未展开引用推测值。

27 个文字层覆盖精确映射、单 CR 映射、仅结束符的额外段、空文本、缺少 TySh 原文、空白与转义字符，以及长度不足／超量／负数／小数／溢出／零、数组数量不符、代理对拆分、无效段落范围、原文不一致、字体缺失／非法、局部样式损坏、未支持颜色或样式引用等情况。无效字符范围与段落范围分别降级；属性为 `null` 时保留对应原因，不以默认值掩盖错误。完整坏数字、EngineData／TySh 非法 UTF-16 输入由集成测试从 PSD 字节构造并断言解析失败；核心单元测试另覆盖 16384 段上限、组合字符边界与非有限度量。

72／300／缺失 DPI 三份样本的混合层文字结果完全相同，只证明当前保留引擎值、没有按 DPI 或矩阵重复缩放。所有度量仍标记 `unverifiedEngine`，RGB 未经过 ICC 转换，样式能力为 `partial`；这些人工样本不能确定物理单位、字体字重或 Photoshop 排版保真。样本合成像素不是文字渲染参考。

### 默认预算查看器规模

[查看器样本模块](../../../../../scripts/psd-viewer-fixtures.mjs)维护人工规模输入。`viewer-scale.psd` 的 PSD 记录为底到顶 `tile-000` 至 `tile-511`，查看器按相反顺序查询。每层边界为 `{x:列×8,y:行×8,width:8,height:8}`，列为编号除以 32 的余数，行为编号除以 32 向下取整；RGBA 为 `(32+列×7,48+行×11,160,255)`。保存时合成图独立按同一人工规则编码，不由候选解析器合成。

`viewer-text.psd` 包含三层，均保留矩阵 `[1.5,0.25,-0.5,2,-3,7]`、文档坐标边界 `{x:16,y:16,width:224,height:96}` 与原文绝对样式区间：

- `long-text`：4804 个 UTF-16 code unit，含 emoji、CR/LF 和尾部空格；默认 2048 单位边界落在代理对内部，因此首段结束于 2047。
- `many-style-runs`：1200 个 UTF-16 code unit，300 个四单位字符段，字号 18／19 交替；每页最多 128 段，首段结束于 512。
- `large-styles`：400 个 UTF-16 code unit，100 个四单位字符段；人工字体名包含多字节及 JSON 转义字符，单项低于 32 KiB，但整份详情高于 256 KiB，必须依完整序列化字节预算续读。

[公开服务回归](../../viewer_fixtures.rs)在默认配置下核对四页图层、全部网格像素、三层原文逐页重建、UTF-16／样式边界及关闭退出后的资源账本归零。所有文字单位仍为 `unverifiedEngine`，字体是虚构测试数据，纯白合成图不能证明文字排版。合成规模样本与原生桌面交互、真实稿视觉及性能验收分别记录，不互相替代。

## 格式依据

[Adobe Photoshop File Formats Specification](https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/) 的以下部分：

- File Header：`8BPS`、version 1、RGB color mode 3、3／4 个合成通道及大端整数。
- Image Resources / ResolutionInfo（1005）：16.16 fixed 分辨率、pixels/inch 单位。
- Layer and Mask Information：图层矩形、通道 ID 与长度、可见性 flags、四字节对齐 Pascal 名称；透明度通道 ID 为 `-1`。
- Channel Image Data / Image Data：RAW 或 PackBits RLE、PSD v1 的两字节行长表。图层通道分别包含 compression 和各自行长表，合成图则共用 compression，连续写全部通道的行长表。
- Additional Layer Information：`lsct` 组结构、`TySh` Type Tool Object Setting、`PlLd` Placed Layer、`brit` Brightness and Contrast、`GdFl` Gradient Fill，以及 Descriptor 的 UTF-16 文本、大端浮点矩阵和条目结构。

本轮样本按规范构造及人工值核对；真实设计稿和 Photoshop 视觉参考仍是后续验收项。
