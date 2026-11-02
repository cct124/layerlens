//! 开发系统交互适配；样本路径复用实例根校验，坐标只从真实标题栏及匿名屏幕计算。
use super::*;
use crate::dev_mcp::{config::Instance, ui::model::UiRect};

impl Windows {
    pub(super) fn file_picker_state(&self) -> Result<FilePickerState, WorkspaceError> {
        #[cfg(windows)]
        {
            let state = layerlens_window_probe::system::file_picker(
                self.main.hwnd().map_err(native)?.0 as usize,
            )
            .map_err(native)?;
            Ok(FilePickerState {
                opened: state.opened,
                foreground: state.foreground,
            })
        }
        #[cfg(not(windows))]
        {
            Ok(FilePickerState {
                opened: false,
                foreground: false,
            })
        }
    }
    pub(super) fn system_action(
        &self,
        before: &WindowState,
        action: WindowAction,
        instance: &Instance,
        drag_region: Option<UiRect>,
    ) -> Result<(), WorkspaceError> {
        #[cfg(not(windows))]
        {
            let _ = (before, action, instance, drag_region);
            Err(error(
                Code::InvalidInput,
                "系统输入仅支持已验证的 Windows 平台。",
            ))
        }
        #[cfg(windows)]
        {
            use layerlens_window_probe::system as input;
            let handle = self.main.hwnd().map_err(native)?.0 as usize;
            match action {
                WindowAction::RestoreTaskbar {} => {
                    if !before.minimized || before.file_picker.opened {
                        return Err(error(
                            Code::InvalidInput,
                            "任务栏恢复仅用于已最小化的测试主窗口。",
                        ));
                    }
                    input::restore_taskbar(handle).map_err(native)
                }
                WindowAction::ResizeEdge { edge, direction } => {
                    if before.file_picker.opened
                        || before.minimized
                        || before.maximized
                        || before.foreground_ready != Some(FocusTarget::Main)
                        || !before.scale_factor.is_finite()
                        || before.scale_factor <= 0.0
                        || before.scale_factor > 8.0
                    {
                        return Err(error(
                            Code::InvalidInput,
                            "边缘调整只允许自有前台普通窗口。",
                        ));
                    }
                    let edge = match edge {
                        ResizeEdge::Left => input::ResizeEdge::Left,
                        ResizeEdge::Right => input::ResizeEdge::Right,
                        ResizeEdge::Top => input::ResizeEdge::Top,
                        ResizeEdge::Bottom => input::ResizeEdge::Bottom,
                        ResizeEdge::TopLeft => input::ResizeEdge::TopLeft,
                        ResizeEdge::TopRight => input::ResizeEdge::TopRight,
                        ResizeEdge::BottomLeft => input::ResizeEdge::BottomLeft,
                        ResizeEdge::BottomRight => input::ResizeEdge::BottomRight,
                    };
                    let sign = match direction {
                        ResizeDirection::Outward => 1,
                        ResizeDirection::Inward => -1,
                    };
                    input::resize(
                        handle,
                        edge,
                        (48.0 * before.scale_factor).round() as i32 * sign,
                    )
                    .map_err(native)
                }
                WindowAction::SystemShortcut { shortcut } => {
                    let key = match shortcut {
                        SystemShortcut::AltF4 => input::SystemShortcut::AltF4,
                        SystemShortcut::SnapLeft => input::SystemShortcut::SnapLeft,
                        SystemShortcut::SnapRight => input::SystemShortcut::SnapRight,
                    };
                    input::shortcut(handle, key).map_err(native)
                }
                WindowAction::ChooseSample { sample } => {
                    let path = instance.sample(&sample).map_err(native)?;
                    let path = path
                        .to_str()
                        .ok_or_else(|| error(Code::InvalidInput, "样本路径必须是 Unicode。"))?;
                    // canonicalize 的 Windows 扩展前缀不是选择器输入的一部分。
                    let path = path.strip_prefix(r"\\?\").unwrap_or(path);
                    input::choose_file(handle, path).map_err(native)
                }
                WindowAction::CancelFilePicker {} => {
                    input::cancel_file_picker(handle).map_err(native)
                }
                WindowAction::DragTitlebar { destination, .. } => {
                    let region = drag_region
                        .ok_or_else(|| error(Code::InvalidInput, "缺少真实标题栏拖动区观测。"))?;
                    let client = self.main.inner_position().map_err(native)?;
                    let (start, end) =
                        drag_points(before, &region, (client.x, client.y), &destination)?;
                    input::drag(
                        handle,
                        input::Point {
                            x: start.0,
                            y: start.1,
                        },
                        input::Point { x: end.0, y: end.1 },
                    )
                    .map_err(native)
                }
                _ => Err(error(Code::InvalidInput, "不是受限系统动作。")),
            }
        }
    }
}

type PhysicalPoint = (i32, i32);
fn drag_points(
    state: &WindowState,
    rect: &UiRect,
    client: (i32, i32),
    destination: &DragDestination,
) -> Result<(PhysicalPoint, PhysicalPoint), WorkspaceError> {
    if state.minimized
        || state.maximized
        || state.file_picker.opened
        || state.foreground_ready != Some(FocusTarget::Main)
        || ![rect.x, rect.y, rect.width, rect.height]
            .iter()
            .all(|v| v.is_finite())
        || rect.x < 0.0
        || rect.y < 0.0
        || rect.width < 8.0
        || rect.height < 8.0
        || rect.y + rect.height > 48.0
        || rect.x + rect.width > state.logical_width
    {
        return Err(error(
            Code::InvalidInput,
            "标题栏拖动须为自有前台普通窗口的真实可见拖动区。",
        ));
    }
    let x = f64::from(client.0) + (rect.x + rect.width / 2.0) * state.scale_factor;
    let y = f64::from(client.1) + (rect.y + rect.height / 2.0) * state.scale_factor;
    let start = (x.round() as i32, y.round() as i32);
    let area = state
        .work_area
        .as_ref()
        .ok_or_else(|| error(Code::InvalidInput, "屏幕工作区不可用。"))?;
    let end = match destination {
        DragDestination::Offset {} => {
            (x + 48.0 * state.scale_factor, y + 32.0 * state.scale_factor)
        }
        DragDestination::TopEdge {} => (x, f64::from(area.y) + 2.0),
        DragDestination::Monitor { index } => {
            let monitor = state
                .monitors
                .iter()
                .find(|m| m.index == *index && !m.current)
                .ok_or_else(|| error(Code::InvalidInput, "目标必须是当前拓扑中的另一显示器。"))?;
            let a = &monitor.work_area;
            (
                f64::from(a.x) + f64::from(a.physical_width) / 2.0,
                f64::from(a.y) + f64::from(a.physical_height) / 3.0,
            )
        }
    };
    if ![x, y, end.0, end.1]
        .iter()
        .all(|v| v.is_finite() && *v >= f64::from(i32::MIN) && *v <= f64::from(i32::MAX))
    {
        return Err(error(Code::InvalidInput, "标题栏坐标超出物理范围。"));
    }
    Ok((start, (end.0.round() as i32, end.1.round() as i32)))
}
