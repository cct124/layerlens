# LayerLens

基于 Rust + Tauri 的开源 PSD 解析与 AI 协作工具，目标是通过交互式选区、图层样式提取和素材导出，为编程 Agent 提供结构化设计上下文，辅助还原 H5 页面。

当前工程为 **桌面脚手架与 M0-01 PSD 解析原型**：桌面包含 Vue 工作区入口、Rust 应用信息调用和错误反馈；独立 Rust 核心提供实验性的简单 PSD 元数据、合成预览及透明 PNG 验证。原型尚未接入桌面，图层查看、完整素材服务和 MCP 属于后续开发范围。设计见 [docs](docs/README.md)，当前任务与验证记录见 [devlog](devlog/README.md)。

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

`npm ci` 的 `prepare` 生命周期会安装 Husky 管理的 Git hooks。首次启动会通过 rustup 获取固定的 Rust 工具链，并通过 Cargo 下载依赖。桌面启动后自动检查前后端通信，成功显示“桌面应用可用”和“基础通信正常”；失败可使用“重新检查”重试。

```powershell
npm run dev:web       # 浏览器预览，仅用于前端展示；不提供桌面 IPC
npm run desktop:build # 构建 release 桌面程序及 Windows NSIS 安装包
```

开发服务器只监听 `127.0.0.1:15420`，端口被占用时明确退出，不自动换端口。遇到 Windows 保留端口导致的 `EACCES`，可用 `netsh interface ipv4 show excludedportrange protocol=tcp` 查看；调整端口时同步 Vite、Tauri `devUrl` 和开发 CSP。NSIS 安装包输出到 `target/release/bundle/nsis/`，可执行文件为 `target/release/layerlens.exe`。首次打包需要下载 NSIS 工具；当前未配置发布签名，安装包用于开发验证。其他操作系统尚未纳入验证范围。

## 检查与测试

```powershell
npm run check          # 仓库格式、前端与 Rust 检查、DTO 一致性及 hook 测试
npm run check:frontend # Prettier 格式、严格类型、lint、组件测试及前端构建
npm run check:rust     # rustfmt、Clippy、Rust 测试及 DTO 一致性检查
npm run test:hooks     # 暂存内容检查及部分暂存保护测试
npm run test:watch     # 交互式前端测试
cargo test --locked -p layerlens-core # 单独验证不依赖 Tauri 的核心
```

Windows CI 使用相同的完整检查入口并构建 NSIS 安装包；新增或变更的流水线需以远端实际运行结果为准。

## PSD 解析实验

本轮精确使用 `ag-psd 0.3.0`（上游仓库名 `ag-psd-rs`），第三方类型隔离在核心适配器内。实验只接纳预检覆盖的 PSD v1、RGB／8 位、RAW／RLE 简单结构；文字、ICC 和其他未验证的附加信息、蒙版、ZIP 及其他模式暂时明确拒绝，不能据此推断候选库本身均不支持。无保存时合成图时仍可查看图层元数据，但不自动重新合成。预览仅开放三通道且不含合成透明度标记的输入，其他情况保留元数据和可用的图层导出，并说明候选限制。

```powershell
npm run fixtures:check # 检查独立合成样本、预期值和指纹是否可复现
New-Item -ItemType Directory -Force .local | Out-Null
cargo run --locked -p layerlens-core --example inspect_psd -- crates/layerlens-core/tests/fixtures/psd/bitmap-raw.psd .local/psd-raw-report
```

输出目录必须不存在。命令生成 `report.json`、可用的 `preview.png` 和逐层 `layer-<id>.png`；报告记录源 SHA-256、规范化元数据、能力限制、解析及逐项解码／编码耗时。简单可见位图以 1× 导出，保留画布外部分和透明边缘；隐藏层、组和未验证的混合／不透明度依赖不导出。失败项保留错误原因并使命令返回非零，不覆盖已有文件。

Windows 读取期间限制并发写入和替换，后续按需解码只使用已取得的压缩数据。默认准入上限为源文件 64 MiB、画布与图层累计 16 Mi 像素、4096 条图层记录，另限制组嵌套 64 层；这些是实验保护阈值，不是全进程内存硬上限或性能承诺。尚未完成真实 H5 稿、文字样式、ICC 色彩对照和规模性能验收。样本来源、独立预期与覆盖范围见[样本说明](crates/layerlens-core/tests/fixtures/psd/README.md)。

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

Prettier 的范围包含 `docs/`、Markdown、前端代码及 `src-tauri` JSON 等受支持文件；生成 DTO、锁文件、编译产物、`.local/` 和 Husky 内部生成目录 `.husky/_/` 由忽略配置排除。Rust 通过 `.rustfmt.toml` 明确使用 2024 edition 与 2024 格式风格。

Husky 的 `pre-commit` 调用 `format:staged`。检查器直接读取 Git 索引中的文件内容，通过 Prettier API 或 rustfmt 标准输入检查，通常只检查暂存的新增、修改、重命名和复制文件，忽略删除项；暂存的格式配置发生变化时，检查范围扩大到整个索引。文件名中的空格和中文按完整路径处理。

提交检查只读，不自动格式化、stash 或 `git add`，因此不会把未暂存修改带入提交。发现格式问题后，在工作区修复并确认 diff，再按需要重新暂存对应文件或代码片段。Husky 未安装或被跳过时，本机 hook 无法强制阻止提交；CI 仍执行完整检查。远端是否强制要求 CI 通过取决于仓库分支保护配置，不能仅凭本地 hook 或流水线文件宣称已启用。

## 代码组织

| 路径                      | 职责                                                      |
| ------------------------- | --------------------------------------------------------- |
| `src/features/workspace/` | 工作区界面、状态检查流程及组件测试                        |
| `src/shared/api/`         | 桌面调用、运行时响应校验及生成的 TypeScript DTO           |
| `crates/layerlens-core/`  | 独立 Rust 核心、协议约束与领域错误                        |
| `src-tauri/`              | Tauri Commands、窗口、权限及打包配置                      |
| `scripts/`                | 暂存格式检查、hook 测试、开发资源生成脚本及应用图标源文件 |
| `.husky/`                 | 受版本管理的 Git hook 入口；内部生成目录忽略              |
| `.github/workflows/`      | Windows 检查与安装包构建                                  |

前端通过 `get_app_info` Command 调用核心，只有 `main` 窗口获得该命令权限。核心不依赖 Tauri；协议版本不匹配返回结构化错误。Rust mock IPC 测试覆盖成功、错误序列化和窗口权限，组件测试覆盖展示、错误重试、浏览器预览及非法响应。

## 生成文件

```powershell
npm run bindings:generate # 从 Rust DTO 生成 src/shared/api/generated.ts
npm run bindings:check    # 检查生成文件是否与 Rust 定义一致，不写文件
npm run icons:generate   # 从 scripts/app-icon.svg 生成配置引用的桌面图标
```

生成的 TypeScript DTO 和桌面图标提交仓库；修改来源后重新生成。Tauri 自动权限与 Schema、编译缓存及临时图标输出均忽略。协作与开发要求见 [AGENTS.md](AGENTS.md)。
