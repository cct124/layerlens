//! 语义定位白名单；外部参数不能变为任意 CSS、XPath 或脚本。
use crate::workspace::error;
use layerlens_core::workspace_contract::{WorkspaceError, WorkspaceErrorCode};
pub(super) fn selector(target: &str) -> Result<String, WorkspaceError> {
    let fixed = [
        "render:rebuild",
        "render:auto",
        "render:next",
        "render:100",
        "render:200",
        "render:400",
        "render:reset",
        "render:release",
        "render:shared",
        "render:native",
        "render:clear-metrics",
        "render:large",
        "render:compact",
        "render:strict",
        "render:source-layers",
        "render:fit",
        "render:focus",
        "window:minimize",
        "window:maximize",
        "window:close",
        "window:drag",
        "menu:file",
        "menu:view",
        "menu:window",
        "command:reload",
        "command:close",
        "command:cleanup",
        "command:fit",
        "command:actual",
        "command:properties",
        "command:document",
        "command:layers",
        "command:tasks",
        "command:resetLayout",
        "tool:pan",
        "tool:region",
        "panel:layers",
        "panel:tasks",
        "panel:properties",
        "panel:document",
        "task:name",
        "task:create",
        "task:refresh",
        "task:more",
        "cleanup:cancel",
        "cleanup:confirm",
        "cleanup:recheck",
        "cleanup:sync",
        "cleanup:tasks",
        "canvas",
        "canvas:reframe",
        "canvas:auto",
        "canvas:next",
        "layers:search",
        "canvas:snap",
        "range:clear",
        "range:view",
    ];
    let parts: Vec<_> = target.split(':').collect();
    let valid_id = |s: &str| super::observation::number(s).is_ok_and(|n| n > 0);
    let dynamic = match parts.as_slice() {
        ["document", id] => valid_id(id),
        ["task", id, action] => valid_id(id) && ["view", "release", "cleanup"].contains(action),
        ["layer", id, action] => {
            super::observation::number(id).is_ok() && ["select", "check", "toggle"].contains(action)
        }
        ["handle", edge] => ["nw", "n", "ne", "e", "se", "s", "sw", "w"].contains(edge),
        _ => false,
    };
    if fixed.contains(&target) || dynamic {
        Ok(format!("[data-dev-target=\"{target}\"]"))
    } else {
        Err(error(
            WorkspaceErrorCode::InvalidInput,
            "未知界面目标；只接受已实现的语义标识。",
        ))
    }
}
