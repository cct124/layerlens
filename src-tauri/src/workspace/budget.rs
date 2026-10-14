//! 在分配完整 JSON 前计数；适配层在副作用前保留回执额度。
use super::error;
use layerlens_core::workspace_contract::{WorkspaceError, WorkspaceErrorCode};

pub(crate) fn check_size(
    value: &impl serde::Serialize,
    limit: usize,
) -> Result<(), WorkspaceError> {
    struct Counter {
        written: usize,
        limit: usize,
    }
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.written) {
                return Err(std::io::Error::other("响应或请求超过字节预算"));
            }
            self.written += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Counter { written: 0, limit }, value).map_err(|_| {
        error(
            WorkspaceErrorCode::ResourceLimit,
            "请求或完整响应超过预算。",
        )
    })
}
