//! 最新前端投影与事件等待；只接收 main WebView 的报告，拒绝旧页面和乱序版本。
use super::model::*;
use crate::workspace::{check_size, error};
use layerlens_core::workspace_contract::{WorkspaceError, WorkspaceErrorCode as Code};
use std::sync::{Arc, Mutex};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

pub(crate) const OBSERVATION_BYTES: usize = 32 * 1024;
#[derive(Default)]
struct Version {
    epoch: u64,
    revision: u64,
    frontend: String,
    loading: bool,
}
pub(crate) struct Observations {
    version: Mutex<Version>,
    latest: watch::Sender<Option<Arc<UiObservation>>>,
}
impl Default for Observations {
    fn default() -> Self {
        Self {
            version: Mutex::new(Version::default()),
            latest: watch::channel(None).0,
        }
    }
}
pub(in crate::dev_mcp) fn number(value: &str) -> Result<u64, WorkspaceError> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| error(Code::InvalidInput, "观测版本必须是十进制整数字符串。"))?;
    if parsed.to_string() != value {
        return Err(error(Code::InvalidInput, "观测版本不是规范十进制整数。"));
    }
    Ok(parsed)
}
fn rect(value: &UiRect) -> bool {
    [value.x, value.y, value.width, value.height]
        .iter()
        .all(|n| n.is_finite() && n.abs() <= 10_000_000.0)
        && value.width >= 0.0
        && value.height >= 0.0
}
impl Observations {
    pub(crate) fn subscribe(&self) -> tokio::sync::watch::Receiver<Option<Arc<UiObservation>>> {
        self.latest.subscribe()
    }
    pub(crate) fn invalidate(&self) {
        let mut version = self.version.lock().unwrap_or_else(|p| p.into_inner());
        version.loading = true;
        self.latest.send_replace(None);
    }
    pub(crate) fn publish(
        &self,
        instance: &str,
        value: UiObservation,
    ) -> Result<(), WorkspaceError> {
        check_size(&value, OBSERVATION_BYTES)?;
        if value.instance_id != instance {
            return Err(error(Code::ForeignSession, "前端报告属于其他实例。"));
        }
        if uuid::Uuid::parse_str(&value.frontend_id).is_err()
            || value.tasks.len() > 128
            || value.cleanup.task_ids.len() > 128
            || value.cleanup.rendered_tasks > 128
            || !["pan", "region"].contains(&value.tool.as_str())
            || !["layers", "tasks"].contains(&value.panel.as_str())
            || value
                .menu
                .as_deref()
                .is_some_and(|s| !["file", "view", "window"].contains(&s))
            || value.focus.as_ref().is_some_and(|s| s.len() > 128)
            || !value.window.device_pixel_ratio.is_finite()
            || !(0.1..=16.0).contains(&value.window.device_pixel_ratio)
            || value.window.width > 100_000
            || value.window.height > 100_000
            || value.window.titlebar.as_ref().is_some_and(|r| !rect(r))
            || value.window.controls.len() > 3
            || value.window.controls.iter().any(|c| {
                !rect(&c.rect)
                    || c.label.len() > 128
                    || !["window:minimize", "window:maximize", "window:close"]
                        .contains(&c.target.as_str())
            })
            || value.region.as_ref().is_some_and(|r| !rect(r))
            || value.canvas.as_ref().is_some_and(|c| {
                !rect(&c.viewport)
                    || c.image.as_ref().is_some_and(|r| !rect(r))
                    || c.draft.as_ref().is_some_and(|r| !rect(r))
                    || !c.zoom.is_finite()
                    || c.zoom <= 0.0
                    || c.snap_guides > 2
                    || [c.pan_x, c.pan_y]
                        .iter()
                        .any(|n| !n.is_finite() || n.abs() > 10_000_000.0)
                    || [&c.snap_x, &c.snap_y].into_iter().flatten().any(|s| {
                        !s.position.is_finite() || s.position < 0.0 || s.position > 10_000_000.0
                    })
                    || c.snap_guides
                        != u32::from(c.snap_x.is_some()) + u32::from(c.snap_y.is_some())
            })
        {
            return Err(error(Code::InvalidInput, "前端报告超出有效范围。"));
        }
        let epoch = number(&value.document_epoch)?;
        let revision = number(&value.view_revision)?;
        if epoch == 0 || revision == 0 {
            return Err(error(Code::InvalidInput, "前端代次及版本必须为正数。"));
        }
        for version in [
            &value.core_sequence,
            &value.selection_revision,
            &value.snapshot_id,
        ]
        .into_iter()
        .flatten()
        {
            number(version)?;
        }
        number(&value.input.pointer_events)?;
        number(&value.input.key_events)?;
        number(&value.window.focus_events)?;
        number(&value.window.blur_events)?;
        let mut current = self.version.lock().unwrap_or_else(|p| p.into_inner());
        if epoch < current.epoch
            || (epoch == current.epoch
                && (current.loading
                    || current.frontend != value.frontend_id
                    || revision <= current.revision))
        {
            return Err(error(Code::StaleRevision, "已丢弃旧页面或乱序观测。"));
        }
        *current = Version {
            epoch,
            revision,
            frontend: value.frontend_id.clone(),
            loading: false,
        };
        self.latest.send_replace(Some(Arc::new(value)));
        Ok(())
    }
    pub(crate) fn read(&self) -> Result<UiObservation, WorkspaceError> {
        self.latest
            .borrow()
            .as_deref()
            .cloned()
            .ok_or_else(|| error(Code::NotFound, "前端未连接或正在导航；尚无有效观测。"))
    }
    pub(crate) async fn wait(
        &self,
        request: UiWaitRequest,
        stopping: &CancellationToken,
    ) -> Result<UiWaitResult, WorkspaceError> {
        if !(1..=30_000).contains(&request.timeout_ms) {
            return Err(error(Code::InvalidInput, "界面等待须为 1–30000 ms。"));
        }
        request.condition.validate()?;
        let mut receiver = self.latest.subscribe();
        let deadline = tokio::time::Instant::now()
            + std::time::Duration::from_millis(request.timeout_ms.into());
        loop {
            let last = receiver.borrow_and_update().as_deref().cloned();
            if let Some(expected) = &request.frontend_id {
                match &last {
                    Some(value) if &value.frontend_id != expected => {
                        return Err(error(Code::StaleRevision, "前端代次已更换，请重新观测。"));
                    }
                    None => return Err(error(Code::NotFound, "前端连接已失效，等待终止。")),
                    _ => {}
                }
            }
            if last
                .as_ref()
                .is_some_and(|state| request.condition.matches(state))
            {
                return Ok(UiWaitResult {
                    satisfied: true,
                    timed_out: false,
                    condition: request.condition,
                    last_state: last,
                });
            }
            tokio::select! {
                _ = stopping.cancelled() => return Err(error(Code::ShuttingDown, "实例退出，界面等待已终止。")),
                _ = tokio::time::sleep_until(deadline) => return Ok(UiWaitResult { satisfied:false, timed_out:true, condition:request.condition, last_state:last }),
                changed = receiver.changed() => changed.map_err(|_| error(Code::ShuttingDown, "前端观测通道已关闭。"))?,
            }
        }
    }
}
impl UiCondition {
    fn validate(&self) -> Result<(), WorkspaceError> {
        let valid = match self {
            Self::Applied { at_least } | Self::ViewRevision { at_least } => {
                number(at_least)?;
                true
            }
            Self::ImageReady {
                document_id,
                revision,
            } => {
                number(document_id)?;
                number(revision)?;
                true
            }
            Self::Panel { panel } => ["tasks", "layers"].contains(&panel.as_str()),
            Self::Tool {
                tool,
                key_events_at_least,
            } => {
                number(key_events_at_least)?;
                ["pan", "region"].contains(&tool.as_str())
            }
            Self::Menu { menu } => menu
                .as_deref()
                .is_none_or(|s| ["file", "view", "window"].contains(&s)),
            Self::Cleanup { phase } => phase.as_deref().is_none_or(|s| {
                [
                    "preparing",
                    "failed",
                    "ready",
                    "confirming",
                    "cancelling",
                    "stale",
                    "uncertain",
                    "complete",
                ]
                .contains(&s)
            }),
            Self::Task { task_id, status } => {
                number(task_id)?;
                ["active", "released", "invalidated"].contains(&status.as_str())
            }
            Self::Content { snapshot_id } => {
                number(snapshot_id)?;
                true
            }
            Self::CanvasIdle {
                pointer_events_at_least,
            } => {
                number(pointer_events_at_least)?;
                true
            }
            Self::Selection { after, .. } => {
                number(after)?;
                true
            }
            Self::TaskCount { count } => *count <= 128,
            Self::WindowFocus { events_after, .. } => {
                if let Some(n) = events_after {
                    number(n)?;
                }
                true
            }
            Self::WindowLayout { width, height, .. } => {
                (1..=100_000).contains(width) && (1..=100_000).contains(height)
            }
            _ => true,
        };
        if valid {
            Ok(())
        } else {
            Err(error(Code::InvalidInput, "界面等待条件无效。"))
        }
    }
    fn matches(&self, value: &UiObservation) -> bool {
        match self {
            Self::WindowFocus {
                focused,
                events_after,
            } => {
                value.window.focused == *focused
                    && events_after.as_ref().is_none_or(|after| {
                        let count = if *focused {
                            &value.window.focus_events
                        } else {
                            &value.window.blur_events
                        };
                        number(count).ok() > number(after).ok()
                    })
            }
            Self::WindowLayout {
                width,
                height,
                maximized,
            } => {
                value.window.width.abs_diff(*width) <= 1
                    && value.window.height.abs_diff(*height) <= 1
                    && value.window.controls.iter().any(|c| {
                        c.target == "window:maximize"
                            && !c.disabled
                            && c.label
                                == if *maximized {
                                    "还原窗口"
                                } else {
                                    "最大化窗口"
                                }
                    })
            }
            Self::Ready {} => value.core_sequence.is_some(),
            Self::Applied { at_least } => value
                .core_sequence
                .as_ref()
                .is_some_and(|n| number(n).ok() >= number(at_least).ok()),
            Self::ImageReady {
                document_id,
                revision,
            } => {
                value
                    .active_document
                    .as_ref()
                    .is_some_and(|d| &d.id == document_id && &d.revision == revision)
                    && value
                        .canvas
                        .as_ref()
                        .is_some_and(|c| c.image_decoded && c.paint_prepared)
            }
            Self::Panel { panel } => &value.panel == panel,
            Self::Tool {
                tool,
                key_events_at_least,
            } => {
                &value.tool == tool
                    && number(&value.input.key_events).ok() >= number(key_events_at_least).ok()
            }
            Self::Menu { menu } => &value.menu == menu,
            Self::Draft { active } => {
                value.canvas.as_ref().is_some_and(|c| c.draft.is_some()) == *active
            }
            Self::CanvasIdle {
                pointer_events_at_least,
            } => {
                number(&value.input.pointer_events).ok() >= number(pointer_events_at_least).ok()
                    && value
                        .canvas
                        .as_ref()
                        .is_some_and(|c| !c.panning && !c.pointer_captured && c.draft.is_none())
            }
            Self::Cleanup { phase } => {
                &value.cleanup.phase == phase && value.cleanup.opened == phase.is_some()
            }
            Self::Task { task_id, status } => value
                .tasks
                .iter()
                .any(|t| &t.id == task_id && &t.status == status),
            Self::TaskCount { count } => value.tasks.len() == *count as usize,
            Self::CleanupSync { failed } => {
                value.cleanup.phase.as_deref() == Some("complete")
                    && !value.cleanup.syncing
                    && value.cleanup.sync_error == *failed
            }
            Self::SelectionIdle {} => !value.selection_running,
            Self::CleanupListEnd {} => {
                value.cleanup.opened && value.cleanup.scrollable && value.cleanup.scrolled_to_end
            }
            Self::Selection { after, has_scope } => {
                value
                    .selection_revision
                    .as_ref()
                    .is_some_and(|n| number(n).ok() > number(after).ok())
                    && value.snapshot_id.is_some() == *has_scope
            }
            Self::Content { snapshot_id } => value
                .content
                .as_ref()
                .is_some_and(|c| &c.snapshot_id == snapshot_id && c.ready),
            Self::ViewRevision { at_least } => {
                number(&value.view_revision).ok() >= number(at_least).ok()
            }
        }
    }
}
