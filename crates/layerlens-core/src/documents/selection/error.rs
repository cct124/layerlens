//! 选区、快照和任务错误；生命周期失败不退回当前文档或当前选择。

use std::{error::Error, fmt};

use crate::documents::DocumentError;

#[derive(Debug)]
pub enum SelectionError {
    Document(DocumentError),
    InvalidInput(&'static str),
    ForeignSession,
    StaleSelection,
    InactiveDocument,
    SnapshotExpired,
    EmptyTargets,
    TaskNotFound,
    TaskReleased,
    TaskInvalidated,
    RequestConflict,
}

impl From<DocumentError> for SelectionError {
    fn from(error: DocumentError) -> Self {
        Self::Document(error)
    }
}

impl fmt::Display for SelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Document(error) => error.fmt(f),
            Self::InvalidInput(message) => write!(f, "无效选区或任务请求：{message}"),
            Self::ForeignSession => f.write_str("引用属于另一核心会话"),
            Self::StaleSelection => f.write_str("选区版本已变化，请重新采样"),
            Self::InactiveDocument => f.write_str("只能提交活动文档的用户选区"),
            Self::SnapshotExpired => f.write_str("选区快照已过期，请重新获取选择"),
            Self::EmptyTargets => f.write_str("选区没有有效可见目标，不能创建任务"),
            Self::TaskNotFound => f.write_str("设计任务不存在"),
            Self::TaskReleased => f.write_str("设计任务已释放，不接受新的读取"),
            Self::TaskInvalidated => f.write_str("设计任务的源文档已被主动清理，任务已失效"),
            Self::RequestConflict => f.write_str("请求 ID 已用于不同的任务参数"),
        }
    }
}

impl Error for SelectionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Document(error) => Some(error),
            _ => None,
        }
    }
}
