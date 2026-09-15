# M0-00 Tauri 工程初始化

## 目标与验收

建立后续 PSD 解析原型可直接使用的最小工程，验证 Rust 核心、Tauri 适配层和 Vue 前端的边界；本任务不承担 PSD 查看器或 MCP 实现。

- 根 Cargo workspace 包含独立核心 crate 和 `src-tauri`，核心测试不依赖桌面运行时。
- Tauri Commands 保持薄适配层，`get_app_info` 校验协议版本并返回可识别错误。
- Vue + TypeScript 启用严格类型检查，最小启动页验证 IPC；DTO 以 Rust 为来源，通过 `ts-rs` 生成 TypeScript。
- 锁定 Rust、Node.js、npm 和应用依赖；建立本地检查及 Windows CI 配置。
- 记录格式、类型、lint、测试、类型生成一致性、构建和桌面启动结果；区分静态检查、真实 IPC 运行及视觉验收。

## 状态与基准

- 状态：已完成；固定工具链下的安装、统一检查、12 项测试、最终桌面打包、开发／release／安装后真实 IPC 及卸载清理均已通过，归档于 2026-09-15。
- 分支：`dev`。
- 代码基准：`0905c44`（`docs: refine multi-document PSD workflow and MCP contracts`）。
- 接手时工作区：干净；归档当时工程及配套文档尚未提交或推送，随后交付见[M0-00a 提交格式检查](M0-00a-提交格式检查.md)和 Git 历史。
- 阶段关系：本任务仅为 M0-00；后续 M0-01 仍须用 PSD 样本完成解析和导出验证。

## 已完成项

- 确认采用 Tauri 2、Vue 3 + TypeScript 严格模式和 Vite，使用独立 Rust 核心与薄桌面适配层。
- 已核验并写入依赖配置：Tauri Rust `2.11.5`、CLI `2.11.4`、API `2.11.1`，Vue `3.5.42`、Vite `8.3.0` 和兼容检查工具的 TypeScript `6.0.3`；npm、Cargo 锁文件已生成。
- 完成根 Cargo workspace、独立核心、薄 Tauri Commands、Vue 最小页和错误反馈；Rust DTO 已生成前端类型，Windows CI 配置使用 README 中的检查与构建入口。
- 固定 Node.js `24.19.0`、npm `11.17.0` 和 Rust `1.96.0`；精确 Rust 工具链已安装，实际为 `rustc 1.96.0 (ac68faa20 2026-05-25)`。最终按 README 原命令完成依赖安装、统一检查和打包，不再依赖临时的 `stable` 别名。
- 最终 release 程序和 NSIS 安装包已生成；开发模式、release 直接启动及安装后的真实 WebView2 页面均显示“桌面应用可用”，协议成功与不匹配路径均已通过 CDP 调用核验。测试安装已卸载并清理开发进程。
- 修复缺失 Windows Common Controls v6 manifest 导致的模拟 IPC 测试进程退出、保留端口冲突和本机浏览器 profile 引发的 Vite 文件监控错误。
- 同步架构选型和实施顺序，建立本任务记录与开发索引。

## 验证

验证环境：Windows x64，Node.js `24.19.0`、npm `11.17.0`，`rust-toolchain.toml` 固定并已安装的 Rust `1.96.0 (ac68faa20 2026-05-25)`。早期验证使用过同版本 `stable` 别名，最终已用精确工具链按 README 原命令执行 `npm ci`、`npm run check` 和 `npm run desktop:build`，全部通过。以下为本机结果，未替代远端 CI。

