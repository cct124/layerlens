//! 解析实验的领域错误；原始 I/O、候选库和 PNG 错误通过 cause 保留。

use std::{error::Error, fmt};

/// 调用方可区分输入、能力、预算及输出失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PsdErrorCode {
    Io,
    SourceChanged,
    InvalidInput,
    Unsupported,
    ResourceLimit,
    ParseFailed,
    PreviewUnavailable,
    LayerNotFound,
    ExportUnsupported,
    EncodeFailed,
}

/// 原型失败结果；不修改输入文件，错误消息带操作上下文。
#[derive(Debug)]
pub struct PsdError {
    pub code: PsdErrorCode,
    message: String,
    cause: Option<Box<dyn Error + Send + Sync>>,
}

impl PsdError {
    pub(super) fn new(code: PsdErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            cause: None,
        }
    }

    pub(super) fn with_cause(
        code: PsdErrorCode,
        message: impl Into<String>,
        cause: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            cause: Some(Box::new(cause)),
        }
    }
}

impl fmt::Display for PsdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for PsdError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause
            .as_ref()
            .map(|cause| cause.as_ref() as &(dyn Error + 'static))
    }
}
