//! 独立测试实例的系统交互边界；不提供通用键盘、脚本或外部窗口控制。
//! 调用者串行执行动作；输入只在验证过的自有前台窗口派发。
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(windows)]
#[allow(unsafe_code)]
#[path = "system_windows.rs"]
mod implementation;

/// 固定的系统快捷键；不接受任意键序列。
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SystemShortcut {
    AltF4,
    SnapLeft,
    SnapRight,
}

/// 原生窗口边框的八个固定方向，不接受任意屏幕坐标。
#[derive(Debug, Clone, Copy)]
pub enum ResizeEdge {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}
impl ResizeEdge {
    pub(super) fn axes(self) -> (i32, i32) {
        match self {
            Self::Left => (-1, 0),
            Self::Right => (1, 0),
            Self::Top => (0, -1),
            Self::Bottom => (0, 1),
            Self::TopLeft => (-1, -1),
            Self::TopRight => (1, -1),
            Self::BottomLeft => (-1, 1),
            Self::BottomRight => (1, 1),
        }
    }
}

/// 物理桌面坐标，由适配层从自有窗口与受限目标计算。
#[derive(Debug, Clone, Copy)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// 原生选择器仅报告归属／焦点，不包含路径或系统窗口文字。
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FilePickerState {
    pub opened: bool,
    pub foreground: bool,
}

/// 明确保留操作与系统错误；无法验证输入前提时不派发。
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SystemError {
    pub operation: &'static str,
    pub code: Option<u32>,
}
impl std::fmt::Display for SystemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({:?})", self.operation, self.code)
    }
}
impl std::error::Error for SystemError {}
impl From<(&'static str, Option<u32>)> for SystemError {
    fn from((operation, code): (&'static str, Option<u32>)) -> Self {
        Self { operation, code }
    }
}

/// 在创建窗口前为开发实例设置唯一任务栏分组身份，避免与其他实例或固定项合并。
///
/// # Errors
/// UUID 格式无效、平台不支持或系统拒绝设置时返回错误。
pub fn set_taskbar_identity(instance: &str) -> Result<(), SystemError> {
    #[cfg(windows)]
    {
        implementation::set_taskbar_identity(instance)
    }
    #[cfg(not(windows))]
    {
        let _ = instance;
        Err(("taskbarUnsupported", None).into())
    }
}

/// 查询主窗口拥有的文件选择器；无效／非当前进程句柄返回错误。
pub fn file_picker(main: usize) -> Result<FilePickerState, SystemError> {
    #[cfg(windows)]
    {
        implementation::file_picker(main)
    }
    #[cfg(not(windows))]
    {
        let _ = main;
        Err(("unsupportedPlatform", None).into())
    }
}

/// 仅点击具有测试主窗口唯一标题的任务栏按钮；不接受外部目标或任意坐标。
pub fn restore_taskbar(main: usize) -> Result<(), SystemError> {
    #[cfg(windows)]
    {
        implementation::restore_taskbar(main)
    }
    #[cfg(not(windows))]
    {
        let _ = main;
        Err(("unsupportedPlatform", None).into())
    }
}

/// 派发固定快捷键并释放本次按键；只允许自有主窗口为系统输入目标。
pub fn shortcut(main: usize, key: SystemShortcut) -> Result<(), SystemError> {
    #[cfg(windows)]
    {
        implementation::shortcut(main, key)
    }
    #[cfg(not(windows))]
    {
        let _ = (main, key);
        Err(("unsupportedPlatform", None).into())
    }
}

/// 从自有标题栏点开始真实拖动，观察系统移动模式后移动并松开。
/// 调用者从真实 DOM 标题栏和屏幕工作区计算坐标，不能传入外部控件坐标。
pub fn drag(main: usize, start: Point, end: Point) -> Result<(), SystemError> {
    #[cfg(windows)]
    {
        implementation::drag(main, start, end)
    }
    #[cfg(not(windows))]
    {
        let _ = (main, start, end);
        Err(("unsupportedPlatform", None).into())
    }
}

/// 从经真实命中测试确认的自有窗口边框拖动；正距离向外、负距离向内，单位为物理像素。
/// 距离非零且绝对值不超过 512；派发后仍须等待实际几何和前端状态。
pub fn resize(main: usize, edge: ResizeEdge, distance: i32) -> Result<(), SystemError> {
    if distance == 0 || !(-512..=512).contains(&distance) {
        return Err(("invalidResizeDistance", None).into());
    }
    #[cfg(windows)]
    {
        implementation::resize(main, edge, distance)
    }
    #[cfg(not(windows))]
    {
        let _ = (main, edge);
        Err(("unsupportedPlatform", None).into())
    }
}

/// 用真实键盘输入选择已由调用方验证属于样本根的单个路径。
/// 不改写业务状态，也不调用文件打开服务替代系统选择器。
pub fn choose_file(main: usize, path: &str) -> Result<(), SystemError> {
    if path.is_empty()
        || path.encode_utf16().count() > 1024
        || path.contains(['\0', '\r', '\n', '"'])
    {
        return Err(("invalidSamplePath", None).into());
    }
    #[cfg(windows)]
    {
        implementation::choose_file(main, path)
    }
    #[cfg(not(windows))]
    {
        let _ = main;
        Err(("unsupportedPlatform", None).into())
    }
}

/// 向自有、前台文件选择器派发 Esc；不取消外部窗口。
pub fn cancel_file_picker(main: usize) -> Result<(), SystemError> {
    #[cfg(windows)]
    {
        implementation::cancel_file_picker(main)
    }
    #[cfg(not(windows))]
    {
        let _ = main;
        Err(("unsupportedPlatform", None).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_handles_never_receive_input() {
        for main in [0, usize::MAX] {
            assert!(file_picker(main).is_err());
            assert!(shortcut(main, SystemShortcut::AltF4).is_err());
            assert!(drag(main, Point { x: 0, y: 0 }, Point { x: 10, y: 10 }).is_err());
            assert!(choose_file(main, "sample.psd").is_err());
            assert!(cancel_file_picker(main).is_err());
            assert!(resize(main, ResizeEdge::BottomRight, 48).is_err());
        }
    }
    #[test]
    fn resizing_rejects_zero_and_unbounded_distance() {
        for distance in [i32::MIN, -513, 0, 513, i32::MAX] {
            assert_eq!(
                resize(0, ResizeEdge::Left, distance).unwrap_err().operation,
                "invalidResizeDistance"
            );
        }
    }
    #[test]
    fn file_input_rejects_control_characters_and_unbounded_text() {
        for path in ["", "x\ny.psd", "x\0.psd", "\"a.psd\" \"b.psd\""] {
            assert_eq!(
                choose_file(0, path).unwrap_err().operation,
                "invalidSamplePath"
            );
        }
        assert_eq!(
            choose_file(0, &"a".repeat(1025)).unwrap_err().operation,
            "invalidSamplePath"
        );
    }
}