| 检查                                 | 当前结果                                                                                                                                                                           |
| ------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| npm 依赖安装与审计                   | 最终 `npm ci` 通过；审计 224 个包，报告 0 个漏洞；npm 与 Cargo 锁文件已生成                                                                                                        |
| 前端格式、严格类型、lint、测试和构建 | `npm run check:frontend` 全部通过，组件测试 7 项，Vite 构建成功                                                                                                                    |
| Rust 核心测试                        | 2 项通过，核心不依赖 Tauri 运行时                                                                                                                                                  |
| Tauri 模拟 IPC 测试                  | 3 项通过：成功结果、协议错误序列化和窗口授权；此前进程错误已修复，见过程记录                                                                                                       |
| Cargo Clippy                         | 固定 Rust `1.96.0` 下通过                                                                                                                                                          |
| Rust 格式与最终工作区统一检查        | `npm run check` 全部通过，含格式、Clippy、类型、lint、前端构建、DTO 一致性和 12 项测试（前端 7、核心 2、模拟 IPC 3）                                                               |
| Rust DTO 生成 TypeScript             | 生成及一致性检查通过；最后调整生成器以去除尾部空白后已重新生成，并通过对应生成一致性、Clippy 和 rustfmt 检查                                                                       |
| Tauri Windows release 与打包         | `npm run desktop:build` 通过；生成 `target/release/layerlens.exe` 和 `target/release/bundle/nsis/LayerLens_0.1.0_x64-setup.exe`；最后精简 Windows 图标配置后再次构建最终 NSIS 成功 |
| 图标生成资源一致性                   | 当前配置引用的 4 个 Windows PNG／ICO 经 `npm run icons:generate` 重新生成，前后 SHA-256 完全一致；移除未使用且字节不稳定的 macOS `icon.icns`                                       |
| 开发与 release 真实 WebView2 IPC     | `npm run dev -- --no-watch` 在 `15420` 实际启动；开发及 release 模式均通过 CDP 核验协议 1 成功、协议 2 返回 `PROTOCOL_MISMATCH`                                                    |
| NSIS 安装、安装后启动和卸载          | 静默安装到 `.local/install-smoke`，退出码 0；安装后页面与两种协议均通过；测试卸载退出码 0，注册项剩余 0、可执行文件已移除                                                          |
| 最小页视觉与运行证据                 | 桌面显示“桌面应用可用”，浏览器预览符合无桌面 IPC 的边界；截图清单见下文                                                                                                            |
| 精确 Rust 工具链                     | `1.96.0` 安装成功；下载故障及临时 curl 重试见过程记录，未将下载配置写为全局设置                                                                                                    |
| Windows CI                           | 配置已建立，尚未远端运行；不能以本机通过代替远端通过                                                                                                                               |
| 文档 diff、链接与设计一致性          | 已检查本次文档 diff，`git diff --check` 通过；相对 Markdown 链接目标存在，阶段和选型表述已同步                                                                                     |

真实 WebView2 检查使用本机辅助脚本 `.local/smoke-webview.mjs` 与 CDP，不是 CI 自动端到端测试。截图 `.local/desktop-dev.png`、`.local/desktop-release.png`、`.local/desktop-installed.png`、`.local/browser-preview.png` 均在本机忽略目录，不提交 Git。开发验证进程已关闭，测试安装已卸载。

PSD 准确性、导出效果、大图性能、MCP 互通和签名发布不在本任务验收范围，均未验证；归档时 Windows CI 已配置但尚未推送运行，后续结果以实际运行记录为准。

## 下一步与阻塞

1. 本任务已无验收阻塞，结果为可构建、可安装并能验证接口的最小桌面工程；下一优先任务为[M0-01 PSD 解析验证](../_plan/M0-01-PSD解析验证.md)。
2. 当前未提供实际 PSD 样本；在 M0-01 按[实施与验收](../../docs/06-实施与验收.md)补齐有来源说明和预期结果的样本，产出能力矩阵与差异记录，不能用脚手架通过替代。
3. 本任务归档时实现和记录尚未提交、推送或运行远端 CI；随后由[M0-00a 提交格式检查](M0-00a-提交格式检查.md)完成提交前门槛，实际交付状态见 Git 历史和交付消息。

相关本机改动：根工程与工具链配置、核心 crate、`src-tauri`、Vue 前端、Windows CI、项目 README，以及 `AGENTS.md`、`docs/02-技术架构.md`、`docs/06-实施与验收.md`、`docs/README.md` 和本次 `devlog/`。最终文件清单以交接时 `git status` 为准。

## 过程记录

### 2026-09-15

