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
fn surface(value: &UiRenderSurface) -> bool {
    (1..=100_000).contains(&value.width)
        && (1..=100_000).contains(&value.height)
        && [value.css_width, value.css_height]
            .iter()
            .all(|v| v.is_finite() && *v > 0.0 && *v <= 100_000.0)
        && value.configured_dpr.is_finite()
        && (0.1..=16.0).contains(&value.configured_dpr)
}
impl UiCanvas {
    pub(crate) fn painted(&self, dpr: f64) -> bool {
        self.image_decoded
            && self.paint_prepared
            && self.image_error.is_none()
            && self.scene_revision == self.painted_revision
            && self.surface.as_ref().is_some_and(|s| {
                (s.configured_dpr - dpr).abs() < 0.000001
                    && (s.css_width - self.viewport.width).abs() <= 1.0
                    && (s.css_height - self.viewport.height).abs() <= 1.0
                    && (f64::from(s.width) - (s.css_width * dpr).floor()).abs() <= 1.0
                    && (f64::from(s.height) - (s.css_height * dpr).floor()).abs() <= 1.0
            })
    }
}
impl UiPreviewResources {
    fn counts(&self) -> [&UiResourceCount; 7] {
        [
            &self.renderers,
            &self.stages,
            &self.images,
            &self.object_urls,
            &self.reads,
            &self.decodes,
            &self.frames,
        ]
    }
    fn valid(&self) -> bool {
        self.canvas_elements <= 128
            && self.canvas_pixels <= 67_108_864
            && self.counts().iter().all(|c| {
                c.created <= 1_000_000_000
                    && c.released <= c.created
                    && c.live == c.created - c.released
            })
    }
    fn released(&self) -> bool {
        self.canvas_elements == 0
            && self.canvas_pixels == 0
            && self.counts().iter().all(|c| c.live == 0)
    }
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
            || value.render_lab.as_ref().is_some_and(|r| {
                r.engine != "konva"
                    || r.surface.as_ref().is_some_and(|s| !surface(s))
                    || !["idle", "loading", "ready", "error", "released"]
                        .contains(&r.phase.as_str())
                    || number(&r.generation).is_err()
                    || r.node_count > 65536
                    || r.resource_count > 131072
                    || !["shared", "native"].contains(&r.text_mode.as_str())
                    || r.native_text_layers > 2048
                    || r.shared_text_layers > 2048
                    || r.unsupported_layers > 2048
                    || !["bounded", "large", "largeLayers"].contains(&r.profile.as_str())
                    || r.source_layers > 2048
                    || r.encoded_bytes > 128 * 1024 * 1024
                    || r.alpha_bytes > 96 * 1024 * 1024
                    || [
                        &r.drawn_layer_ids,
                        &r.raster_layer_ids,
                        &r.text_raster_layer_ids,
                        &r.degraded_layer_ids,
                        &r.unsupported_layer_ids,
                    ]
                    .iter()
                    .any(|ids| {
                        ids.len() > 2048
                            || ids.iter().collect::<std::collections::HashSet<_>>().len()
                                != ids.len()
                    })
                    || r.samples.len() > 60
                    || r.samples.iter().any(|s| {
                        number(&s.generation).is_err()
                            || !["rebuild", "zoom", "reset", "pan", "resize", "fit", "focus"]
                                .contains(&s.operation.as_str())
                            || [
                                Some(s.submit_ms),
                                Some(s.ready_ms),
                                Some(s.readback_ms),
                                s.frame_interval_ms,
                                s.input_to_ready_ms,
                            ]
                            .into_iter()
                            .flatten()
                            .any(|n| !n.is_finite() || n < 0.0)
                    })
                    || r.candidate_ids.len() > 64
                    || r.limitations.len() > 64
                    || r.limitations.iter().any(|s| s.len() > 512)
                    || r.backend.len() > 64
                    || r.error.as_ref().is_some_and(|s| s.len() > 2048)
                    || [&r.source_sha256, &r.font_sha256]
                        .into_iter()
                        .flatten()
                        .any(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
                    || [
                        Some(r.init_ms),
                        Some(r.render_ms),
                        Some(r.scene_read_ms),
                        Some(r.assets_ms),
                        Some(r.first_ready_ms),
                        r.selection_ms,
                        r.hit_ms,
                    ]
                    .iter()
                    .flatten()
                    .any(|n| !n.is_finite() || *n < 0.0)
            })
            || !value.preview_resources.valid()
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
            || value
                .window
                .canvas_viewport
                .as_ref()
                .is_some_and(|r| !rect(r))
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
                    || c.surface.as_ref().is_some_and(|s| !surface(s))
                    || c.inspection.precision != "bounds"
                    || c.inspection.candidate_count > 100_000
                    || c.image_error.as_ref().is_some_and(|s| s.len() > 1024)
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
        if let Some(canvas) = &value.canvas
            && number(&canvas.painted_revision)? > number(&canvas.scene_revision)?
        {
            return Err(error(Code::InvalidInput, "绘制确认不能超出场景代次。"));
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
        if number(&value.window.display_revision)? == 0 {
            return Err(error(Code::InvalidInput, "显示环境代次必须为正数。"));
        }
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
            Self::CleanupFocus {
                plan_id,
                key_events_at_least,
            } => {
                number(plan_id)?;
                number(key_events_at_least)?;
                true
            }
            Self::CanvasInput {
                document_id,
                revision,
                key_events_at_least,
                pointer_events_at_least,
                ..
            } => {
                number(document_id)?;
                number(revision)?;
                number(key_events_at_least)?;
                number(pointer_events_at_least)?;
                true
            }
            Self::CanvasModifiers {
                key_events_at_least,
                ..
            } => {
                number(key_events_at_least)?;
                true
            }
            Self::CanvasPaint {
                document_id,
                revision,
                scene_at_least,
            } => {
                number(document_id)?;
                number(revision)?;
                number(scene_at_least)?;
                true
            }
            Self::Inspection {
                document_id,
                revision,
                ..
            } => {
                number(document_id)?;
                number(revision)?;
                true
            }
            Self::Renderer {
                engine,
                phase,
                generation_at_least,
            } => {
                number(generation_at_least)?;
                engine == "konva"
                    && ["idle", "loading", "ready", "error", "released"].contains(&phase.as_str())
            }
            Self::Applied { at_least }
            | Self::ViewRevision { at_least }
            | Self::PreviewReleased { at_least } => {
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
            Self::Display {
                width,
                height,
                device_pixel_ratio,
                after_revision,
            } => {
                if let Some(revision) = after_revision {
                    number(revision)?;
                }
                (1..=100_000).contains(width)
                    && (1..=100_000).contains(height)
                    && device_pixel_ratio.is_finite()
                    && (0.1..=16.0).contains(device_pixel_ratio)
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
            Self::PreviewReleased { at_least } => {
                value.render_lab.is_none()
                    && value.active_document.is_none()
                    && value.canvas.is_none()
                    && value
                        .core_sequence
                        .as_ref()
                        .is_some_and(|s| number(s).ok() >= number(at_least).ok())
                    && value.preview_resources.released()
            }
            Self::CanvasInput {
                document_id,
                revision,
                alt,
                ctrl,
                pointer_captured,
                key_events_at_least,
                pointer_events_at_least,
            } => {
                value
                    .active_document
                    .as_ref()
                    .is_some_and(|d| &d.id == document_id && &d.revision == revision)
                    && value.canvas.as_ref().is_some_and(|c| {
                        c.alt == *alt
                            && c.inspection.ctrl == *ctrl
                            && c.pointer_captured == *pointer_captured
                    })
                    && number(&value.input.key_events).ok() >= number(key_events_at_least).ok()
                    && number(&value.input.pointer_events).ok()
                        >= number(pointer_events_at_least).ok()
            }
            Self::CanvasModifiers {
                alt,
                ctrl,
                key_events_at_least,
            } => {
                value
                    .canvas
                    .as_ref()
                    .is_some_and(|c| c.alt == *alt && c.inspection.ctrl == *ctrl)
                    && number(&value.input.key_events).ok() >= number(key_events_at_least).ok()
            }
            Self::LayerFilter { active } => value
                .canvas
                .as_ref()
                .is_some_and(|c| c.inspection.filter_active == *active),
            Self::Display {
                width,
                height,
                device_pixel_ratio,
                after_revision,
            } => {
                value.window.width == *width
                    && value.window.height == *height
                    && (value.window.device_pixel_ratio - device_pixel_ratio).abs() < 0.000001
                    && after_revision.as_ref().is_none_or(|after| {
                        number(&value.window.display_revision).ok() > number(after).ok()
                    })
            }
            Self::CanvasPaint {
                document_id,
                revision,
                scene_at_least,
            } => {
                value
                    .active_document
                    .as_ref()
                    .is_some_and(|d| &d.id == document_id && &d.revision == revision)
                    && value.canvas.as_ref().is_some_and(|c| {
                        c.painted(value.window.device_pixel_ratio)
                            && number(&c.painted_revision).ok() >= number(scene_at_least).ok()
                    })
            }
            Self::Inspection {
                document_id,
                revision,
                selected_layer_id,
                auto_select,
                ctrl,
            } => {
                value
                    .active_document
                    .as_ref()
                    .is_some_and(|d| &d.id == document_id && &d.revision == revision)
                    && value.canvas.as_ref().is_some_and(|c| {
                        c.inspection.ready
                            && !c.inspection.pending
                            && c.inspection.selected_layer_id == *selected_layer_id
                            && c.inspection.details_layer_id == *selected_layer_id
                            && c.inspection.tree_selected_layer_id == *selected_layer_id
                            && c.inspection.auto_select == *auto_select
                            && c.inspection.ctrl == *ctrl
                    })
            }
            Self::RenderInteraction {
                selected_layer_id,
                auto_select,
                ctrl,
            } => value.render_lab.as_ref().is_some_and(|r| {
                r.selected_layer_id == *selected_layer_id
                    && r.auto_select == *auto_select
                    && r.ctrl == *ctrl
                    && !value.selection_running
            }),
            Self::Renderer {
                engine,
                phase,
                generation_at_least,
            } => value.render_lab.as_ref().is_some_and(|r| {
                &r.engine == engine
                    && &r.phase == phase
                    && number(&r.generation).ok() >= number(generation_at_least).ok()
            }),
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
                        .is_some_and(|c| c.painted(value.window.device_pixel_ratio))
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
            Self::CleanupFocus {
                plan_id,
                key_events_at_least,
            } => {
                value.cleanup.opened
                    && value.cleanup.phase.as_deref() == Some("ready")
                    && value.cleanup.plan_id.as_ref() == Some(plan_id)
                    && value.cleanup.focus_inside
                    && value.window.focused
                    && number(&value.input.key_events).ok() >= number(key_events_at_least).ok()
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
