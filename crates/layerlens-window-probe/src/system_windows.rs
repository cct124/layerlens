//! 自有窗口的 Win32 输入与 UI Automation；所有 FFI 指针在本模块内验证。
//! SendInput 不保证送达，调用者必须继续等待实际窗口／业务状态。
use super::{FilePickerState, Point, ResizeEdge, SystemError, SystemShortcut};
use crate::windows::{desktop_receives_input, owned_thread};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{GetLastError, HWND, POINT, RECT},
    System::Threading::GetCurrentThreadId,
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};
#[path = "taskbar_windows.rs"]
mod taskbar;
pub(super) fn restore_taskbar(main: usize) -> Result<(), SystemError> {
    taskbar::restore(main)
}

fn error(operation: &'static str) -> SystemError {
    // SAFETY: 仅取得当前线程错误码，无指针参数。
    SystemError {
        operation,
        code: Some(unsafe { GetLastError() }),
    }
}

pub(super) fn set_taskbar_identity(instance: &str) -> Result<(), SystemError> {
    taskbar::set_identity(instance)
}
fn check_input(window: HWND) -> Result<(), SystemError> {
    let thread = owned_thread(window)?;
    // SAFETY: 当前线程查询无参数；窗口句柄已验证属于当前进程。
    if !desktop_receives_input(thread)? || !desktop_receives_input(unsafe { GetCurrentThreadId() })?
    {
        return Err(("inputDesktopUnavailable", None).into());
    }
    // SAFETY: 系统验证句柄；仅读取前台归属、可见性及输入状态。
    if unsafe { GetForegroundWindow() } != window || unsafe { IsWindowVisible(window) } == 0 {
        return Err(("ownedWindowNotForeground", None).into());
    }
    let mut state = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: state 已初始化，大小精确，调用期间缓冲区有效。
    if unsafe { GetGUIThreadInfo(thread, &mut state) } == 0 {
        return Err(error("inputGuiThread"));
    }
    // SAFETY: 只判断系统句柄的父子关系，不解引用句柄。
    if state.hwndFocus != window && unsafe { IsChild(window, state.hwndFocus) } == 0 {
        return Err(("ownedKeyboardFocusMissing", None).into());
    }
    if !state.hwndCapture.is_null()
        || state.flags & (GUI_INMOVESIZE | GUI_INMENUMODE | GUI_POPUPMENUMODE | GUI_SYSTEMMENUMODE)
            != 0
    {
        return Err(("systemInteractionBusy", None).into());
    }
    for key in [
        VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN, VK_LBUTTON, VK_RBUTTON, VK_MBUTTON,
    ] {
        // SAFETY: 合法虚拟键常量；高位才表示当前按下，不使用历史低位。
        if unsafe { GetAsyncKeyState(i32::from(key)) } < 0 {
            return Err(("physicalInputAlreadyHeld", None).into());
        }
    }
    Ok(())
}
fn keyboard(key: u16, scan: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}
fn mouse(point: Point, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: point.x,
                dy: point.y,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}
