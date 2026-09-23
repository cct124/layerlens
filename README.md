# LayerLens

基于 Rust + Tauri 的开源 PSD 解析与 AI 协作工具，目标是通过交互式选区、图层样式提取和素材导出，为编程 Agent 提供结构化设计上下文，辅助还原 H5 页面。

当前工程已完成 **M0 PSD 解析原型与 M1 多文档桌面查看器的有限范围验收**：桌面已接入文件选择、打开／切换／关闭／重载、作业取消、保存时合成预览、每份文档的缩放和平移，以及图层树、只读属性和画布定位。独立 Rust 核心统一管理文档修订、有界后台、LRU 缓存与资源计费，并提供 PSD 图层分类、分段文字样式和能力诊断。M1 完整自动检查与公开样本核心回归通过；2026-09-20 用户完成 Windows 开发版桌面手测，反馈验收清单各项正常。此结论不代表完整 PSD 兼容、新一轮性能实测或安装发布验收。

**M2-01 已实现核心选区、不可变快照与任务绑定；M2-02 已接入桌面图层勾选、矩形框选、边缘吸附和任务闭环**：可新建、移动和调整区域，松开后提交；可固定／释放任务、复制当前会话引用，以及按固定快照查看内容。名称点击仍只检查属性。M2-03 已实现主动资源清理的核心预检／确认与任务失效，桌面清理入口及确认弹窗尚未接入；新的 Windows 原生验收仍未完成，不能视为 M2 整体出口完成。产品化素材导出及 MCP 仍属后续范围。设计见 [docs](docs/README.md)，当前阶段与验证记录见 [devlog](devlog/README.md)。

## 开发环境

