//! 只读 Win32 绑定；所有输出缓冲区均由本模块初始化，句柄从不被解引用。
//! https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-getguithreadinfo

use super::{FocusSnapshot, GuiThreadSnapshot, WindowTarget};
use windows_sys::Win32::{
    Foundation::{GetLastError, HWND},
    UI::WindowsAndMessaging::{
        GUI_INMENUMODE, GUI_INMOVESIZE, GUI_POPUPMENUMODE, GUI_SYSTEMMENUMODE, GUITHREADINFO,
        GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, IsChild,
    },
};

type ProbeError = (&'static str, Option<u32>);

fn last_error(operation: &'static str) -> ProbeError {
    // SAFETY: GetLastError 仅读取当前线程状态，没有指针参数。
    (operation, Some(unsafe { GetLastError() }))
}

fn owned_thread(window: HWND) -> Result<u32, ProbeError> {
    if window.is_null() {
        return Err(("missingOwnedWindow", None));
    }
    let mut process = 0;
    // SAFETY: window 是由系统验证的不透明句柄；process 在调用期间有效且可写。
    let thread = unsafe { GetWindowThreadProcessId(window, &mut process) };
    if thread == 0 {
        return Err(last_error("ownedWindowThread"));
    }
    if process != std::process::id() {
        return Err(("foreignOwnedWindow", None));
    }
    Ok(thread)
}

struct OwnedWindows {
    main: HWND,
    auxiliary: HWND,
}

impl OwnedWindows {
    fn target(&self, window: HWND) -> WindowTarget {
        if window.is_null() {
            WindowTarget::None
        } else if window == self.main {
            WindowTarget::Main
        } else if window == self.auxiliary {
            WindowTarget::Auxiliary
        // SAFETY: IsChild 只查询由系统验证的句柄，不访问 Rust 内存或修改窗口。
        } else if unsafe { IsChild(self.main, window) } != 0 {
            WindowTarget::MainChild
        // SAFETY: 同上；窗口关闭竞态只使系统查询返回 false。
        } else if unsafe { IsChild(self.auxiliary, window) } != 0 {
            WindowTarget::AuxiliaryChild
        } else {
            WindowTarget::Other
        }
    }

    fn thread(&self, id: u32) -> Result<GuiThreadSnapshot, ProbeError> {
        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: info 完整初始化且 cbSize 精确匹配 Win32 结构；传入有效可写缓冲区。
        if unsafe { GetGUIThreadInfo(id, &mut info) } == 0 {
            return Err(last_error("guiThreadInfo"));
        }
        Ok(GuiThreadSnapshot {
            active: self.target(info.hwndActive),
            keyboard_focus: self.target(info.hwndFocus),
            mouse_capture: self.target(info.hwndCapture),
            menu_owner: self.target(info.hwndMenuOwner),
            in_menu_mode: info.flags & (GUI_INMENUMODE | GUI_POPUPMENUMODE | GUI_SYSTEMMENUMODE)
                != 0,
            in_move_size: info.flags & GUI_INMOVESIZE != 0,
        })
    }
}

pub(super) fn observe(main: usize, auxiliary: usize) -> Result<FocusSnapshot, ProbeError> {
    let windows = OwnedWindows {
        main: main as HWND,
        auxiliary: auxiliary as HWND,
    };
    let main_thread = owned_thread(windows.main)?;
    let auxiliary_thread = owned_thread(windows.auxiliary)?;
    // SAFETY: 无参数只读查询；返回的句柄只做分类，不访问其内存。
    let foreground = unsafe { GetForegroundWindow() };
    Ok(FocusSnapshot {
        foreground: windows.target(foreground),
        main_thread: windows.thread(main_thread)?,
        auxiliary_thread: windows.thread(auxiliary_thread)?,
        foreground_thread: if foreground.is_null() {
            None
        } else {
            // thread=0 是 Win32 文档声明的前台 GUI 线程查询，不返回外部线程标识。
            Some(windows.thread(0)?)
        },
    })
}
