//! 隔离实例的原生窗口测试。只持有 main 与一个无 WebView 的辅助窗口；不接受外部窗口句柄。
//! 焦点由系统窗口 API 转移，窗口事件与 WebView 可信事件分别观测，禁止合成 blur。

use crate::workspace::error;
use layerlens_core::workspace_contract::{WorkspaceError, WorkspaceErrorCode as Code};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use tauri::{LogicalSize, WindowEvent};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) enum WindowSize {
    Minimum,
    Default,
    RenderLab,
}
impl WindowSize {
    fn dimensions(self) -> (u32, u32) {
        match self {
            Self::Minimum => (900, 600),
            Self::Default => (1280, 800),
            Self::RenderLab => (1280, 900),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(super) enum WindowAction {
    FocusMain {},
    FocusAuxiliary {},
    Minimize {},
    Maximize {},
    Restore {},
    Resize {
        size: WindowSize,
    },
    /// 正常 close 请求；实际退出由外部运行器持有进程句柄验证。
    Close {},
}

#[derive(Debug, Clone, Default)]
struct Events {
    sequence: u64,
    main_focused: Option<bool>,
    focused: u64,
    blurred: u64,
    resized: u64,
    moved: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct WindowState {
    /// 原生系统诊断独立于 Tauri／WebView 事件计数；不可用时保留原因。
    #[cfg(windows)]
    pub native_focus: layerlens_window_probe::FocusObservation,
    pub event_sequence: String,
    pub focus_events: String,
    pub blur_events: String,
    pub resize_events: String,
    pub move_events: String,
    pub focused: Option<bool>,
    pub auxiliary_focused: bool,
    pub visible: bool,
    pub minimized: bool,
    pub maximized: bool,
    /// 原生位置及客户区尺寸为物理像素；逻辑尺寸为物理尺寸除以当前 scaleFactor。
    pub x: i32,
    pub y: i32,
    pub physical_width: u32,
    pub physical_height: u32,
    pub logical_width: f64,
    pub logical_height: f64,
    pub scale_factor: f64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct WindowActionResult {
    pub before: WindowState,
    pub dispatched: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(super) enum WindowCondition {
    Focus { main: bool, auxiliary: bool },
    State { minimized: bool, maximized: bool },
    Size { size: WindowSize },
}
impl WindowCondition {
    fn matches(&self, state: &WindowState) -> bool {
        match self {
            Self::Focus { main, auxiliary } => {
                state.focused == Some(*main) && state.auxiliary_focused == *auxiliary
            }
            Self::State {
                minimized,
                maximized,
            } => state.minimized == *minimized && state.maximized == *maximized,
            Self::Size { size } => {
                let (width, height) = size.dimensions();
                !state.minimized
                    && !state.maximized
                    && (state.logical_width - f64::from(width)).abs() <= 1.0
                    && (state.logical_height - f64::from(height)).abs() <= 1.0
            }
        }
    }
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct WindowWait {
    pub condition: WindowCondition,
    pub after_sequence: Option<String>,
    pub timeout_ms: u32,
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct WindowWaitResult {
    pub satisfied: bool,
    pub timed_out: bool,
    pub last_state: WindowState,
}

pub(crate) struct Windows {
    main: tauri::WebviewWindow,
    auxiliary: tauri::Window,
    events: watch::Sender<Events>,
}
fn native(error: tauri::Error) -> WorkspaceError {
    crate::workspace::error(Code::Internal, format!("测试实例窗口操作失败：{error}"))
}
impl Windows {
    pub(crate) fn install(main: tauri::WebviewWindow) -> tauri::Result<Arc<Self>> {
        let auxiliary = tauri::window::WindowBuilder::new(&main, "dev-focus-auxiliary")
            .title("LayerLens · 测试焦点辅助窗口")
            .inner_size(240.0, 100.0)
            .visible(false)
            .focused(false)
            .skip_taskbar(true)
            .build()?;
        let events = watch::channel(Events::default()).0;
        let observed = events.clone();
        let close_auxiliary = auxiliary.clone();
        main.on_window_event(move |event| {
            observed.send_modify(|value| {
                value.sequence = value.sequence.saturating_add(1);
                match event {
                    WindowEvent::Focused(focused) => {
                        value.main_focused = Some(*focused);
                        if *focused {
                            value.focused += 1;
                        } else {
                            value.blurred += 1;
                        }
                    }
                    WindowEvent::Resized(_) => value.resized += 1,
                    WindowEvent::Moved(_) => value.moved += 1,
                    _ => {}
                }
            });
            // main 真正销毁后关闭辅助窗口，使最后窗口退出仍走原有 RunEvent 清理。
            if matches!(event, WindowEvent::Destroyed)
                && let Err(error) = close_auxiliary.close()
            {
                eprintln!("operation=dev_auxiliary_close error={error}");
            }
        });
        let observed = events.clone();
        auxiliary.on_window_event(move |_| {
            observed.send_modify(|value| value.sequence = value.sequence.saturating_add(1));
        });
        Ok(Arc::new(Self {
            main,
            auxiliary,
            events,
        }))
    }
    fn snapshot(&self) -> Result<WindowState, WorkspaceError> {
        let scale = self.main.scale_factor().map_err(native)?;
        let size = self.main.inner_size().map_err(native)?;
        let position = self.main.outer_position().map_err(native)?;
        let auxiliary_focused = self.auxiliary.is_focused().map_err(native)?;
        let visible = self.main.is_visible().map_err(native)?;
        let minimized = self.main.is_minimized().map_err(native)?;
        let maximized = self.main.is_maximized().map_err(native)?;
        let events = self.events.borrow().clone();
        Ok(WindowState {
            #[cfg(windows)]
            native_focus: layerlens_window_probe::observe_focus(
                self.main.hwnd().map_err(native)?.0 as usize,
                self.auxiliary.hwnd().map_err(native)?.0 as usize,
            ),
            event_sequence: events.sequence.to_string(),
            focus_events: events.focused.to_string(),
            blur_events: events.blurred.to_string(),
            resize_events: events.resized.to_string(),
            move_events: events.moved.to_string(),
            // Windows is_focused 查询顶层 HWND；键盘焦点转入 WebView2 子窗口时会返回 false。
            // Tauri 的原生 Focused 事件来自 WebView2 GotFocus/LostFocus，不能用该查询覆盖。
            focused: events.main_focused,
            auxiliary_focused,
            visible,
            minimized,
            maximized,
            x: position.x,
            y: position.y,
            physical_width: size.width,
            physical_height: size.height,
            logical_width: f64::from(size.width) / scale,
            logical_height: f64::from(size.height) / scale,
            scale_factor: scale,
        })
    }
    pub(super) async fn read(self: &Arc<Self>) -> Result<WindowState, WorkspaceError> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || this.snapshot())
            .await
            .map_err(|e| error(Code::Internal, format!("窗口观测任务失败：{e}")))?
    }
    pub(super) async fn action(
        self: &Arc<Self>,
        action: WindowAction,
    ) -> Result<WindowActionResult, WorkspaceError> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            let before = this.snapshot()?;
            match action {
                WindowAction::FocusMain {} => {
                    this.auxiliary.hide().map_err(native)?;
                    this.main.set_focus().map_err(native)?;
                    // 顶层 HWND 激活与 WebView2 子视图焦点是两层状态，二者都走原生 API。
                    let webview: &tauri::Webview = this.main.as_ref();
                    webview.set_focus().map_err(native)?;
                }
                WindowAction::FocusAuxiliary {} => {
                    this.auxiliary.show().map_err(native)?;
                    this.auxiliary.set_focus().map_err(native)?;
                }
                WindowAction::Minimize {} => this.main.minimize().map_err(native)?,
                WindowAction::Maximize {} => this.main.maximize().map_err(native)?,
                WindowAction::Restore {} => {
                    this.main.unminimize().map_err(native)?;
                    this.main.unmaximize().map_err(native)?;
                }
                WindowAction::Resize { size } => {
                    let (width, height) = size.dimensions();
                    this.main
                        .set_size(LogicalSize::new(width, height))
                        .map_err(native)?;
                }
                WindowAction::Close {} => this.main.close().map_err(native)?,
            }
            Ok(WindowActionResult {
                before,
                dispatched: true,
            })
        })
        .await
        .map_err(|e| error(Code::Internal, format!("窗口输入任务失败：{e}")))?
    }
    pub(super) async fn wait(
        self: &Arc<Self>,
        request: WindowWait,
        stopping: &CancellationToken,
    ) -> Result<WindowWaitResult, WorkspaceError> {
        if !(1..=30_000).contains(&request.timeout_ms) {
            return Err(error(Code::InvalidInput, "窗口等待须为 1–30000 ms。"));
        }
        let after = request
            .after_sequence
            .as_deref()
            .map(super::ui::observation::number)
            .transpose()?;
        let mut events = self.events.subscribe();
        let deadline =
            tokio::time::Instant::now() + Duration::from_millis(request.timeout_ms.into());
        loop {
            events.borrow_and_update();
            let state = self.read().await?;
            let advanced =
                after.is_none_or(|n| state.event_sequence.parse::<u64>().is_ok_and(|v| v > n));
            let satisfied = advanced && request.condition.matches(&state);
            let timed_out = tokio::time::Instant::now() >= deadline;
            if satisfied || timed_out {
                return Ok(WindowWaitResult {
                    satisfied,
                    timed_out: !satisfied,
                    last_state: state,
                });
            }
            // 原生最小化不保证有 resize 事件；有界重查系统状态，不能靠固定 sleep 宣称成功。
            tokio::select! {
                _ = stopping.cancelled() => return Err(error(Code::ShuttingDown, "实例退出，窗口等待终止。")),
                _ = tokio::time::sleep_until(deadline.min(tokio::time::Instant::now() + Duration::from_millis(50))) => {},
                changed = events.changed() => changed.map_err(|_| error(Code::ShuttingDown, "窗口观测已停止。"))?,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn native_contract_rejects_external_targets_scripts_and_unbounded_sizes() {
        for value in [
            json!({"kind":"focusMain","handle":123}),
            json!({"kind":"focusAuxiliary","label":"other"}),
            json!({"kind":"resize","width":10000,"height":10000}),
            json!({"kind":"resize","size":"screen"}),
            json!({"kind":"execute","script":"window.blur()"}),
        ] {
            assert!(serde_json::from_value::<WindowAction>(value).is_err());
        }
        assert!(
            serde_json::from_value::<WindowAction>(json!({"kind":"resize","size":"minimum"}))
                .is_ok()
        );
        assert!(serde_json::from_value::<WindowAction>(json!({"kind":"close"})).is_ok());
    }
}