- Windows x64，Visual Studio 2022 的“使用 C++ 的桌面开发”工作负载及 Windows SDK。
- Microsoft Edge WebView2 Runtime；开发环境要求见 [Tauri Windows 前置条件](https://v2.tauri.app/start/prerequisites/#windows)。
- Rust **1.96.0**，由 `rust-toolchain.toml` 固定，包含 rustfmt、Clippy。
- Node.js **24.19.0**、npm **11.17.0**，由 `.node-version`、`package.json` 记录；已有 Node 后可执行 `npm install --global npm@11.17.0` 安装对应 npm。

Tauri Rust **2.11.5**、CLI **2.11.4**、API **2.11.1** 是本次初始化时核对的稳定版本，组件版本号不要求相同。前端使用 Vue 3.5.42、Vite 8.3.0、TypeScript 6.0.3；TypeScript 版本依据类型检查与 lint 工具兼容范围选择。npm 和 Cargo 锁文件随仓库提交，升级需重新验证。

## 启动与构建

在仓库根目录执行：

```powershell
npm ci
npm run dev
```

`npm ci` 的 `prepare` 生命周期会安装 Husky 管理的 Git hooks。首次启动会通过 rustup 获取固定的 Rust 工具链，并通过 Cargo 下载依赖。桌面启动后订阅工作区状态，成功显示“工作区已连接”，通过“文件 → 打开 PSD”选择本机文件；通信失败可使用“重新连接”。同一路径复用标签，磁盘变化须使用“文件 → 重新载入”或文档面板“从磁盘重新加载”，失败保留旧内容。浏览器预览不调用桌面能力。

工作区采用参考 Photoshop 布局的中性深灰界面：顶部“文件／视图／窗口”菜单和下方工具选项、左侧工具、中央画布、右上“属性／文档”及右下“图层／任务”。文件菜单负责打开、重载和关闭文档；视图菜单负责适应窗口和 100%；窗口菜单切换四个面板或重置面板布局。各面板独立滚动，点击图层名称在上方检查属性，勾选加入任务范围；顶部“查看任务”可随时进入任务页。底部保留缩放入口，可见性图标只读，不提供原稿编辑。切换或调整面板保留搜索、滚动和任务名称草稿。

右侧左边缘可拖动调整宽度，上下分区交界可拖动调整高度；采用透明热区，不增加明显分隔线或悬停高亮，仅切换调整光标。双击对应边缘恢复该方向默认尺寸，“窗口 → 重置面板布局”恢复全部尺寸。拖动中 Esc 取消本次调整，不清空选区；键盘聚焦边缘后可用方向键及 Home／End 调整。本机记住宽度和上下比例，重启恢复尺寸，但不持久化任务、面板活动标签或输入草稿。具体约束见[产品与交互](docs/01-产品与交互.md#界面布局)。

快捷键：`V` 移动工具（会清空当前范围），`M` 区域选择，`Ctrl+O` 打开 PSD，`Ctrl+0` 适应窗口，`Ctrl+1` 100%。编辑输入框时不触发这些快捷键；保留选区临时平移请按住鼠标中键。当前仅提供已实现的操作，不显示占位的 Photoshop 编辑菜单。

菜单访问键为 `Alt+F`／`Alt+V`／`Alt+W`；打开菜单后可用方向键导航，Esc 仅收起菜单，不清空选区。桌面菜单与自定义标题栏合并：拖动顶部空白或品牌区域移动窗口，双击最大化／还原，右上角提供最小化、最大化／还原和关闭按钮。菜单和窗口按钮不参与拖动；关闭窗口沿用应用退出清理，区别于文件菜单的“关闭当前文档”。浏览器预览不显示窗口按钮。

标题栏配置与权限变更需要重新启动 `npm run dev`，仅前端热更新不会移除旧窗口的系统标题栏。重启会结束当前会话及固定任务。Windows 多屏、DPI、边缘缩放和系统贴靠仍按[验收清单](docs/06-实施与验收.md)单独验证；不承诺 Windows 11 最大化按钮悬停的原生 Snap Layouts 弹层。

左侧“区域选择”工具可在预览上拖出矩形，框内拖动移动范围，八个边角控制点调整尺寸；“重新框选”允许从已有范围内部新建。松开鼠标后确认；按住中键可临时平移并保留已提交范围，松开后继续框选。Esc、开始中键平移、缩放、切换标签、重载或窗口失焦取消未提交草稿。点击左侧移动工具则取消草稿并清空当前文档的范围，核心确认后进入平移模式；失败保留原模式并提示，其他文档选区及固定任务保留。右侧任务面板可查看命中图层、文字和样式并固定任务，技术标识默认折叠。任务仅在本次运行期间保留，目前不自动调用 AI。

区域工具选项中的“边缘吸附”默认开启，新建的拖动终点、移动或调整的边缘可吸附画布及当前有效可见的非组图层几何边界，显示参考线和目标名称；按住 Alt 临时关闭，松开恢复。开关只在本次运行中保留，跨文档共用，不提交选区或改变固定任务。图层列表未完整或读取失败时明确提示仅画布吸附；拖动开始后固定目标集合，晚到图层用于下一次手势。几何边界不等于包含效果的视觉边界；距离和规则见[区域选择与吸附](docs/01-产品与交互.md#区域选择与吸附)。

桌面沿用核心默认资源额度，不开放实验提额开关。超过单文件 64 MiB 或累计声明像素等限制时明确拒绝；多选触及核心队列上限后停止提交剩余文件，并提示稍后重新选择。画布显示保存时合成图，保留未执行 ICC 转换等限制说明。原稿数据保持只读，不生成 CSS。

```powershell
npm run dev:web       # 浏览器预览，仅用于前端展示；不提供桌面 IPC
npm run desktop:build # 构建 release 桌面程序及 Windows NSIS 安装包
```

开发服务器只监听 `127.0.0.1:15420`，端口被占用时明确退出，不自动换端口。遇到 Windows 保留端口导致的 `EACCES`，可用 `netsh interface ipv4 show excludedportrange protocol=tcp` 查看；调整端口时同步 Vite、Tauri `devUrl` 和开发 CSP。NSIS 安装包输出到 `target/release/bundle/nsis/`，可执行文件为 `target/release/layerlens.exe`。首次打包需要下载 NSIS 工具；当前未配置发布签名，安装包用于开发验证。其他操作系统尚未纳入验证范围。

区域工具下，画布获得焦点后按 Esc：正在拖动时取消本次手势；没有拖动时清空当前范围并保持区域工具，可直接重新框选。按住 Esc 不会将取消拖动升级成清空；编辑任务名称时不触发此画布快捷键。

## 检查与测试

```powershell
npm run check          # 仓库格式、前端与 Rust 检查、DTO 一致性及 hook 测试
npm run check:frontend # Prettier 格式、严格类型、lint、组件测试及前端构建
npm run check:rust     # rustfmt、Clippy、Rust 测试、DTO 一致性及 Windows 测量脚本回归
npm run test:parser    # 本地 ag-psd 补丁回归；也纳入 check:rust
npm run test:measurement # Windows 测量会话、目录保护及失败清理；也纳入 check:rust
npm run test:hooks     # 暂存内容检查及部分暂存保护测试
npm run test:watch     # 交互式前端测试
cargo test --locked -p layerlens-core # 单独验证不依赖 Tauri 的核心
```

Windows CI 使用相同的完整检查入口并构建 NSIS 安装包；新增或变更的流水线需以远端实际运行结果为准。

## 核心文档服务

Rust 的 `documents::DocumentService` 管理标签顺序、活动文档、打开／重载作业及不可变修订。重复路径复用文档或已知加载请求，后台规范化后合并路径别名；打开失败保留已有内容，较早请求完成不会抢占后来打开或手动切换的焦点。关闭活动标签优先切换到右邻，其次左邻。

`DocumentLease` 固定 PSD 修订的原始事实、单位、来源与能力诊断；重载、关闭及服务退出不改变已有引用。当前不暴露绕过调度的解码入口，也不生成 CSS。`OpenJob` 提供状态、排队／执行耗时和完成结果；句柄丢弃不自动取消。只有显式 `cancel` 被接受才保证该作业不发布成功。

同一租约提供只读图层检查：列表按页（单页 ≤128 条、响应 ≤256 KiB）读取，详情一次只取一个图层，文字按 UTF-16 分页（≤2048 code unit、每类 ≤128 段样式）续读且不拆分代理对。详情按完整 JSON ≤256 KiB 缩短文字页，保留原文绝对样式区间；单项属性或不可分页元数据超限时明确拒绝。`select_layer` 只在当前修订内提交查看器的检查单选，不建立任务授权；重载、关闭与旧修订的引用被拒绝。完整图层与文字不进入工作区通知，桌面只按 `nextOffset`／`textStart` 续读。

打开和预览共用 **1 个后台线程、4 个未完成作业**，按提交顺序执行；默认最多 **8 个存活或预留修订、256 MiB 源字节额度、64 Mi 声明像素额度**。解析开始前按单文件上限预留，成功后按实际源字节和声明像素结算；旧修订的额度随最后一个引用释放。单文件限制沿用 M0，保守预留可能拒绝一个实际很小的输入。

`preview(document_id)` 请求当前固定修订的原尺寸保存时合成 PNG，结果保留 `partial` 等能力限制。相同修订的在途请求共用 `PreviewJob`，显式 `cancel_preview` 会取消整个共同作业；命中缓存无需工作线程或空闲作业名额。缓存 key 包含文档、修订和预览种类，使用 LRU 淘汰。成功重载或关闭会使旧缓存失效，失败重载保留原内容。

预览默认额度为 **128 MiB 解码预留、64 MiB 单张 PNG、128 MiB 存活输出总量、64 MiB／16 项缓存**。执行前预留完整单文件解码额度和单张 PNG 上限，编码成功后按 PNG 分配容量结算；输出紧张时先淘汰缓存，外部仍持有的图像继续计费，不足则明确失败。`PreviewLease` 只保留 PNG，不延长源 PSD 的寿命；完成的作业句柄、结果和状态副本也可能持有 PNG，最后一个引用释放后才归还额度。这些是保守准入限制，不覆盖全部编解码器临时开销、分配器、WebView 或进程 RSS。

排队取消在清理后完成，执行中的取消须等待同步解析／解码返回并释放未发布数据。发布后的资源清理期间状态为 `Finishing`，不再接受取消，仍占用作业额度。`request_shutdown` 停止准入、请求取消，并在锁外释放预览缓存和排队引用；`shutdown`／析构等待线程退出。适配层须在后台执行阻塞等待、退出及可能销毁大修订或图像的操作，不能放在 UI 线程。详细约定见[技术架构](docs/02-技术架构.md#当前核心文档与预览实现m1)。

Windows 本机只读烟测入口（可传多个路径；输出 JSON Lines，不写入设计稿或预览）：

```powershell
cargo run --locked -p layerlens-core --example document_lifecycle -- "design-a.psd" "design-b.psd"
cargo run --release --locked -p layerlens-core --example document_lifecycle -- --large ".local/psd-samples/tab-1.psd"
cargo run --release --locked -p layerlens-core --example document_preview -- --large ".local/psd-samples/tab-1.psd"
```

两个示例的 `--large` 仅用于显式实验：单文件上限改为 512 MiB／128 Mi 声明像素，全局改为 1 GiB／384 Mi 声明像素，其他默认值不变。生命周期烟测将所有文档保留到关闭阶段，并持有首个修订核对关闭后的计费和最终归零。预览烟测依次请求首张与缓存图像，保留 PNG 后关闭文档，再释放图像并核对源／输出账本；不把图片写入磁盘。预览示例的 `--checkpoint` 在每个阶段输出后等待 stdin 换行，供外部进程采集内存，等待不计入请求耗时。本机 5 份真实样本的串行测量、OS 缓存条件及限制见[M1 记录](devlog/_fin/260920/M1-01-文档生命周期与后台任务.md#真实-psd-预览与释放基线)。示例计时不代表桌面首帧可见耗时。

## 核心选区与任务

`documents::selection` 复用同一 `DocumentService`，独立于查看器检查单选。先通过 `user_selection` 取得活动文档与全局选择版本，再使用固定 `DocumentLease` 调用 `prepare_selection` 在锁外计算多图层或单矩形范围，最后由 `commit_selection` 再次核对版本并原子发布。无效、过期或超限提交保留原选择，相同规范化选择不新增快照；清空使用明确的 `clear_selection`。范围计算可能扫描元数据，后续适配层须在有界后台执行，不逐帧提交拖动草稿。

`create_task` 以会话、快照与客户端请求 ID 绑定固定范围；`task_snapshot` 取得本次读取的强引用，`release_task` 幂等释放。切换选区、关闭和重载不改变任务；释放后拒绝新的任务读取，已经取得的引用仍可完成。`SnapshotLease::content_page` 按固定范围分页，保留几何交集、原始边界、全文 UTF-16 区间及绝对样式索引，结构组不增加兄弟图层授权。

快照数量、范围字节、旧选择缓存和任务记录均受独立额度约束，不提高 M1 源修订／预览额度；已释放／失效任务仍保留有界幂等记录。桌面通过同一核心的 `SelectionHandle` 和独立有界后台接入，Rust `selection_contract` 生成 DTO，`selection_request` 校验完整请求／响应预算；核心页预算之外另留信封空间。工作区通知只包含与文档列表原子采样的选择摘要，文字不进入通知。默认值、限制与未实现项见[当前核心选区与任务实现](docs/02-技术架构.md#当前核心选区与任务实现m2-01)；MCP 尚未接入。

桌面图层名称点击与勾选分别用于检查和任务范围。选定范围后在侧栏固定任务，已有任务可查看固定内容、复制引用或释放；关闭全部文档后任务面板仍可使用。任务引用仅在当前会话有效，MCP 未实现前不能据此声称 Agent 已可读取。复制失败时可从只读引用框手动复制。任务记录默认最多 128 条，含已释放／失效记录；释放不恢复记录名额，也不保证其他引用立即归零。

### 主动资源清理（当前仅核心）

`DocumentService::prepare_cleanup(document_id)` 返回不固定大文档的只读影响计划；`commit_cleanup(&plan)` 在同一状态锁内重检影响，变化则拒绝并要求重新确认。同一计划成功后重试幂等；只作用于该文档 ID 的全部存活修订，不按路径误清理关闭后重新打开的新文档。确认关闭目标标签、将活跃任务标记为 `Invalidated`、撤销快照查找并取消相关加载／预览；普通关闭和任务释放保持原有语义。

清理后旧租约的图层／内容读取在检查点返回失效错误；已返回的不可变元数据、PNG 或已导出文件不能撤回。同步计算和外部强引用仍按生命周期计费，回执不表示内存已归零。当前仅提供 Rust 核心 API 与终态 DTO／界面兼容，不提供桌面清理命令或按钮，后续需有界保管计划、用户确认及晚到响应处理。详见[核心清理架构](docs/02-技术架构.md#当前核心主动清理m2-03)。

## PSD 解析实验

首版支持子集采用 `ag-psd 0.3.0 + LayerLens patch 3`，通过 Cargo 本地补丁固定在 `vendor/ag-psd/`；来源、许可证和修改范围见[补丁记录](vendor/ag-psd/LAYERLENS-PATCHES.md)。第三方类型隔离在核心适配器内，支持范围、已知限制与后续条件见[解析器决策](docs/07-PSD首版支持与解析器决策.md)。

实验接纳预检覆盖的 PSD v1、RGB／8 位、3／4 个合成通道及 RAW／RLE 像素。未知资源和附加块在已校验的边界内跳读，并记录诊断；明确未实现的渐变等能力可局部降级，结构损坏、越界和预算错误仍使读取失败。图层区分组、文字、位图、形状、智能对象、调整层和未知类型。

文字保留 TySh 原文、UTF-16 长度、CR/LF、首尾空白及原始矩阵；新增经完整区间校验的字符／段落样式、显式字体引用、RGB 值和属性来源。每类最多 16384 段；区间错误只关闭对应类别，无效属性返回 `null` 并附诊断，不补默认字体。度量单位暂标为 `unverifiedEngine`，不按 DPI 或矩阵换算为 pt、px 或 CSS；颜色未做 ICC 转换、字重和字体可用性未推断，文字样式仍为 `partial`。输出契约与验证边界见[文字与样式](docs/03-PSD数据与素材.md#文字与样式)。

工具持续负责原稿数据、来源、单位、变换和限制，目标项目的 CSS、布局、响应式策略及字体替代由 Agent 决定；这不是只适用于原型的临时限制。详见[产品职责边界](docs/01-产品与交互.md#目标与场景)。

预览使用保存时合成图，不重新渲染图层；支持有 global alpha 标记的合成透明度。ICC 仅记录存在性、大小及指纹，未执行颜色转换，因此该类预览标为 `partial`。缺失合成图或额外通道语义未验证时，预览明确不可用，不影响已读取元数据。ZIP、高位深、其他颜色模式及 PSB 尚未开放。

```powershell
npm run fixtures:check # 检查独立合成样本、预期值和指纹是否可复现
New-Item -ItemType Directory -Force .local | Out-Null
cargo run --locked -p layerlens-core --example inspect_psd -- crates/layerlens-core/tests/fixtures/psd/bitmap-raw.psd .local/psd-raw-report
```

输出目录必须不存在。命令生成 `report.json`、可用的 `preview.png` 和逐层 `layer-<id>.png`；报告记录源 SHA-256、解析器标识、显式预算、规范化元数据、逐项诊断和耗时。报告包含原文和图层名称，私有稿报告与派生图应存入被忽略的 `.local/`。简单可见位图以 1× 导出，保留画布外部分和透明边缘；隐藏层、组、复杂类型及含蒙版、效果或其他未验证视觉依赖的图层不作为独立素材导出。预览允许 `partial`，图层仅导出 `supported` 项；不支持项记录为跳过，实际解码或写入失败返回非零，不覆盖已有文件。

实验报告新增 `openTimings`（源读取、预检、候选解析、规范化、指纹、源缓冲释放及总耗时）、产物 `pngTimings`（解码／编码）和 `summary`（规模、能力及导出限制计数）。原有 `parseElapsedMs` 保留为含源读取与释放的打开总耗时，不能再与分项相加。图层 `exportBlockers` 保留可重叠的拒绝因素，原有主要原因不变；这些是未发布的核心／CLI 实验字段，尚未成为桌面或 MCP DTO。

Windows 读取期间限制并发写入和替换，后续按需解码只使用已取得的压缩数据。默认上限为源文件 64 MiB、画布／图层／蒙版累计 16 Mi 像素、4096 条图层记录、64 层组嵌套；单次解码的 RGBA 与临时缓冲预算为 128 MiB，不含源数据、元数据与输出 PNG，也不是全进程内存上限。CLI 可通过 `--max-file-mib`、`--max-total-pixels`、`--max-decoded-mib`、`--max-layers` 显式调整，报告保留实际值。`--metadata-only` 只写报告，`--preview-only` 写报告和合成预览，两者互斥。

12 个仓库内合成样本由独立生成器维护，包含默认预算可打开的 512 层网格及文字单位／样式数量／响应字节分页样本；另在被忽略的 `.local/public-psd-260917/` 下载 21 份公开 PSD，默认预算下 15 份、显式实验预算下 17 份完成读取与预览。真实长页、中等页面、图层密集样本及既有私有稿已完成 release 性能和释放基线；新增 3 个上游 Photoshop 参考像素及 1 张配套 PNG 对照通过，仅证明对应保存时合成图数据。下载资源、参考图和派生产物均不进入仓库。Photoshop 文字排版／单位、ICC 与复杂素材仍待验证。能力矩阵、失败原因及实测记录见[M0-01 结果](devlog/_fin/260917/M0-01-PSD解析验证.md)，桌面 WebView 计时与验收边界见[M1 记录](devlog/_fin/260920/M1-01-文档生命周期与后台任务.md)，仓库样本的独立预期见[样本说明](crates/layerlens-core/tests/fixtures/psd/README.md)。

换机器后可按登记清单恢复公开样本；脚本逐文件校验长度与 SHA-256／Git blob，只写 `.local/`，不覆盖已有文件，也不执行下载内容：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/fetch-psd-samples.ps1 -OutputPath .local/public-psd-260919
```

`scripts/psd-samples.json` 固定 psd-tools 提交与逐文件指纹，并登记 GraphicBurger 模板；许可说明随资源留在 `.local/`，不随仓库分发。该批 53 份 PSD/PSB 的默认额度成功 36 份；12 份不符合首版 RGB/8 位、3／4 通道子集，1 份 PSB、3 份超预算被明确拒绝，提额后其中 2 份通过，另有 `1layer.psd` 报“图层信息区包含未声明的数据”待单独评估。盘点明细见[M1-02 记录](devlog/_fin/260920/M1-02-图层检查与测试样本恢复.md)。

### 性能与资源基线

在仓库根目录使用 Windows PowerShell 5.1 运行；需要可用的 CIM 系统信息查询和子进程权限。脚本先完成锁定依赖的 release 构建，再启动独立 CLI 测量，不将编译耗时计入基线：

```powershell
New-Item -ItemType Directory -Force .local | Out-Null
npm run bench:psd -- -InputPath crates/layerlens-core/tests/fixtures/psd/bitmap-raw.psd -OutputPath .local/baseline-raw -Mode all -Runs 6
```

`OutputPath` 必须不存在，父目录须已存在。`Mode` 为 `metadata`、`preview`（默认）或 `all`；`Runs` 为 1–100，默认 6。每轮重新打开并释放文档，后两种模式额外在同一文档上再次请求预览。这条同步适配器测量路径没有解码缓存，保留为 M0 对照；它不经过新增的 `DocumentService` PNG 缓存。源／像素预算通过 `MaxFileMiB`、`MaxTotalPixels`、`MaxDecodedMiB`、`MaxLayers` 显式调整，默认值与核心一致。

输出包含 `baseline.json`、`run-NNN/report.json` 和按模式生成的 PNG。基线记录系统与构建信息、源／可执行文件／锁文件指纹、各阶段耗时及 ready／opened／exported／released 内存检查点；首次打开单列，后续打开统计最小值、中位数和最大值。简报不复制图层名称、文字及完整属性，私有稿的 PNG 和所有报告仍应留在 `.local/`。

测量脚本每 25 ms 及检查点采集工作集与私有内存：峰值工作集是整个子进程的累计 OS 计数，私有内存峰值仅为采样最大值，释放效果看当时存活占用。系统文件缓存未清空，首次打开不能称为冷读；PNG 写盘未请求物理设备同步。会话总耗时包含检查点等待，核心阶段不包含等待和报告序列化。

默认进程保护为 `TimeoutSeconds=180`、`MaxWorkingSetMiB=1024`、`MaxPrivateMiB=1536`，超限终止本次测量子进程并返回失败；它们是外部观察阈值，不是分配硬上限或产品默认预算。基线只覆盖同步 CLI，不代表桌面响应、并发多文档、应用缓存或取消行为已经验收。

## 格式化与提交

```powershell
npm run format            # 格式化 Rust 和仓库内 Prettier 支持的文件
npm run format:check      # 完整格式检查，不写入文件
npm run format:rust       # 仅格式化 Rust
npm run format:rust:check # 仅检查 Rust 格式
npm run format:web        # 格式化 Prettier 支持的代码、配置和文档
npm run format:web:check  # 仅检查上述 Prettier 文件
npm run format:staged     # 检查 Git 索引中的待提交内容
```

Prettier 的范围包含 `docs/`、Markdown、前端代码及 `src-tauri` JSON 等受支持文件；生成 DTO、锁文件、编译产物、`.local/` 和 Husky 内部生成目录 `.husky/_/` 由忽略配置排除。PSD `manifest.json` 由 `fixtures:check` 逐字节校验，不另行格式化生成结果。Rust 通过 `.rustfmt.toml` 明确使用 2024 edition 与 2024 格式风格；格式入口只处理两个自有 crate，`vendor/` 保留上游源码格式以便审查补丁，暂存检查采用相同忽略规则。

Husky 的 `pre-commit` 调用 `format:staged`。检查器直接读取 Git 索引中的文件内容，通过 Prettier API 或 rustfmt 标准输入检查，通常只检查暂存的新增、修改、重命名和复制文件，忽略删除项；暂存的格式配置发生变化时，检查范围扩大到整个索引。文件名中的空格和中文按完整路径处理。

提交检查只读，不自动格式化、stash 或 `git add`，因此不会把未暂存修改带入提交。发现格式问题后，在工作区修复并确认 diff，再按需要重新暂存对应文件或代码片段。Husky 未安装或被跳过时，本机 hook 无法强制阻止提交；CI 仍执行完整检查。远端是否强制要求 CI 通过取决于仓库分支保护配置，不能仅凭本地 hook 或流水线文件宣称已启用。

## 代码组织

| 路径                      | 职责                                                       |
| ------------------------- | ---------------------------------------------------------- |
| `src/features/workspace/` | 工作区界面、状态检查流程及组件测试                         |
| `src/shared/api/`         | 桌面调用、运行时响应校验及生成的 TypeScript DTO            |
| `crates/layerlens-core/`  | 独立 Rust 核心、协议约束与领域错误                         |
| `src-tauri/`              | Tauri Commands、窗口、权限及打包配置                       |
| `scripts/`                | 格式与 hook 检查、PSD 测量及回归、开发资源生成与图标源文件 |
| `.husky/`                 | 受版本管理的 Git hook 入口；内部生成目录忽略               |
| `.github/workflows/`      | Windows 检查与安装包构建                                   |

前端通过 `get_app_info` Command 调用核心，只有 `main` 窗口获得该命令权限。核心不依赖 Tauri；协议版本不匹配返回结构化错误。Rust mock IPC 测试覆盖成功、错误序列化和窗口权限，组件测试覆盖展示、错误重试、浏览器预览及非法响应。

## 生成文件

```powershell
npm run bindings:generate # 从 Rust DTO 生成 src/shared/api/generated.ts
npm run bindings:check    # 检查生成文件是否与 Rust 定义一致，不写文件
npm run icons:generate   # 从 scripts/app-icon.svg 生成配置引用的桌面图标
```

生成的 TypeScript DTO 和桌面图标提交仓库；修改来源后重新生成。Tauri 自动权限与 Schema、编译缓存及临时图标输出均忽略。协作与开发要求见 [AGENTS.md](AGENTS.md)。
