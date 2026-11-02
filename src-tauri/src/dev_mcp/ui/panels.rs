//! 面板观测边界与固定目标的真实输入；不接受任意元素、脚本或状态写入。
use super::model::*;
use crate::workspace::error;
use layerlens_core::workspace_contract::{WorkspaceError, WorkspaceErrorCode as Code};
use serde_json::{Value, json};

fn bounded(value: f64) -> bool {
    value.is_finite() && (0.0..=1_000_000.0).contains(&value)
}
fn rect(value: &UiRect) -> bool {
    [value.x, value.y, value.width, value.height]
        .into_iter()
        .all(bounded)
}
impl UiPanels {
    pub(super) fn valid(&self) -> bool {
        bounded(self.sidebar_width)
            && bounded(self.inspector_height)
            && bounded(self.resources_height)
            && self.inspector_percent.is_finite()
            && (0.0..=100.0).contains(&self.inspector_percent)
            && rect(&self.width_handle)
            && rect(&self.height_handle)
            && self
                .saved_sidebar_width
                .is_none_or(|v| v.is_finite() && (260.0..=560.0).contains(&v))
            && self
                .saved_inspector_percent
                .is_none_or(|v| v.is_finite() && v > 0.0 && v < 100.0)
            && self.tree_count <= 4096
            && self.search.len() <= 1024
            && self.task_name.len() <= 1024
            && [&self.tree, &self.properties]
                .into_iter()
                .flatten()
                .all(|s| {
                    rect(&s.rect)
                        && [s.scroll_top, s.scroll_height, s.client_height]
                            .into_iter()
                            .all(bounded)
                        && s.scroll_top <= (s.scroll_height - s.client_height).max(0.0) + 1.0
                        && s.rendered_rows <= 4096
                        && s.text_length <= 4 * 1024 * 1024
                })
    }
    pub(super) fn scroll(&self, target: PanelScrollTarget) -> Option<&UiPanelScroll> {
        match target {
            PanelScrollTarget::Layers => self.tree.as_ref(),
            PanelScrollTarget::Properties => self.properties.as_ref(),
        }
    }
}
fn ready(state: &UiObservation) -> Result<&UiPanels, WorkspaceError> {
    if state.cleanup.opened || !state.window.focused || !state.window.visible {
        return Err(error(
            Code::InvalidInput,
            "面板输入需要无模态且有焦点的可见窗口。",
        ));
    }
    state
        .panels
        .as_ref()
        .filter(|p| p.valid())
        .ok_or_else(|| error(Code::InvalidInput, "面板观测尚未就绪。"))
}
pub(super) fn drag_actions(
    state: &UiObservation,
    axis: PanelAxis,
    delta: i32,
    hold: bool,
) -> Result<Value, WorkspaceError> {
    let panels = ready(state)?;
    if delta == 0 || !(-2048..=2048).contains(&delta) {
        return Err(error(
            Code::InvalidInput,
            "面板拖动距离须为非零且不超过 2048 CSS 像素。",
        ));
    }
    let r = match axis {
        PanelAxis::Width => &panels.width_handle,
        PanelAxis::Height => &panels.height_handle,
    };
    let x = (r.x + r.width / 2.0).floor() as i32;
    let y = (r.y + r.height / 2.0).floor() as i32;
    let end_x = x + if matches!(axis, PanelAxis::Width) {
        delta
    } else {
        0
    };
    let end_y = y + if matches!(axis, PanelAxis::Height) {
        delta
    } else {
        0
    };
    if r.width <= 0.0
        || r.height <= 0.0
        || [x, end_x]
            .into_iter()
            .any(|v| v < 0 || v >= state.window.width as i32)
        || [y, end_y]
            .into_iter()
            .any(|v| v < 0 || v >= state.window.height as i32)
    {
        return Err(error(
            Code::InvalidInput,
            "面板拖动起终点必须在真实视口内。",
        ));
    }
    let mut actions = vec![
        json!({"type":"pointerMove","origin":"viewport","x":x,"y":y,"duration":0}),
        json!({"type":"pointerDown","button":0}),
        json!({"type":"pointerMove","origin":"viewport","x":end_x,"y":end_y,"duration":200}),
    ];
    if !hold {
        actions.push(json!({"type":"pointerUp","button":0}));
    }
    Ok(
        json!({"actions":[{"type":"pointer","id":"layerlens-pointer","parameters":{"pointerType":"mouse"},"actions":actions}]}),
    )
}
pub(super) fn scroll_actions(
    state: &UiObservation,
    target: PanelScrollTarget,
    edge: ScrollEdge,
) -> Result<Value, WorkspaceError> {
    let region = ready(state)?
        .scroll(target)
        .ok_or_else(|| error(Code::InvalidInput, "目标滚动面板不可见。"))?;
    let x = (region.rect.x + region.rect.width / 2.0).floor() as i32;
    let y = (region.rect.y + region.rect.height / 2.0).floor() as i32;
    if region.rect.width <= 0.0
        || region.rect.height <= 0.0
        || x < 0
        || y < 0
        || x >= state.window.width as i32
        || y >= state.window.height as i32
    {
        return Err(error(Code::InvalidInput, "滚动目标必须在真实视口内。"));
    }
    let delta = (region.scroll_height.ceil() as i32 + 1)
        * match edge {
            ScrollEdge::Start => -1,
            ScrollEdge::End => 1,
        };
    Ok(
        json!({"actions":[{"type":"wheel","id":"layerlens-wheel","actions":[{"type":"scroll","origin":"viewport","x":x,"y":y,"deltaX":0,"deltaY":delta,"duration":100}]}]}),
    )
}
