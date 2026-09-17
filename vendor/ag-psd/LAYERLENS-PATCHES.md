# ag-psd 0.3.0 局部兼容补丁（patch 2）

来源为 crates.io 的 [`ag-psd 0.3.0` 发行归档](https://static.crates.io/crates/ag-psd/ag-psd-0.3.0.crate)，SHA-256：
`6470c6a2f96a00a8d4154504020b34807c2282438c98f90f5069ef3f8cb0e7bf`。
上游仓库为 <https://github.com/Vasyanator/ag-psd-rs>，对应提交
`ff8754fb5dfb3b91f4e28dd37df98f6404f1dbcc`。保留原始 `LICENSE` 和 README；
包名、版本及唯一直接依赖 `flate2` 均未改变。LayerLens 通过 Cargo 的本地
`patch.crates-io` 固定这份源码，后续升级需重新审查补丁和运行回归测试。
归档 SHA-256 已再次实算核对；来源提交取自发行包 `.cargo_vcs_info.json`。
仅复制 `Cargo.toml`、`LICENSE`、`README.md` 和 `src/`，没有复制上游或私有图片、PSD 样本。

## 补丁范围

- `psd.rs`、`reader.rs`：新增有归属、tag 和原始偏移的诊断。未知资源及附加块按声明边界
  跳过；已知渐变／图案缺口使用 `UnsupportedFeature`，只有显式
  `allow_unsupported_features` 才恢复。解析损坏、越界和资源错误不再被 catch-all 吞掉。
- `reader.rs`：section 内读取受父块边界约束；Unicode / ASCII 分配前核对实际字节；
  延迟合成保留 alpha、PSD/PSB、strict 和内存预算，图层解码继承同一预算和 strict。
  RGB8 原始通道直接复制到目标，RLE 的目标 RGBA、行表和解码行共用预算；strict 模式
  拒绝短行、过长展开和截断 PackBits。预算按每次 helper 调用独立计算，不是跨调用缓存预算。
- `reader.rs`、`writer.rs`：蒙版按 Adobe 规范先读写参数、再处理 real mask，兼容无 real mask
  时的两字节填充；对应上游往返测试及 LayerLens 独立手构蒙版样本共同覆盖。
- `descriptor.rs`、`engine_data.rs`：计数校验及嵌套深度上限，避免声明长度直接分配和深递归。
- `psd.rs`、`additional_info/text_keys.rs`：保留 TySh 原始文本 `raw_text`，不受原有 CR/LF
  归一化及 EngineData 合并影响。patch 2 另保留解析后的 `raw_engine_data`，供核心校验原文与区间，
  不使用候选裁剪、去重或补默认值的样式；缺失 Txt 字段返回 None，不伪装为空文本。
  reader 仅移除格式使用的尾部 NUL，不裁剪空白；strict 模式新增拒绝非法 UTF-16，非 strict 仍保留上游替换行为。
- `engine_data.rs`：patch 2 对 UTF-16BE 字符串和字节转义做有界读取，拒绝截断、缺少结束符及非法代理项；
  数字须完整解析为有限值，非法 token 返回不含原文的错误，不再打印字符并跳过。
- `additional_info/vector_keys.rs`：仅把明确未实现的渐变与图案解释改为 typed unsupported。
- `image_resources.rs`：原先静默跳过的 timeline information 显式返回 typed unsupported。
  恢复前仍完整解析其 descriptor；渐变／图案同样先验证完整 descriptor，再报告语义未实现。
- `layerlens_tests.rs`：公开、内存内构造的回归样本，不依赖私有 PSD。

## 修改文件和验证入口

相对原始发行包，修改文件为 `src/lib.rs`、`src/psd.rs`、`src/reader.rs`、`src/writer.rs`、
`src/descriptor.rs`、`src/engine_data.rs`、`src/image_resources.rs`、
`src/additional_info/text_keys.rs` 和 `src/additional_info/vector_keys.rs`。
新增本文件和 `src/layerlens_tests.rs`；其余发行源码逐文件哈希一致，未统一重排第三方格式。

从 LayerLens 根目录运行 `npm run test:parser`（`cargo test --locked -p ag-psd --lib`）；
该入口已纳入 `npm run check:rust` 和 CI 使用的 `npm run check`。
本轮 269 项通过，其中 12 项为补丁专项回归。上游部分测试在其外部样本缺失时直接返回，
此结果不等同于验证了上游完整样本库；LayerLens 自有公开样本在核心集成测试中另行验证。

patch 2 核心集成测试使用独立编码的 `text-engine-*` PSD，核对原文、矩阵、字体、字符／段落属性与来源、
不同 DPI、语义降级，以及直接修改 PSD 字节后的非法 TySh／EngineData UTF-16 和数字拒绝。
私有稿仅在本机做结构与读取验证，不作为公开 fixture 或 Photoshop 视觉保真证据。

这不是完整 PSD 安全审计，也未提供完整元数据总内存预算或颜色管理。LayerLens 的容器预检、
支持子集与资源预算仍是正式入口的必要前提。尤其 ZIP 和高位深路径仍沿用上游部分临时
分配逻辑，本补丁的预算验收限于正式支持的 RGB8 RAW/RLE 子集。
上游写入 API 保留但 LayerLens 不调用它。
