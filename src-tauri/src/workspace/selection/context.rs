//! 开发协议的选区附带内容。摘要与内容固定到同一快照，读取失败不回退到另一个选区。

use super::{dto, selection_error};
use layerlens_core::{
    documents::selection::*, selection_contract::*, workspace_contract::WorkspaceError,
};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectionContext {
    #[serde(flatten)]
    pub summary: SelectionSummaryDto,
    pub content: Option<SelectionContentDto>,
    pub content_error: Option<WorkspaceError>,
}

pub(crate) fn read_context(
    handle: &SelectionHandle,
    fixed: Option<SelectionSummaryDto>,
) -> Result<SelectionContext, WorkspaceError> {
    let current = handle.user_selection().map_err(selection_error)?;
    let (summary, snapshot) = if let Some(summary) = fixed {
        // 工作区结果已包含原子摘要；只按该摘要续读，不拼接新的当前选区。
        let snapshot = summary
            .scope
            .as_ref()
            .map(|scope| {
                let session = summary.session_id.parse()?;
                handle.selection_snapshot(&session, scope.snapshot_id.parse()?)
            })
            .transpose();
        (summary, snapshot)
    } else {
        let summary = dto::summary(SelectionSummary {
            session_id: current.session_id,
            revision: current.revision,
            document_id: current.document_id,
            document_revision: current.document_revision,
            snapshot: current.snapshot.as_ref().map(SnapshotLease::brief),
            items: current
                .snapshot
                .as_ref()
                .map_or_else(Vec::new, |s| s.scope().items.clone()),
        });
        (summary, Ok(current.snapshot))
    };
    let mut context = SelectionContext {
        summary,
        content: None,
        content_error: None,
    };
    if context.summary.region.is_some() {
        match snapshot.and_then(|snapshot| {
            snapshot
                .ok_or(SelectionError::SnapshotExpired)?
                .content_page_with_budget(None, 32, 64 * 1024)
        }) {
            Ok(page) => context.content = Some(dto::content(page)),
            Err(error) => context.content_error = Some(selection_error(error)),
        }
    }
    super::super::budget::check_size(&context, 128 * 1024)?;
    Ok(context)
}
