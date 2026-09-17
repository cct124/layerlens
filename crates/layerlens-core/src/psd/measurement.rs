//! 同步核心操作的墙钟阶段计时；不采集平台内存，也不改变解析与导出规则。

use serde::Serialize;
use std::time::Instant;

/// 打开文件到元数据可用的阶段耗时，单位毫秒；不包含 UI、报告序列化或像素解码。
///
/// total_ms 包含阶段之间的管理开销，不能与分项相加。源缓冲在返回前释放。
#[derive(Debug, Default, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenTimings {
    pub source_read_ms: f64,
    pub preflight_ms: f64,
    pub candidate_parse_ms: f64,
    pub normalization_ms: f64,
    pub source_hash_ms: f64,
    pub source_release_ms: f64,
    pub total_ms: f64,
}

/// 一次 PNG 请求的阶段耗时，单位毫秒；不含写盘，失败不返回不完整的成功计时。
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PngTimings {
    pub decode_ms: f64,
    pub encode_ms: f64,
}

pub(super) fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}
