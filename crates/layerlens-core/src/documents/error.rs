//! 文档服务错误；保留文件和解析失败的原因链，不依赖 IPC 错误类型。

use std::{error::Error, fmt, io, path::PathBuf};

use crate::psd::PsdError;

use super::{DocumentId, RevisionId};

/// 调用方可匹配的准入、生命周期与后台执行错误。
#[derive(Debug)]
pub enum DocumentError {
    InvalidConfiguration(&'static str),
    Path {
        path: PathBuf,
        source: io::Error,
    },
    QueueFull {
        limit: usize,
    },
    ResourceLimit {
        resource: &'static str,
        limit: u64,
    },
    NotFound(DocumentId),
    StaleRevision,
    InvalidQuery(&'static str),
    LayerNotFound(crate::psd::LayerId),
    ShuttingDown,
    IdExhausted,
    WorkerStart(io::Error),
    WorkerPanicked,
    Parse {
        path: PathBuf,
        source: PsdError,
    },
    Preview {
        document: DocumentId,
        revision: RevisionId,
        source: PsdError,
    },
}

impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration(message) => write!(f, "无效的文档服务配置：{message}"),
            Self::Path { path, source } => write!(f, "无法解析路径 {}：{source}", path.display()),
            Self::QueueFull { limit } => write!(f, "后台作业已达到上限 {limit}"),
            Self::ResourceLimit { resource, limit } => {
                write!(f, "全局 {resource} 额度不足（上限 {limit}）")
            }
            Self::NotFound(id) => write!(f, "文档 {} 不存在或已关闭", id.get()),
            Self::StaleRevision => f.write_str("文档修订已变化，请重新读取图层。"),
            Self::InvalidQuery(message) => write!(f, "无效图层查询：{message}"),
            Self::LayerNotFound(id) => write!(f, "图层 {} 不属于本次修订", id.0),
            Self::ShuttingDown => f.write_str("文档服务正在退出"),
            Self::IdExhausted => f.write_str("文档服务标识符已耗尽"),
            Self::WorkerStart(source) => write!(f, "无法启动文档后台线程：{source}"),
            Self::WorkerPanicked => f.write_str("文档后台计算异常终止"),
            Self::Parse { path, source } => write!(f, "无法打开 {}：{source}", path.display()),
            Self::Preview {
                document,
                revision,
                source,
            } => write!(
                f,
                "文档 {} 修订 {} 预览失败：{source}",
                document.get(),
                revision.get()
            ),
        }
    }
}

impl Error for DocumentError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Path { source, .. } | Self::WorkerStart(source) => Some(source),
            Self::Parse { source, .. } | Self::Preview { source, .. } => Some(source),
            _ => None,
        }
    }
}
