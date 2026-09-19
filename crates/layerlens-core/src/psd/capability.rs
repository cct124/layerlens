//! 文档、图层与文字共用的支持程度；不依赖具体数据模型或解析器。

use serde::Serialize;
use ts_rs::TS;

/// 能力按对象分别报告，限制不能由空值或默认样式掩盖。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Support {
    Supported,
    Partial,
    Unsupported,
}

/// 描述本次解析或输出的支持程度及已知原因。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct Capability {
    pub status: Support,
    pub reason: String,
}

impl Capability {
    pub(super) fn partial(reason: &str) -> Self {
        Self {
            status: Support::Partial,
            reason: reason.into(),
        }
    }

    pub(super) fn supported(reason: &str) -> Self {
        Self {
            status: Support::Supported,
            reason: reason.into(),
        }
    }

    pub(super) fn unsupported(reason: &str) -> Self {
        Self {
            status: Support::Unsupported,
            reason: reason.into(),
        }
    }
}