fn send(inputs: &[INPUT]) -> Result<(), SystemError> {
    // SAFETY: inputs 是完整初始化的 INPUT 数组，数量有界且结构大小精确。
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
    if sent as usize != inputs.len() {
        Err(error("sendInputIncomplete"))
    } else {
        Ok(())
    }
}
fn key_pair(key: u16) -> [INPUT; 2] {
    [keyboard(key, 0, 0), keyboard(key, 0, KEYEVENTF_KEYUP)]
}
fn chord(window: HWND, modifier: u16, key: u16) -> Result<(), SystemError> {
    check_input(window)?;
    let result = send(&[
        keyboard(modifier, 0, 0),
        keyboard(key, 0, 0),
        keyboard(key, 0, KEYEVENTF_KEYUP),
        keyboard(modifier, 0, KEYEVENTF_KEYUP),
    ]);
    if result.is_err() {
        release(&[
            keyboard(key, 0, KEYEVENTF_KEYUP),
            keyboard(modifier, 0, KEYEVENTF_KEYUP),
        ])?;
    }
    result
}
fn release(inputs: &[INPUT]) -> Result<(), SystemError> {
    // 仅释放本次动作按下的键，不激活窗口、取消外部任务或发送新按下事件。
    send(inputs).map_err(|_| ("inputReleaseFailed", None).into())
}
pub(super) fn shortcut(main: usize, key: SystemShortcut) -> Result<(), SystemError> {
    let (modifier, key) = match key {
        SystemShortcut::AltF4 => (VK_MENU, VK_F4),
        SystemShortcut::SnapLeft => (VK_LWIN, VK_LEFT),
        SystemShortcut::SnapRight => (VK_LWIN, VK_RIGHT),
    };
    chord(main as HWND, modifier, key)
}
fn dialog(main: usize) -> Result<Option<HWND>, SystemError> {
    let main = main as HWND;
    owned_thread(main)?;
    // SAFETY: main 是已校验自有顶层窗口，查询最后活动的自有弹窗。
    let popup = unsafe { GetLastActivePopup(main) };
    if popup.is_null() || popup == main {
        return Ok(None);
    }
    owned_thread(popup)?;
    // SAFETY: 仅读取 popup 的所有者与可见性，拒绝非自有父窗口的窗口。
    if unsafe { GetWindow(popup, GW_OWNER) } != main || unsafe { IsWindowVisible(popup) } == 0 {
        return Ok(None);
    }
    let mut class = [0u16; 32];
    // SAFETY: class 有效可写且长度与传参相同；不读取窗口标题或路径。
    let len = unsafe { GetClassNameW(popup, class.as_mut_ptr(), class.len() as i32) };
    if len <= 0 {
        return Err(error("dialogClass"));
    }
    if class[..len as usize] != [35, 51, 50, 55, 55, 48] {
        return Ok(None);
    }
    Ok(Some(popup))
}
pub(super) fn file_picker(main: usize) -> Result<FilePickerState, SystemError> {
    let popup = dialog(main)?;
    // SAFETY: 只查询当前系统前台；不返回其身份。
    Ok(FilePickerState {
        opened: popup.is_some(),
        foreground: popup.is_some_and(|w| unsafe { GetForegroundWindow() } == w),
    })
}
pub(super) fn cancel_file_picker(main: usize) -> Result<(), SystemError> {
    let popup = dialog(main)?.ok_or(SystemError::from(("filePickerNotOpen", None)))?;
    check_input(popup)?;
    let result = send(&key_pair(VK_ESCAPE));
    if result.is_err() {
        release(&[keyboard(VK_ESCAPE, 0, KEYEVENTF_KEYUP)])?;
    }
    result
}
fn absolute(point: Point) -> Result<Point, SystemError> {
    // SAFETY: 四个系统常量仅查询虚拟桌面几何。
    let (x, y, w, h) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    Ok(Point {
        x: absolute_axis(point.x, x, w)?,
        y: absolute_axis(point.y, y, h)?,
    })
}

fn absolute_axis(coordinate: i32, origin: i32, extent: i32) -> Result<i32, SystemError> {
    let offset = i64::from(coordinate) - i64::from(origin);
    if !(2..=65536).contains(&extent) || offset < 0 || offset >= i64::from(extent) {
        return Err(("pointOutsideVirtualDesktop", None).into());
    }
    // SendInput 用 65536 个区间映射桌面；定位像素中心，避免落在一像素边框之外。
    Ok(((2 * offset + 1) * 65536 / (2 * i64::from(extent))) as i32)
}
static LAST_DRAG_RELEASE: Mutex<Option<Instant>> = Mutex::new(None);

fn independent_click_delay(
    elapsed: Duration,
    double_click_ms: u32,
) -> Result<Duration, SystemError> {
    // GetDoubleClickTime 的公开上限为 5000ms；不修改系统设置或放宽输入等待预算。
    if double_click_ms > 5000 {
        return Err(("invalidDoubleClickTime", None).into());
    }
    Ok(Duration::from_millis(u64::from(double_click_ms) + 1).saturating_sub(elapsed))
}

pub(super) fn drag(main: usize, start: Point, end: Point) -> Result<(), SystemError> {
    drag_to(main, start, end, None)
}

