//! 从 Windows 任务栏真实点击本测试实例；唯一标题来自自有 HWND，不接收调用者选择器。
use super::*;

fn app_id(instance: &str) -> Result<String, SystemError> {
    if !test_title(&format!("LayerLens · 测试 {instance}")) {
        return Err(("invalidTaskbarInstance", None).into());
    }
    Ok(format!("LayerLens.Test.{instance}"))
}

pub(super) fn set_identity(instance: &str) -> Result<(), SystemError> {
    let value: Vec<u16> = app_id(instance)?.encode_utf16().chain(Some(0)).collect();
    // SAFETY: 已校验的 UUID 生成有界 NUL 结尾 ID，仅修改调用进程的任务栏身份。
    let result = unsafe {
        windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(value.as_ptr())
    };
    if result < 0 {
        return Err(("setTaskbarIdentity", Some(result as u32)).into());
    }
    Ok(())
}
use windows::{
    Win32::{
        Foundation::HWND as UiaWindow,
        System::{Com::*, Variant::VARIANT},
        UI::Accessibility::*,
    },
    core::Interface,
};

pub(super) fn restore(main: usize) -> Result<(), SystemError> {
    let window = main as HWND;
    let thread = owned_thread(window)?;
    // SAFETY: 已验证自有句柄；只读最小化与当前线程输入桌面。
    if unsafe { IsIconic(window) } == 0
        || !desktop_receives_input(thread)?
        || !desktop_receives_input(unsafe { GetCurrentThreadId() })?
    {
        return Err(("taskbarRequiresMinimizedInputDesktop", None).into());
    }
    for key in [
        VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN, VK_LBUTTON, VK_RBUTTON, VK_MBUTTON,
    ] {
        // SAFETY: 固定合法键值，读取当前按下状态。
        if unsafe { GetAsyncKeyState(i32::from(key)) } < 0 {
            return Err(("physicalInputAlreadyHeld", None).into());
        }
    }
    let mut text = [0u16; 160];
    // SAFETY: 自有窗口及有效可写 UTF-16 缓冲区。
    let count = unsafe { GetWindowTextW(window, text.as_mut_ptr(), text.len() as i32) };
    if count <= 0 {
        return Err(error("taskbarOwnTitle"));
    }
    let title = String::from_utf16_lossy(&text[..count as usize]);
    // 测试构建的标题含完整随机实例 UUID；普通应用标题不能作为外部目标依据。
    if !test_title(&title) {
        return Err(("taskbarTitleNotUnique", None).into());
    }
    let identity = app_id(title.trim_start_matches("LayerLens · 测试 "))?;
    let class: Vec<u16> = "Shell_TrayWnd".encode_utf16().chain(Some(0)).collect();
    // SAFETY: 固定系统任务栏类；不按调用者提供的窗口名搜索。
    let taskbar = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
    if taskbar.is_null() {
        return Err(("taskbarUnavailable", None).into());
    }
    // SAFETY: 当前阻塞线程初始化 MTA，守卫在接口释放后撤销初始化。
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
        .ok()
        .map_err(uia_error("uiaTaskbarInitialize"))?;
    let _apartment = ComApartment;
    // SAFETY: 固定 Windows UI Automation 类和接口。
    let automation: IUIAutomation =
        unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) }
            .map_err(uia_error("uiaTaskbarCreate"))?;
    let limits: IUIAutomation2 = automation
        .cast()
        .map_err(uia_error("uiaTaskbarTimeoutInterface"))?;
    // SAFETY: 仅限制本次 COM 客户端连接／事务，不修改系统设置。
    unsafe { limits.SetConnectionTimeout(1000) }
        .map_err(uia_error("uiaTaskbarConnectionTimeout"))?;
    // SAFETY: 有效 COM 客户端及有界超时。
    unsafe { limits.SetTransactionTimeout(1000) }
        .map_err(uia_error("uiaTaskbarTransactionTimeout"))?;
    // SAFETY: 固定系统任务栏句柄，仅读取该子树的按钮。
    let root = unsafe { automation.ElementFromHandle(UiaWindow(taskbar)) }
        .map_err(uia_error("uiaTaskbarRoot"))?;
    // SAFETY: 固定按钮类型属性与有效 VARIANT。
    let filter = unsafe {
        automation.CreatePropertyCondition(
            UIA_ControlTypePropertyId,
            &VARIANT::from(UIA_ButtonControlTypeId.0),
        )
    }
    .map_err(uia_error("uiaTaskbarFilter"))?;
    // SAFETY: 仅固定任务栏子树，结果不对外暴露其他应用名称。
    let elements = unsafe { root.FindAll(TreeScope_Descendants, &filter) }
        .map_err(uia_error("uiaTaskbarButtons"))?;
    // SAFETY: 系统返回的有效 COM 数组。
    let count = unsafe { elements.Length() }.map_err(uia_error("uiaTaskbarCount"))?;
    if !(0..=256).contains(&count) {
        return Err(("taskbarItemLimit", None).into());
    }
    let mut candidates = Vec::new();
    for index in 0..count {
        // SAFETY: 数组索引已校验。
        let element =
            unsafe { elements.GetElement(index) }.map_err(uia_error("uiaTaskbarElement"))?;
        // SAFETY: 只在内存中与自有窗口的唯一标题匹配，不记录外部名称。
        let name = unsafe { element.CurrentName() }
            .map_err(uia_error("uiaTaskbarName"))?
            .to_string();
        // Windows 11 可能以应用分组名呈现按钮；唯一 AppUserModelID 仍能区分测试实例。
        // SAFETY: 固定任务栏元素的只读属性；不向外返回 AutomationId。
        let automation_id = unsafe { element.CurrentAutomationId() }
            .map_err(uia_error("uiaTaskbarAutomationId"))?
            .to_string();
        if !name.contains(&title) && !automation_id.contains(&identity) && !name.contains(&identity)
        {
            continue;
        }
        // SAFETY: 已匹配的系统元素，查询可见和启用状态。
        if unsafe { element.CurrentIsOffscreen() }
            .map_err(uia_error("uiaTaskbarOffscreen"))?
            .as_bool()
            || !unsafe { element.CurrentIsEnabled() }
                .map_err(uia_error("uiaTaskbarEnabled"))?
                .as_bool()
        {
            continue;
        }
        candidates.push(element);
    }
    if candidates.len() != 1 {
        let reason = if count == 0 {
            "taskbarButtonsUnavailable"
        } else if candidates.is_empty() {
            "taskbarInstanceMissing"
        } else {
            "taskbarInstanceAmbiguous"
        };
        return Err((reason, None).into());
    }
    // SAFETY: 唯一自有测试按钮，仅取实际屏幕矩形。
    let rect =
        unsafe { candidates[0].CurrentBoundingRectangle() }.map_err(uia_error("uiaTaskbarRect"))?;
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return Err(("taskbarButtonInvisible", None).into());
    }
    let point = Point {
        x: rect.left + (rect.right - rect.left) / 2,
        y: rect.top + (rect.bottom - rect.top) / 2,
    };
    // SAFETY: 点击前确认实际输入目标仍属于任务栏根，防止遮挡或会话变化。
    let hit = unsafe {
        WindowFromPoint(POINT {
            x: point.x,
            y: point.y,
        })
    };
    if hit != taskbar && unsafe { GetAncestor(hit, GA_ROOT) } != taskbar {
        return Err(("taskbarButtonOccluded", None).into());
    }
    let absolute = absolute(point)?;
    let result = send(&[
        mouse(
            absolute,
            MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
        ),
        mouse(Point { x: 0, y: 0 }, MOUSEEVENTF_LEFTDOWN),
    ]);
    release(&[mouse(Point { x: 0, y: 0 }, MOUSEEVENTF_LEFTUP)])?;
    result
}

fn test_title(title: &str) -> bool {
    title.strip_prefix("LayerLens · 测试 ").is_some_and(|id| {
        id.len() == 36
            && id.bytes().enumerate().all(|(i, b)| {
                if [8, 13, 18, 23].contains(&i) {
                    b == b'-'
                } else {
                    b.is_ascii_hexdigit()
                }
            })
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_instance_uuid_titles_can_identify_taskbar_targets() {
        assert_eq!(
            app_id("11111111-1111-4111-8111-111111111111").unwrap(),
            "LayerLens.Test.11111111-1111-4111-8111-111111111111"
        );
        assert!(app_id("ordinary").is_err());
        assert!(test_title(
            "LayerLens · 测试 11111111-1111-4111-8111-111111111111"
        ));
        for name in [
            "LayerLens",
            "LayerLens · 测试 ordinary",
            "LayerLens · 测试 11111111/1111-4111-8111-111111111111",
        ] {
            assert!(!test_title(name));
        }
        assert!(restore(0).is_err());
    }
}