- 按用户授权先建立最新稳定 Tauri 的最小脚手架，再验证 PSD 解析；因此将原先“先解析，再建立工程”的计划拆为 M0-00 与 M0-01，查看器仍在后续 M1。
- 选择独立 Rust 核心，便于在没有桌面窗口时测试协议与后续解析逻辑；使用 Rust DTO 生成前端类型，避免手工维护两套接口。
- npm 注册表的 TypeScript 最新版本为 `7.0.2`，但 `typescript-eslint 8.70.0` 的 peer 范围要求 TypeScript `<6.1`，因此选择兼容的稳定版本 `6.0.3`，随后完成安装、锁文件和兼容性检查。
- 本记录建立时工程正在并行实现；未将待执行的静态检查、启动、IPC 或 CI 标为成功。
- 依赖安装完成，npm 审计 224 个包、0 个漏洞。前端格式、严格类型、lint、7 项组件测试及 Vite 构建通过；独立核心 2 项测试和 Cargo Clippy 通过，Rust DTO 生成成功。
- 初次 Tauri 模拟 IPC 测试进程以 `0xc0000139` 退出。为 Windows 测试宿主补充 Common Controls v6 manifest 后，成功响应、错误序列化和未授权窗口共 3 项模拟 IPC 测试通过；不能只依据核心直调测试替代此路径。
- Windows 将 TCP 端口区间 `1346–1445` 列为保留范围，原开发端口 `1420` 无法监听。现将 Vite、Tauri `devUrl` 与开发 CSP 统一为 `15420`；README 同步排查命令和需要同时修改的配置，仍保持端口冲突时失败而非自动换端口。
- 首次无头浏览器将 profile 放入 `.local/`，Vite 监视该目录时遇到 `EBUSY`。现将 `**/.local/**` 加入开发服务器的 watch ignore，避免临时浏览器数据参与源码监控。
- 已生成首个 NSIS 包 `target/release/bundle/nsis/LayerLens_0.1.0_x64-setup.exe`。直接启动 release 程序后，通过真实 WebView2 的 CDP 检查确认页面显示“桌面应用可用”，`get_app_info` 协议 1 成功、协议 2 返回 `PROTOCOL_MISMATCH`；本机截图保存在忽略目录 `.local/desktop-release.png`。
- 验证中途精确命名的 Rust `1.96.0` 工具链尚在下载，因此先以已安装且版本相同的 `stable` 别名完成部分检查，实际编译器为 `rustc 1.96.0 (ac68faa20 2026-05-25)`；当时没有将其表述为固定工具链的最终验证。
- Rust 工具链首次 reqwest 下载停滞。停止唯一的安装进程后，以临时 `RUSTUP_USE_CURL=1` 重试成功，精确 `1.96.0` 已安装；rustup 同时自更新至 `1.29.1`。该下载设置未全局持久化。
- 使用固定工具链按 README 原命令完成 `npm ci`、`npm run check` 和 `npm run desktop:build`。12 项测试及全部静态检查通过，最终 NSIS 包生成成功。
- `npm run dev -- --no-watch` 在 `15420` 完成真实开发启动；开发、release 及 NSIS 安装后均通过 CDP 检查正常与错误协议。静默安装退出码 0；测试卸载退出码 0，注册项为 0、安装目录内的应用可执行文件已移除，开发进程已关闭。
- 收尾调整类型生成器以去除尾部空白，重新生成 DTO，并通过生成一致性、对应 Clippy 和 rustfmt 检查；随后核对 Windows 图标资源与最终打包配置。
- 资源重生成验证发现初始 5 个图标中仅 macOS `icon.icns` 字节不稳定。当前只支持 Windows，因此移除该无用配置和文件；保留的 4 个 PNG／ICO 再生成后 SHA-256 完全一致。最终图标配置下再次执行 `npm run desktop:build`，NSIS 刷新成功；未重复不受影响的安装与 IPC 验证。
- M0-00 验收完成，记录移入 `_fin/`，下一任务登记为 M0-01。远端 CI 尚未执行，工程与记录仍未提交或推送。