fn drag_to(
    main: usize,
    start: Point,
    end: Point,
    expected_border: Option<u32>,
) -> Result<(), SystemError> {
    let main = main as HWND;
    check_input(main)?;
    let last_release = *LAST_DRAG_RELEASE
        .lock()
        .map_err(|_| SystemError::from(("dragTimingUnavailable", None)))?;
    if let Some(last_release) = last_release {
        // SAFETY: 无参数的只读系统查询；只分隔本实例的独立拖动，不改变双击设置。
        let delay =
            independent_click_delay(last_release.elapsed(), unsafe { GetDoubleClickTime() })?;
        // 同一边缘连续按下可能成为 WM_NCLBUTTONDBLCLK。按实际系统阈值等待，锁外执行。
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }
        check_input(main)?;
    }
    let start_absolute = absolute(start)?;
    let end_absolute = absolute(end)?;
    // SAFETY: WindowFromPoint 只读取输入命中窗口；IsChild 不解引用系统句柄。
    let hit = unsafe {
        WindowFromPoint(POINT {
            x: start.x,
            y: start.y,
        })
    };
    if hit != main && unsafe { IsChild(main, hit) } == 0 {
        return Err(("dragPointNotOwned", None).into());
    }
    let move_flags = MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK;
    // 先投递移动并读取真实落点，再验证将接收按下的窗口；父窗口的命中不能代表 WebView 子窗口。
    send(&[mouse(start_absolute, move_flags)])?;
    let deadline = Instant::now() + Duration::from_millis(250);
    loop {
        let mut cursor = POINT::default();
        // SAFETY: 初始化且有效的可写 POINT，仅查询实际指针坐标。
        if unsafe { GetCursorPos(&mut cursor) } == 0 {
            return Err(error("dragCursorPosition"));
        }
        if cursor.x == start.x && cursor.y == start.y {
            break;
        }
        if Instant::now() >= deadline {
            return Err(("dragCursorNotAtStart", None).into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    check_input(main)?;
    if let Some(expected) = expected_border
        && !border_hit(main, start, expected)?
    {
        return Err(("resizeBorderChanged", None).into());
    }
    let up = mouse(Point { x: 0, y: 0 }, MOUSEEVENTF_LEFTUP);
    let result = (|| {
        send(&[mouse(Point { x: 0, y: 0 }, MOUSEEVENTF_LEFTDOWN)])?;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let mut state = GUITHREADINFO {
                cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                ..Default::default()
            };
            let thread = owned_thread(main)?;
            // SAFETY: 初始化的 GUI 信息缓冲区，线程来自已验证自有窗口。
            if unsafe { GetGUIThreadInfo(thread, &mut state) } == 0 {
                return Err(error("dragGuiThread"));
            }
            if state.flags & GUI_INMOVESIZE != 0 {
                break;
            }
            if Instant::now() >= deadline {
                return Err(("systemMoveNotEntered", None).into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        for step in 1..=8 {
            // SAFETY: 每步检查自有窗口仍是前台；发生外部焦点切换就结束本次手势。
            if unsafe { GetForegroundWindow() } != main {
                return Err(("foregroundChangedDuringDrag", None).into());
            }
            let p = Point {
                x: start_absolute.x + (end_absolute.x - start_absolute.x) * step / 8,
                y: start_absolute.y + (end_absolute.y - start_absolute.y) * step / 8,
            };
            send(&[mouse(p, move_flags)])?;
            std::thread::sleep(Duration::from_millis(25));
        }
        Ok(())
    })();
    let released = release(&[up]);
    *LAST_DRAG_RELEASE
        .lock()
        .map_err(|_| SystemError::from(("dragTimingUnavailable", None)))? = Some(Instant::now());
    released?;
    result
}

fn resize_point(rect: &RECT, edge: ResizeEdge, inset: i32) -> Result<Point, SystemError> {
    let width = i64::from(rect.right) - i64::from(rect.left);
    let height = i64::from(rect.bottom) - i64::from(rect.top);
    if inset < 0 || width <= i64::from(inset) * 2 || height <= i64::from(inset) * 2 {
        return Err(("invalidResizeBounds", None).into());
    }
    let (dx, dy) = edge.axes();
    let x = match dx {
        -1 => i64::from(rect.left) + i64::from(inset),
        1 => i64::from(rect.right) - i64::from(inset.max(1)),
        _ => i64::from(rect.left) + width / 2,
    };
    let y = match dy {
        -1 => i64::from(rect.top) + i64::from(inset),
        1 => i64::from(rect.bottom) - i64::from(inset.max(1)),
        _ => i64::from(rect.top) + height / 2,
    };
    Ok(Point {
        x: x as i32,
        y: y as i32,
    })
}

fn border_hit(window: HWND, point: Point, expected: u32) -> Result<bool, SystemError> {
    let (Ok(x), Ok(y)) = (i16::try_from(point.x), i16::try_from(point.y)) else {
        return Err(("hitTestCoordinateOutOfRange", None).into());
    };
    // SAFETY: 仅查询真实输入目标；后续消息限定为自有窗口及其子窗口。
    let actual = unsafe {
        WindowFromPoint(POINT {
            x: point.x,
            y: point.y,
        })
    };
    if actual != window && unsafe { IsChild(window, actual) } == 0 {
        return Ok(false);
    }
    let packed = ((i32::from(y) << 16) | i32::from(x as u16)) as isize;
    let mut hit = 0;
    // SAFETY: WM_NCHITTEST 只读有符号短坐标；目标已验证自有，结果缓冲区有效，等待限于 250ms。
    if unsafe {
        SendMessageTimeoutW(
            actual,
            WM_NCHITTEST,
            0,
            packed,
            SMTO_ABORTIFHUNG,
            250,
            &mut hit,
        )
    } == 0
    {
        return Err(error("resizeHitTest"));
    }
    Ok(hit == expected as usize)
}

pub(super) fn resize(main: usize, edge: ResizeEdge, distance: i32) -> Result<(), SystemError> {
    let window = main as HWND;
    check_input(window)?;
    // SAFETY: 当前进程自有窗口，仅读取正常窗口状态。
    if unsafe { IsZoomed(window) } != 0
        || unsafe { IsIconic(window) } != 0
        || dialog(main)?.is_some()
    {
        return Err(("resizeRequiresNormalWindow", None).into());
    }
    let mut rect = RECT::default();
    // SAFETY: 自有窗口与初始化的可写 RECT。
    if unsafe { GetWindowRect(window, &mut rect) } == 0 {
        return Err(error("resizeWindowRect"));
    }
    let expected = match edge {
        ResizeEdge::Left => HTLEFT,
        ResizeEdge::Right => HTRIGHT,
        ResizeEdge::Top => HTTOP,
        ResizeEdge::Bottom => HTBOTTOM,
        ResizeEdge::TopLeft => HTTOPLEFT,
        ResizeEdge::TopRight => HTTOPRIGHT,
        ResizeEdge::BottomLeft => HTBOTTOMLEFT,
        ResizeEdge::BottomRight => HTBOTTOMRIGHT,
    };
    let mut start = None;
    // 有界探查真实边框，处理不可见 resize margin；只读命中结果，不派发合成缩放消息。
    for inset in [0, 1, 2, 4, 8, 16] {
        let point = resize_point(&rect, edge, inset)?;
        if border_hit(window, point, expected)? {
            start = Some(point);
            break;
        }
    }
    let start = start.ok_or(SystemError::from(("resizeBorderNotFound", None)))?;
    let (dx, dy) = edge.axes();
    let end = Point {
        x: start
            .x
            .checked_add(dx * distance)
            .ok_or(SystemError::from(("resizeCoordinateOverflow", None)))?,
        y: start
            .y
            .checked_add(dy * distance)
            .ok_or(SystemError::from(("resizeCoordinateOverflow", None)))?,
    };
    drag_to(main, start, end, Some(expected))
}

struct ComApartment;
impl Drop for ComApartment {
    fn drop(&mut self) {
        // SAFETY: 此守卫仅在当前线程成功初始化 COM 后创建，且不跨线程。
        unsafe {
            windows::Win32::System::Com::CoUninitialize();
        }
    }
}
fn uia_error(operation: &'static str) -> impl FnOnce(windows::core::Error) -> SystemError {
    move |e| SystemError {
        operation,
        code: Some(e.code().0 as u32),
    }
}
pub(super) fn choose_file(main: usize, path: &str) -> Result<(), SystemError> {
    use windows::{
        Win32::{
            Foundation::HWND as UiaWindow,
            System::{Com::*, Variant::VARIANT},
            UI::Accessibility::*,
        },
        core::Interface,
    };
    let popup = dialog(main)?.ok_or(SystemError::from(("filePickerNotOpen", None)))?;
    check_input(popup)?;
    // SAFETY: 在阻塞工作线程初始化 MTA；失败时不继续，守卫在接口释放之后退出。
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
        .ok()
        .map_err(uia_error("uiaInitialize"))?;
    let _apartment = ComApartment;
    // SAFETY: 固定系统 COM 类和接口，进程内上下文，没有调用者指定 CLSID。
    let automation: IUIAutomation =
        unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) }
            .map_err(uia_error("uiaCreate"))?;
    let limits: IUIAutomation2 = automation
        .cast()
        .map_err(uia_error("uiaTimeoutInterface"))?;
    // SAFETY: COM 接口在当前 MTA 内存活；超时限制提供者阻塞，不延长测试门槛。
    unsafe { limits.SetConnectionTimeout(1000) }.map_err(uia_error("uiaConnectionTimeout"))?;
    // SAFETY: 同上，仅设置本次 UIA 客户端超时。
    unsafe { limits.SetTransactionTimeout(1000) }.map_err(uia_error("uiaTransactionTimeout"))?;
    // SAFETY: popup 是已验证自有原生对话框，不在桌面或其他进程搜索。
    let root = unsafe { automation.ElementFromHandle(UiaWindow(popup)) }
        .map_err(uia_error("uiaDialogRoot"))?;
    // SAFETY: 固定控件类型与合法 VARIANT；UIA 负责返回对象的生命周期。
    let edit_type = unsafe {
        automation.CreatePropertyCondition(
            UIA_ControlTypePropertyId,
            &VARIANT::from(UIA_EditControlTypeId.0),
        )
    }
    .map_err(uia_error("uiaEditCondition"))?;
    // 目录内容也可能暴露可编辑控件，先按固定文件名 ID 收窄，不能先枚举所有 Edit
    // 再提高数量上限。仍在自有对话框子树检索，且保留类型、唯一性和可见性验证。
    // SAFETY: 固定属性和值，VARIANT 在各 COM 调用期间有效。
    let filename_id = unsafe {
        automation.CreatePropertyCondition(UIA_AutomationIdPropertyId, &VARIANT::from("1148"))
    }
    .map_err(uia_error("uiaFileNameCondition"))?;
    // SAFETY: 同上，兼容 Common Item Dialog 的另一个固定文件名 Edit ID。
    let alternate_id = unsafe {
        automation.CreatePropertyCondition(UIA_AutomationIdPropertyId, &VARIANT::from("1001"))
    }
    .map_err(uia_error("uiaFileNameAlternateCondition"))?;
    // SAFETY: 条件对象由当前 MTA 持有，COM 为组合条件保留引用。
    let ids = unsafe { automation.CreateOrCondition(&filename_id, &alternate_id) }
        .map_err(uia_error("uiaFileNameIds"))?;
    // SAFETY: 同上，类型与 ID 同时满足才能成为候选。
    let filter = unsafe { automation.CreateAndCondition(&edit_type, &ids) }
        .map_err(uia_error("uiaFileNameFilter"))?;
    // SAFETY: 只检索已验证对话框的子树，系统提供者无业务状态写入。
    let edits = unsafe { root.FindAll(TreeScope_Descendants, &filter) }
        .map_err(uia_error("uiaDialogEdits"))?;
    // SAFETY: edits 是返回的有效 COM 数组。
    let count = unsafe { edits.Length() }.map_err(uia_error("uiaEditCount"))?;
    if !(0..=32).contains(&count) {
        return Err(("uiaEditLimit", None).into());
    }
    let mut candidates = Vec::new();
    for index in 0..count {
        // SAFETY: index 已在数组长度内，元素由 COM 引用计数持有。
        let edit = unsafe { edits.GetElement(index) }.map_err(uia_error("uiaEdit"))?;
        // SAFETY: 只读标识与可见／可用状态，不读取外部窗口文字。
        let id = unsafe { edit.CurrentAutomationId() }
            .map_err(uia_error("uiaEditId"))?
            .to_string();
        // Common Item Dialog 的文件名字段；不按本地化的显示文字匹配。
        if id != "1148" && id != "1001" {
            continue;
        }
        // SAFETY: 已取到的元素，固定只读属性调用。
        if unsafe { edit.CurrentIsEnabled() }
            .map_err(uia_error("uiaEditEnabled"))?
            .as_bool()
            && !unsafe { edit.CurrentIsOffscreen() }
                .map_err(uia_error("uiaEditOffscreen"))?
                .as_bool()
        {
            candidates.push(edit);
        }
    }
    if candidates.len() != 1 {
        return Err(("fileNameControlNotUnique", None).into());
    }
    let edit = &candidates[0];
    check_input(popup)?;
    // SAFETY: 在自有前台文件对话框中请求真实焦点，不直接设置控件值。
    unsafe { edit.SetFocus() }.map_err(uia_error("uiaFileNameFocus"))?;
    let focus_deadline = Instant::now() + Duration::from_secs(2);
    loop {
        // SAFETY: 只读已验证文件名控件的真实焦点；不模拟输入或反复抢焦点。
        if unsafe { edit.CurrentHasKeyboardFocus() }
            .map_err(uia_error("uiaFileNameFocused"))?
            .as_bool()
        {
            break;
        }
        if Instant::now() >= focus_deadline {
            return Err(("fileNameFocusNotObserved", None).into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    chord(popup, VK_CONTROL, 0x41)?;
    let mut input = Vec::new();
    for code in path.encode_utf16() {
        input.push(keyboard(0, code, KEYEVENTF_UNICODE));
        input.push(keyboard(0, code, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
    }
    let release_deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match check_input(popup) {
            Ok(()) => break,
            Err(e)
                if e.operation == "physicalInputAlreadyHeld"
                    && Instant::now() < release_deadline =>
            {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(e) => return Err(e),
        }
    }
    if let Err(e) = send(&input) {
        let ups: Vec<_> = input.iter().skip(1).step_by(2).copied().collect();
        release(&ups)?;
        return Err(e);
    }
    // SAFETY: 只读 ValuePattern，用于确认真实键盘输入已到达，不调用 SetValue。
    let value: IUIAutomationValuePattern = unsafe { edit.GetCurrentPatternAs(UIA_ValuePatternId) }
        .map_err(uia_error("uiaFileNameValue"))?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        // SAFETY: 有效只读模式，值仅在此比较，不写入报告。
        if unsafe { value.CurrentValue() }.map_err(uia_error("uiaFileNameRead"))? == path {
            break;
        }
        if Instant::now() >= deadline {
            return Err(("fileNameInputNotObserved", None).into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    check_input(popup)?;
    let result = send(&key_pair(VK_RETURN));
    if result.is_err() {
        release(&[keyboard(VK_RETURN, 0, KEYEVENTF_KEYUP)])?;
    }
    result
}

#[cfg(test)]
mod resize_tests {
    use super::*;
    #[test]
    fn absolute_coordinates_land_inside_each_pixel_of_negative_origin_desktops() {
        for extent in [2, 1600, 4480, 65536] {
            for pixel in 0..extent {
                let absolute = absolute_axis(pixel - 1920, -1920, extent).unwrap();
                assert_eq!(
                    i64::from(absolute) * i64::from(extent) / 65536,
                    i64::from(pixel)
                );
            }
        }
        for (coordinate, origin, extent) in [
            (0, 0, 1),
            (0, 0, 65537),
            (-1, 0, 100),
            (100, 0, 100),
            (i32::MAX, i32::MIN, 100),
        ] {
            assert!(absolute_axis(coordinate, origin, extent).is_err());
        }
    }
    #[test]
    fn consecutive_drags_wait_out_the_actual_double_click_interval() {
        assert_eq!(
            independent_click_delay(Duration::from_millis(100), 500).unwrap(),
            Duration::from_millis(401)
        );
        assert_eq!(
            independent_click_delay(Duration::from_millis(500), 500).unwrap(),
            Duration::from_millis(1)
        );
        assert_eq!(
            independent_click_delay(Duration::from_millis(501), 500).unwrap(),
            Duration::ZERO
        );
        assert!(independent_click_delay(Duration::ZERO, 5001).is_err());
    }
    #[test]
    fn border_points_preserve_negative_monitor_coordinates_and_reject_degenerate_bounds() {
        let rect = RECT {
            left: -1200,
            top: -200,
            right: -200,
            bottom: 500,
        };
        for (edge, x, y) in [
            (ResizeEdge::Left, -1198, 150),
            (ResizeEdge::Right, -202, 150),
            (ResizeEdge::Top, -700, -198),
            (ResizeEdge::Bottom, -700, 498),
            (ResizeEdge::TopLeft, -1198, -198),
            (ResizeEdge::TopRight, -202, -198),
            (ResizeEdge::BottomLeft, -1198, 498),
            (ResizeEdge::BottomRight, -202, 498),
        ] {
            let p = resize_point(&rect, edge, 2).unwrap();
            assert_eq!((p.x, p.y), (x, y));
        }
        assert!(resize_point(&rect, ResizeEdge::Left, 500).is_err());
        assert!(resize_point(&rect, ResizeEdge::Top, -1).is_err());
        let top = resize_point(&rect, ResizeEdge::Top, 0).unwrap();
        assert_eq!((top.x, top.y), (-700, -200));
        let corner = resize_point(&rect, ResizeEdge::BottomRight, 0).unwrap();
        assert_eq!((corner.x, corner.y), (-201, 499));
        assert!(resize_point(&RECT::default(), ResizeEdge::Top, 1).is_err());
    }
}
