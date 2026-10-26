//! 开发测试专用的只读窗口诊断。只分类当前进程自有窗口及其子窗口，
//! 不返回原始 HWND、外部进程身份或窗口文字，也不修改焦点／捕获状态。
//! Win32 FFI 集中在独立模块，桌面与核心继续禁止 unsafe。

use schemars::JsonSchema;
use serde::Serialize;

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows;

/// 一次系统查询中的窗口归属；Other 不携带外部窗口的身份。
#[derive(Debug, Clone, Copy, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WindowTarget {
    None,
    Main,
    MainChild,
    Auxiliary,
    AuxiliaryChild,
    Other,
}

/// 系统 GUI 线程状态；这些读数不能替代 WebView 的可信 focus／blur 事件。
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GuiThreadSnapshot {
    pub active: WindowTarget,
    pub keyboard_focus: WindowTarget,
    pub mouse_capture: WindowTarget,
    pub menu_owner: WindowTarget,
    pub in_menu_mode: bool,
    pub in_move_size: bool,
}

/// 线程桌面是否正在接收系统输入；不返回桌面名称或切换桌面。
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DesktopObservation {
    Available {
        calling_thread_receives_input: bool,
        window_thread_receives_input: bool,
    },
    Unavailable {
        operation: &'static str,
        error_code: Option<u32>,
    },
}

/// 顺序读取的诊断快照，并非跨线程原子快照。
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FocusSnapshot {
    pub desktop: DesktopObservation,
    pub foreground: WindowTarget,
    pub main_thread: GuiThreadSnapshot,
    pub auxiliary_thread: GuiThreadSnapshot,
    pub foreground_thread: Option<GuiThreadSnapshot>,
}

/// 查询失败单独报告，不能伪装为“窗口未聚焦”。
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum FocusObservation {
    Available {
        snapshot: FocusSnapshot,
    },
    Unavailable {
        operation: &'static str,
        #[serde(rename = "errorCode")]
        error_code: Option<u32>,
    },
}

/// 读取两个自有顶层窗口的实际焦点／捕获归属。
///
/// 参数是调用方已持有的 HWND 值；无效、空或非当前进程的窗口均返回
/// Unavailable。Windows 负责验证不透明句柄，此接口不解引用调用方地址。
/// 调用不会激活窗口、释放鼠标捕获或产生输入事件。
pub fn observe_focus(main: usize, auxiliary: usize) -> FocusObservation {
    #[cfg(windows)]
    {
        match windows::observe(main, auxiliary) {
            Ok(snapshot) => FocusObservation::Available { snapshot },
            Err((operation, error_code)) => FocusObservation::Unavailable {
                operation,
                error_code,
            },
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (main, auxiliary);
        FocusObservation::Unavailable {
            operation: "unsupportedPlatform",
            error_code: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_handles_are_unavailable_not_unfocused() {
        for handle in [0, usize::MAX] {
            let result = observe_focus(handle, handle);
            assert!(matches!(result, FocusObservation::Unavailable { .. }));
            let value = serde_json::to_value(result).unwrap();
            assert!(value.get("snapshot").is_none());
            assert!(value.get("errorCode").is_some());
        }
    }

    #[test]
    fn external_windows_have_only_an_anonymous_category() {
        let encoded = serde_json::to_value(WindowTarget::Other).unwrap();
        assert_eq!(encoded, "other");
    }
}
