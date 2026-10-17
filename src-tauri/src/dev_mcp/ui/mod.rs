//! 开发界面适配：前端只上报观测，真实输入与截图委托受控 WebDriver 会话。
pub(crate) mod capture;
mod driver;
mod gesture;
pub(crate) mod model;
pub(super) mod observation;
mod targets;
#[cfg(test)]
mod tests;
pub(super) use driver::Driver;
pub(crate) use observation::Observations;

use super::config::Instance;
use layerlens_core::workspace_contract::{WorkspaceError, WorkspaceErrorCode};
use std::sync::Arc;

#[tauri::command]
pub(crate) fn dev_view_report<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    instance: tauri::State<'_, Arc<Instance>>,
    observation: model::UiObservation,
) -> Result<(), WorkspaceError> {
    if window.label() != "main" || (observation.render_lab.is_some() && !instance.render_lab) {
        return Err(crate::workspace::error(
            WorkspaceErrorCode::InvalidInput,
            "此窗口未启用开发观测。",
        ));
    }
    instance.observations.publish(&instance.id, observation)
}
