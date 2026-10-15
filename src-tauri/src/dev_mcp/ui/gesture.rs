//! 将受限画布手势校验为一批 W3C 输入；非法顺序不能产生部分输入。
use super::{driver::validate_canvas_input, model::*};
use crate::workspace::error;
use layerlens_core::workspace_contract::{WorkspaceError, WorkspaceErrorCode};
use serde_json::{Value, json};

/// 所有位置及按钮状态在派发前验证；移步固定 100 ms，整个请求至多 32 步。
pub(super) fn actions(
    before: &UiObservation,
    steps: &[GestureStep],
    hold: bool,
) -> Result<Value, WorkspaceError> {
    let invalid = || {
        error(
            WorkspaceErrorCode::InvalidInput,
            "画布手势步骤或按键顺序无效。",
        )
    };
    if steps.is_empty() || steps.len() > 32 || !matches!(steps[0], GestureStep::Move { .. }) {
        return Err(invalid());
    }
    let mut buttons = 0_u8;
    let mut pressed = false;
    let mut pointer = Vec::new();
    let mut keyboard = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        let value = match *step {
            GestureStep::Move { x, y } => {
                validate_canvas_input(before, &[(x, y)])?;
                json!({"type":"pointerMove","origin":"viewport","x":x,"y":y,"duration":if index == 0 { 0 } else { 100 }})
            }
            GestureStep::Down { button } | GestureStep::Up { button } => {
                let number = match button {
                    PointerButton::Left => 0,
                    PointerButton::Middle => 1,
                };
                let mask = 1 << number;
                let down = matches!(step, GestureStep::Down { .. });
                if down == (buttons & mask != 0) {
                    return Err(invalid());
                }
                buttons ^= mask;
                pressed |= down;
                json!({"type":if down { "pointerDown" } else { "pointerUp" },"button":number})
            }
            GestureStep::Escape => {
                if buttons == 0 {
                    return Err(invalid());
                }
                pointer.push(json!({"type":"pause","duration":0}));
                keyboard.push(json!({"type":"keyDown","value":"\u{e00c}"}));
                pointer.push(json!({"type":"pause","duration":0}));
                keyboard.push(json!({"type":"keyUp","value":"\u{e00c}"}));
                continue;
            }
        };
        pointer.push(value);
        keyboard.push(json!({"type":"pause","duration":0}));
    }
    if !pressed || hold != (buttons != 0) {
        return Err(invalid());
    }
    Ok(json!({"actions":[
        {"type":"pointer","id":"layerlens-pointer","parameters":{"pointerType":"mouse"},"actions":pointer},
        {"type":"key","id":"layerlens-keyboard","actions":keyboard}
    ]}))
}
