# RENDER-01 渲染器对比实验

## 目标与验收

按[实验设计](../../../docs/09-渲染器对比实验.md)，在独立开发实例中以同一 Rust 解析结果比较 LeaferJS、Konva、CanvasKit。交付实验窗口、样本和能力矩阵、三个适配器、真实 MCP 回归、报告和实现比较。原稿只读，正式窗口及构建不引入实验资源；不以生成样本替代 Photoshop 保真证据。

## 状态与代码基准

- 创建：2026-10-15，Asia/Shanghai；完成：2026-10-16。状态：三轮对比完成，LeaferJS 为首版主候选，资源波动与支持边界保留；正式迁移另行规划。
- 分支 `dev`，本地 HEAD／本地远端跟踪引用 `e30ce2e`；接手工作区干净。未重新查询远端 CI，不将跟踪引用一致视为 CI 成功。
- 用户已授权先落设计和计划，再实施本实验；未授权提交或推送。
- M2-02／M2-03 的系统专项与 CI 核验、M3-01 的参考图任务继续保留，本实验按用户指定优先实施，不代替阶段出口。

## 已完成

- 第三轮完成场景像素准入、简单蒙版、真实字体绑定／自动行距／基线、统一位图采样、960×600 视口及 MCP 资源／交互验收。四个最小真实样本三轮视觉通过，最终程序另 4/4；大视口两引擎各 3/3；固定资源矩阵 Leafer 3/3、Konva 0/3，完整保留此前各一次资源失败，未声明长期无泄漏。
- 对比阶段收敛为 Leafer 主候选，Konva 对照、CanvasKit 专项保留；依据和限制见[第三轮结果](../../../docs/09-渲染器对比实验.md#第三轮收敛结果与首版决策2026-10-16)。正式工作区未迁移，文件只在本机，未提交推送。

- 检查当前实现、依赖、MCP 隔离实例与窗口／观测边界；建立独立设计文档和本连续任务记录。
- 实现独立 `render-lab` feature、构建配置及实验页面；普通构建输出检查排除实验入口、引擎、字体和 WASM。显式实验实例复用受限 `main` 和固定修订读取，正常实例没有场景权限。
- 核心提供有界场景 DTO、按需位图和参考图、真实文字／路径属性与隐藏祖先；生成 480×320、11 层 `render-simple.psd`，三种适配器均真实绘制。固定 Noto Sans 字节与 SHA-256，普通构建不包含字体。
- 接入自动／Ctrl 临时选择、悬停、alpha／路径／边界候选、中键平移、100%／200%／400% 缩放和核心检查选择；保留三方案最近画面。显式释放、旧场景代次、正常关闭与重开复用核心生命周期。
- 补齐 MCP 语义目标、实验观测、方案／代次及选择等待、截图白名单和三轮串行稳定性入口。最终三轮验收 7/7（普通发行关闭 1/1、业务 3/3、真实渲染 WebView 3/3），60 个产物副本 SHA-256 全部匹配。
- 第二轮使用安全 `sysinfo` 封装、受限 comparison 档位、中文子集和 CanvasKit ParagraphBuilder；补齐 16 ms 定时手势、公共读回屏障、独立截图动作、OS 内存及分卷报告收集。
- 第二轮 release 的 60 个对比实例、3 次原交互回归及普通发行关闭检查通过；六份真实 PSD 中五份有未绘制层，未将流程通过记为保真通过。性能及条件性结论见[设计结果表](../../../docs/09-渲染器对比实验.md#第二轮实测结果2026-10-16)。

## 实施计划

1. **场景与隔离**：检索 Rust 解析及文档租约、文字模型、样本生成器；提供有上限的实验场景及能力说明，复用源修订与既有选择服务。增加显式开发实验配置与独立页面，普通构建排除入口与资源。
2. **三个适配器**：精确锁定依赖；相同场景、字体、DPR／缩放／平移；提供一致的挂载、重绘、释放及错误观测。缺项不得硬编码或伪装为原生支持。
3. **MCP 与实验报告**：先扩展语义输入、观测／等待；用真实 WebView 完成各方案、100%／200%／400%、选择、切换／关闭等回归，核验源文件哈希并保留截图及耗时。性能逐引擎运行。
4. **独立视觉结论**：用来源与字体可核验的独立 PSD／参考补保真、真实 DPI 和薄弱点；对比实现成本后给条件性选型，不直接迁移主工作区。

## 验证

### 第三轮实测证据（2026-10-16）

- 归档收尾：完整 `format:check`、`git diff --check` 与 11 份相关文档的 117 个相对文件链接通过；归档引用无旧路径。当前普通二进制哈希与最终关闭测试报告一致。日志 `.local/render-third-format-check.log`／`render-third-doc-links.log`。全部改动保留本机，未暂存、提交或推送，远端 CI 未核验。

- Windows x64、release、WebView2／EdgeDriver 155.0.4283.45、DPR 1.5；实验视口 960×600、原生窗口 1280×900。全部功能输入、等待、采样、截图通过开发测试 MCP；按用户确认的空闲时段执行，最后已通知恢复电脑操作。
- 四份最小真实样本：`public-2layers`、`public-semi-transparent-layers`、`public-text`、`public-mask`。公开源固定 psd-tools 提交，新增 mask 的 Git blob SHA-1、长度和 SHA-256 均核验；`acceptance-manifest.json` 保存独立提取参考指纹。使用 psd-tools 1.10.9、attrs 25.3.0 与 Pillow 11.3.0，提取保存时合成，关闭 ICC、不重新合成。ArialMT Version 7.06 指纹见字体说明，仅存本机忽略目录，原 PSD 未提供制作时字体版本。
- 首轮修正后 4/4：`.local/dev-mcp/stability-hQZmVB/summary.json`；三轮视觉 12/12：[summary.json](../../../.local/dev-mcp/stability-4UhzUU/summary.json)，每实例三引擎、三倍率。最终代码确认 4/4：[summary.json](../../../.local/dev-mcp/stability-iOzU6c/summary.json)。原尺寸共 36＋12 个引擎对照，全部无未绘制层并通过固定数值门槛；已查看真实文字与蒙版截图，400% 文字实际原生重绘。不能将本次有限样本结论推广至第二轮所有未支持样本。
- 固定资源矩阵：[summary.json](../../../.local/dev-mcp/stability-K658DA/summary.json)，六个独立实例、各 20 轮，共 120 轮。Leafer 三实例通过；Konva 三实例均因释放态斜率 >2 MiB／轮失败，整个批次明确 failed。Leafer 峰值 814.5／874.3／893.1 MiB，斜率 1.29／−7.38／1.21；Konva 峰值 566.6／585.5／602.2 MiB，斜率 2.50／5.92／5.56。早期有效诊断 `stability-7Xppee`（Leafer）和 `stability-ec0bjF`（Konva）也失败，合看分别 3/4、0/4；没有修改门槛或强制 GC，无法证明长期无泄漏。
- 大视口混合长页：[summary.json](../../../.local/dev-mcp/stability-6EqJ9n/summary.json)，6/6，通过真实平移、倍率、自动选择／关闭自动选择、Ctrl 按下及松开、核心同步、释放／关闭重开。Leafer 三实例同步绘制 P95 3.8–4.1 ms、平移输入至应用就绪 P95 5.5–6.2 ms；Konva 为 4.2–4.7／1.8–2.0 ms，均低于本机 50 ms 门槛，不代表物理呈现或硬件 FPS。
- 原渲染／业务隔离回归：[summary.json](../../../.local/dev-mcp/stability-GhiWFc/summary.json)，5/5（渲染 2、业务 2、普通发行拒绝开发入口 1）。所有矩阵的外部进程检查均无残留；源文件哈希未改变。
- 实测最终实验程序另存 `.local/dev-mcp/render-release-third-261016/layerlens.exe`，SHA-256 `2937005a3b849be8cc109b3656b8f9928b51cc91ebc3ad09b6fcd24745a80d56`。含本机字体的实验程序不公开分发。随后重建普通发行程序，确保 `target/release/layerlens.exe` 恢复普通功能。
- 白名单证据 `.local/dev-mcp/render-evidence-third-261016-*` 共 12 包、477 文件、44,712,113 字节，逐文件核对长度和 SHA-256 全部匹配；包括真实截图、独立参考 PNG、循环检查点、失败诊断、最终普通发行关闭检查与汇总，不包含字体、可执行文件、凭据或 profile。清单见 `.local/render-third-evidence-summary.json`。仍遵守单包 64 MiB／768 文件／单文件 8 MiB。
- 最终 `check:frontend` 通过：22 文件、193 测试，类型／lint／普通 dist 三文件隔离通过；日志 `.local/render-third-frontend-final.log`。实验 feature 全工作区 Rust：核心 107、桌面 58、实验场景集成 4 及其他集成／doctest 通过，日志 `.local/render-third-rust-final.log`。all-targets Clippy、15 项开发脚本、3 项独立视觉门槛反例、两套 DTO 一致性检查通过；日志分别为 `render-third-clippy-confirmed.log`、`render-third-scripts-final.log`、`render-third-reference-tests.log`、`render-third-bindings-dev.log`／`render-third-bindings-render.log`，均位于 `.local/`。
- 最终普通 release 重建后拒绝开发参数 1/1，报告 `.local/dev-mcp/render-third-production-final/desktop-G7lTUy/report.json`，未创建监听或实例。此前普通基准检查不代替该新程序的结果。沙箱的临时目录／源文件替换／回环 socket 拒绝均按相同入口提升执行后通过；没有跳过用例。Vite 仅提示未来 native config loader 的扩展名兼容警告，当前锁定版本构建通过。

### 第三轮范围与停止条件（2026-10-16，运行前固定）

- 数值视觉门槛在运行前固定：原尺寸可见采样覆盖率 ≥99%；透明位图／单蒙版 RGB MAE ≤2 且最大通道差 >8 的像素比例 ≤2%。黑色文字样本另要求前景在 1 px 容差内的召回／精确率均 ≥95%，行数一致、各行边界差 ≤2 px；同时检查三引擎截图。参考为 psd-tools 1.10.9 独立解码的保存时合成图（关闭 ICC），不进行图层重合成。复杂色彩管理不在本次门槛内。

- 用户授权继续深入测试，并要求在需要停止电脑操作时提前告知。当前进行代码／样本准备；任何会取得真实焦点的 MCP 批次，先说明预计时长并等待用户确认。
- 先检查共同 Rust 场景：将独立素材导出准入与场景像素读取分开，保留正式导出限制；仅扩展可验证的普通透明度、简单位图蒙版和确有来源的字体绑定，未知视觉语义继续明确拒绝。
- 视觉样本限定真实透明位图、真实文字、单一蒙版，另保留 `2layers` 控制样本。每层必须有真实输出或明确能力失败；以原稿保存的合成／独立 PNG 为参考，原尺寸查布局、透明度、顺序，200%／400% 查文字／路径重新绘制。不能把生成样本或整图回退作为原生文字通过。字体版本、ICC／重采样差异单独记录；没有可信真值的项不算通过。
- 内存以 Leafer／Konva 为主，每引擎三个独立进程，每次 20 轮加载／关闭固定 2000 小位图样本；预热前五轮与后五轮分组，记录工作集、私有提交量及显式资源。预先采用本机实验门槛：采样私有提交量最大值不超过 1280 MiB；后五轮释放态中位数较第 6–10 轮增加不超过 64 MiB，后 15 轮释放态线性趋势不超过 2 MiB／轮；资源计数为零且退出无残留。此门槛用于首版候选筛选，不是长期无泄漏证明，不强制 GC 或重试挑选读数。
- 代表性混合长页使用 960×600 视口，经 MCP 完成平移、缩放、自动／Ctrl 临时选择、关闭重开；先补语义窗口尺寸／视口操作和资源观测所需契约。受测交互必须正确；同步绘制 P95 ≤50 ms、连续平移输入到应用就绪 P95 ≤50 ms 为本机实验响应门槛，明确不代表硬件 FPS。
- 三引擎跑最小视觉对照；Leafer／Konva 跑资源与实际视口主矩阵，CanvasKit 只补会改变选择的文字／路径证据。满足基础视觉、交互、资源门槛即可确定首版候选并结束对比；若失败属于共同解析／场景缺口则明确记录阻塞与范围，不通过扩展跑分替代。保留全部失败及原始报告，不修改普通工作区或自动提交推送。

### 第二轮（2026-10-16）

- 环境：Windows x64、WebView2／EdgeDriver 155.0.4283.45、DPR 1.5，正式对比均为 `buildProfile=release`。所有功能操作通过开发测试 MCP；连续手势使用真实 WebDriver 输入，benchmark 同时启用原生窗口 MCP，共 19 工具，核对焦点与失焦计数。
- 共享布局三轮 36/36，12 样本各三轮，报告 [summary.json](../../../.local/dev-mcp/stability-DWnJP7/summary.json)；原生排版三轮 6/6，`typography`／`text-2000`，报告 [summary.json](../../../.local/dev-mcp/stability-3SPe81/summary.json)；公开真实 PSD 三轮 18/18 **工程流程**，报告 [summary.json](../../../.local/dev-mcp/stability-JgpuoH/summary.json)。自动重试均为 0，实例正常退出且外部进程检查无残留。
- 原有交互回归 3/3，报告位于 `.local/dev-mcp/render-second-baseline/` 的 `webview-sDpbSA`／`webview-pUCSCg`／`webview-c6O8Wl`；另用进程检查器核对 `remaining=[]`、`terminated=[]`。普通 release 拒绝开发参数 1/1，未创建监听或实例，报告 [report.json](../../../.local/dev-mcp/render-second-production/desktop-1vUirB/report.json)。最终成功批次共 64 项实例／流程检查，不含此前失败诊断。
- 六份真实 PSD 中，`public-2layers` 有 2 个绘制节点、0 未绘制层；其余五份为 0 绘制节点、2–3 个未绘制层，缺口分别涉及独立像素读取／图层类型、ArialMT、蒙版或未解释属性。流程成功不代表这些样本完成视觉重绘。公开源来自 psd-tools 固定提交 `51082d37f041425a9721e8a4252214a12b35cfa9`，来源及哈希见 `.local/render-corpus/public-manifest.json`。
- 独立 `public-masks-2-reference.png` 的 SHA-256 为 `f70305b07baaf62ec0e8e5ce57c16760582511be380fa268204e15ce321d3acf`；三轮 `webview-KpmyA6`／`webview-zZ4zgb`／`webview-6tLZSX` 的 `visual-reference.json` 均保留。每引擎每轮 3600 个可见像素采样，RGB MAE 约 37.93、最大通道差 103、72.89% 像素的最大通道差超过 8；未校正 ICC／浏览器重采样，仅用于定位共同场景缺失。已查看原生中文样本三引擎 100% 截图，中文、换行和旋转实际绘制，但没有对应 Photoshop 文字真值。
- `npm run check:frontend` 通过：21 文件／191 项测试、类型、lint 和普通 dist 的 3 文件隔离检查，日志 `.local/render-second-frontend-confirmed.log`。`cargo test --locked --workspace --features layerlens-desktop/render-lab` 通过：核心单元 103、相关集成（包括实验场景 3 项）、桌面 57 及 doctest；随后新增的定时手势边界用例单独 1/1，通过日志分别为 `.local/render-second-rust-tests-confirmed.log` 与 `.local/render-second-timed-gesture.log`。
- 实验 feature 的 all-targets Clippy、最终 lint、13 项脚本回归、13 份 fixtures 可复现检查通过；日志 `.local/render-second-final-clippy.log`、`.local/render-second-script-tests-final.log`。沙箱内曾出现 Vitest 临时文件 ENOENT、Rust 源文件 PermissionDenied 和目录联接 EPERM；按原入口获得提升执行后通过，没有跳过测试或放松检查。
- 收尾的 `bindings:dev:check`／`bindings:render:check`、完整 `format:check` 与 `git diff --check` 通过；11 份相关文档的 108 个相对文件链接均存在。日志 `.local/render-second-bindings-dev.log`、`.local/render-second-bindings-render.log`、`.local/render-second-format-final.log`、`.local/render-second-doc-links-confirmed.log`。
- 实验与普通 release 均构建成功；当前 `target/release/layerlens.exe` 已恢复为普通发行程序。实测实验程序另存 `.local/dev-mcp/render-release-261016/layerlens.exe`，SHA-256 为 `ca88eebe16cc6bad41f3e613268b12fe0f3240bc04761b3431d8502758b325b4`。`target/debug/layerlens.exe` 为早期诊断构建，不能代替本轮成功 release。
- 白名单产物位于 `.local/dev-mcp/render-evidence-261016-*`：共享布局 `shared-0`／`shared-1`／`shared-2` 三卷、`native`、`public`、`baseline`、`production`，以及 `failure-paragraph-style`／`failure-paragraph-defaults`／`failure-focus`。10 包共 909 文件、140,029,680 字节，全部逐文件重算长度和 SHA-256 匹配；每包保留 `manifest.json`，未包含凭据、profile、字体或可执行文件。共享原始产物超过 64 MiB，采用按轮分卷，未放宽单包 64 MiB／768 文件及单文件 8 MiB 限额。

### 第一轮（2026-10-15）

- `npm run check` 通过：前端 190 项、核心单元 103 项及相关集成／doctest、普通桌面 33 项、解析器 270 项、hooks 13 项、测量脚本 5 项、开发脚本 11 项；普通前端产物共 3 个文件，隔离检查通过。日志 `.local/render-check-all.log`。
- 实验 feature 的桌面／核心 Clippy 通过；实验核心 3 项（真实几何／文字／alpha、预算、vmsk/vsms 等价）和实验桌面 56 项通过；普通开发 MCP 55 项及 DTO 检查通过。实验 DTO 生成一致性通过。日志 `.local/render-feature-check.log`。
- 本机首轮真实 MCP：`.local/dev-mcp/render-lab-261015/webview-eC103J/report.json` 通过，保留 9 张引擎／倍率截图、对比和重开截图。已查看三个 100% 截图：矩形、曲线、alpha 与文字均实际绘制；LeaferJS／Konva 较接近，当前 CanvasKit 适配的文字基线和分段间距存在可见差异，尚不能归因为引擎能力或 Photoshop 保真优劣。
- 当前环境 Windows x64、WebView2／EdgeDriver 155.0.4283.45、真实 DPR 1.5。倍率测试不是 100%／200% 系统 DPI 实测。当前普通 release／实验 debug 均构建成功；最终三轮报告 `.local/dev-mcp/stability-RfHAjx/summary.json` 为 passed，7/7、实例身份互不重复、自动重试 0，所有进程检查无残留。
- 产物白名单副本 `.local/dev-mcp/render-evidence-261015/`：60 个文件／6,548,360 字节，重新逐文件核对 SHA-256，全部匹配；不包含连接文件、配置、WebView profile 或字体文件。源 PSD 前后哈希一致。查看最终 CanvasKit 400% 和三方案对比截图，放大的文字保持重新绘制，未将合成位图当作文字输出。
- 最终微调补齐关闭自动选择后的悬停清除、受限平移绘制合并及迟到选择错误的代次保护；重建时类型检查通过，MCP 三轮已覆盖最新程序。
- 最终 lint、完整只读格式检查与 diff 空白检查通过；本轮涉及文档的 100 个相对文件链接均存在。未提交或推送。

## 下一步与限制

- 引擎对比到此收敛为 Leafer 首版主候选。下一项是单独规划正式工作区接入：复用 Rust 权威状态、保留合成参考回退、明确支持层与命中精度，制定真实字体／不支持效果提示。用户本轮未要求执行正式迁移。
- 迁移继续使用本次最小视觉、大视口与资源回归；重点跟踪 2000 小位图的较高内存与早期失败，不把最终 3/3 扩大为长期稳定保证。Konva 的失败只说明当前适配与本机采样未通过门槛，不能认定框架固有泄漏；CanvasKit 暂保留专项对照。
- 目前只有 Windows／DPR 1.5 的证据；没有物理屏幕呈现、GPU 显存、跨屏多 DPI 或长时无泄漏证明。生成中文／换行／旋转样本不等于 Photoshop 文字真值；命中仍为共享实现。
- M2-02／M2-03 与 e30ce2e 远端 CI 本轮未重新验收。新实验特性尚无远端 CI 结论。改动仅在本机，未提交或推送。

## 过程记录

### 2026-10-16

- 第三轮用户授权深入测试，要求开始真实焦点操作前告知。提前固定视觉与资源门槛；分别取得空闲窗口确认，测试后明确恢复操作。没有通用电脑控制或注入组件状态。
- 初次视觉 `stability-Z5NlJP` 误用了依赖正式标题栏按钮的等待条件；改为原生尺寸等待后核对真实前端视口。`stability-HjTKzO` 发现 Leafer／Konva 默认位图平滑采样与 CanvasKit 不一致，统一 nearest 后使用原阈值通过，两个失败保留。
- 普通素材导出准入比场景读取更严格；新增独立实验像素读取，不修改正式导出规则。灰度蒙版保留矩形外默认值与源 alpha，图层 opacity 单独应用一次；kind=128 全局蒙版只使用逐层值。多子路径孔洞／布尔叠加继续拒绝并加反例，防止半透明交叠错误被当成通过。
- 真实 Arial 点文字原先有基线偏移；使用 Leafer 公开文字布局、Konva 现代基线度量定位，自动行距仅取源明确比例。字体由本机显式文件校验 PostScript 名称和 hash，禁止缺字体静默替换。新增 `public-mask-disabled` 仍被解析器严格模式拒绝 Unread section data，保留来源与诊断；没有放松解析模式，最小蒙版验收使用完整可解析的 `public-mask`。
- 首个大视口 `stability-AJ6Pmb` 真实失焦发生在脚本重复设置已拥有的焦点；修正为双层焦点预检并在必要切换时等新事件，保留失败；后续 6/6。初始内存门槛失败后，显式 `--keep-going` 完成剩余预定独立试验，失败仍计数且批次失败，不自动重试。
- 资源增长主要位于 WebView，应用自身约 27–30 MiB；当前没有足够证据区分 GC、解码缓存或真实泄漏。保留有效失败与有限样本结论，正式迁移不得删除资源验收。

- 用户授权按选型补测计划继续实施。保留 Konva 的条件性工程倾向，视觉准确性和交互正确性作为选型通过条件；不以首轮小样本计时宣布性能胜者。
- 本轮入口：独立 render-lab 与开发测试 MCP。先补受限规模场景、原生段落模式、统一绘制就绪观测、真实输入延迟和进程资源采样，再运行冷／热分组和顺序轮换。PNG 对比快照移出连续交互路径。
- 样本分开记录：公开真实 PSD 与已有独立参考用于能力／保真缺口；生成的中文、旋转、段落及 100／500／2000 层样本用于可重复工程测量。缺字体或未实现蒙版明确保留失败／缺口，不静默替换字体或把生成图作为 Photoshop 真值。
- 验收：三引擎真实 MCP 回归、规模与排版报告、资源释放／重开、源哈希、正式构建隔离及受影响契约／静态检查。性能报告分别记录加载、资源准备、构建、绘制同步、输入与进程内存；不宣称物理屏幕呈现或完整 Photoshop 兼容。
- 接手工作区为第一轮未提交改动；继续本机实现，未授权提交推送。
- 中断后用户明确授权继续，先用安全封装修复内存采样。初次 Win32 FFI 被仓库 `unsafe_code = "forbid"` 拒绝；改为 `sysinfo 0.35.1` 可选依赖、关闭默认特性，仅启用 Windows system。没有降低 lint 或更改安全规则。
- 新增 12 个确定性生成场景：路径／文字／位图各 100／500／2000 层、长页、大位图和中文排版。6 个公开 PSD 复用已下载的 psd-tools 固定提交，先核对来源 SHA-256；参考 PNG 另核对指纹。仅写入 `.local`，源文件保持不变。中文子集 36,676 字节，来源、OFL 和生成器见实验 assets 说明。
- 第一、二次原生段落诊断分别发现 CanvasKit 默认 textStyle 与 ParagraphStyle 字段未初始化；按真实 API 使用 ParagraphStyle／TextStyle 构造器后通过。失败保留于 `.local/dev-mcp/stability-PXMnuI/`、`stability-aS5Ieb/`，正常关闭且无残留。
- 100 ms 固定移步不足以观察连续绘制，先扩展 MCP 的 `moveTimed`（1–100 ms，最多 32 步），通过边界契约后使用 16 ms 请求。一次规模批次在真实 `blur → lostpointercapture` 后按设计取消平移，保留 `stability-kHduFt` 的 3 个已通过样本和 1 个失败；用户确认暂不操作后，增加 MCP 原生焦点确认与手势期间失焦计数断言，重新建立完整批次，无自动重试。
- 正式数据均来自 release；三轮轮换首用引擎，热构建各三次、每倍率各三次，连续平移不截图／不读回。公共同步绘制屏障不等于物理显示；首用初始化每个具体场景每引擎只有一项，不用它计算可靠冷启动分布。
- 实测推翻了简单的统一性能排序：Leafer 密集文字较快，CanvasKit 路径较快；2000 位图下 CanvasKit P95 波动较大，Leafer 进程内存较高。原生排版的混合样式回退按实际能力计数。
- 公开真实样本仅 `2layers` 无未绘制层，其余五个为能力缺口；独立蒙版参考的三轮／三引擎结果一致，约 72.9% 可见采样像素差异超过阈值。正式工作区保持原合成图实现。

### 2026-10-15

- 用户确认三渲染器实验方案并授权文档后实施；本轮仅针对开发实验，保留原预览及现有选择语义。
- 现有测试进程已拥有独立目录、WebView profile、身份、选择服务和真实驱动。采用专属实验页面替换该隔离进程的页面，可复用固定 `main` 权限，避免扩大至任意窗口控制。
- 从 npm 官方元数据核对并精确锁定：LeaferJS 2.3.1（MIT）、Konva 10.9.0（MIT）、CanvasKit 0.42.0（BSD-3-Clause）；没有引入第二套 PSD 解析器。实验本地字体为 OFL，来源和哈希见 `src/render-lab/assets/README.md`。
- 样本回归发现上游 `vmsk` 直接 handler 被双向别名错误遮蔽；按 patch 4 优先直接 handler，并验证 `vsms` 等价。上游仍按 1×1 返回归一化点，实验适配器按文档尺寸转换，整数几何独立回归通过；正式支持声明不扩大。
- 保留失败尝试：最初 Rust 测试因 vmsk 缺项失败；Tauri 构建发现 builder 无 `url` 方法，改为复制窗口配置设置 URL；首次 WebView 握手因响应式数组不能 `structuredClone` 失败（`webview-ZmLlsn/report.json`），改为只读普通数据投影后通过。没有跳过失败或伪造绘制完成。
- 安装审计发现既有 `source-map-js 1.2.1` 高严重性 source-map 拒绝服务告警；已通过 `git show HEAD:package-lock.json` 确认基线存在，本轮未混入自动升级。该包是既有工具链传递依赖，另行按依赖升级评审处理。
- 首轮实现比较：适配器约 LeaferJS 101／Konva 83／CanvasKit 128 行（格式化后的阶段计数，不作为质量评分）；前两者组织节点与变换，CanvasKit 显式管理 WebGL、Surface、Typeface、Font、Paint、Path 与 Image。共享数据校验、字体／alpha、分段定位和交互代码单独计入共同成本。
- 最终三轮保留 27 组倍率观测。独立的首次 Leafer 初始化约 22.7–23.4 ms，首次绘制就绪含其图像任务；后续切换为热路径。CanvasKit 首次初始化约 408.5–425.0 ms，包含 WASM／WebGL／字体对象初始化；这些是本机小样本读数。固定顺序尚未轮换，绘制提交与引擎就绪口径有差别，不用这些数值排名。后续比较须补冷／热分组、顺序轮换、场景加载分段与 RSS。
- 常规实验构建包中，三适配器入口约 LeaferJS 214 KB、Konva 191 KB、CanvasKit 121 KB，CanvasKit 另加载约 7.32 MB WASM；固定字体约 2.05 MB 为共同资源。数值为本轮 Vite 未压缩产物，不是安装包、下载压缩量或运行内存。
